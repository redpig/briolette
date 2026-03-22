#![no_std]
#![no_main]

mod config;
mod led;
mod pn7150;
mod power;
mod transaction;

use defmt_rtt as _;
use panic_probe as _;

use embassy_executor::Spawner;
use embassy_nrf::gpio::{Input, Level, Output, OutputDrive, Pull};
use embassy_nrf::peripherals;
use embassy_nrf::{bind_interrupts, twim};
use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::{Duration, Timer};
use static_cell::StaticCell;

use crate::config::{Config, Mode};
use crate::led::{Color, Leds, Pattern};
use crate::pn7150::Pn7150;
use crate::power::{ChargeLevel, Power};
use crate::transaction::{Phase, TransactionEngine};

// Re-export the library crate's keypad module.
use briolette_relay_firmware::keypad::{AmountEntry, Key, KeyEvent, Keypad};

bind_interrupts!(struct Irqs {
    SPIM0_SPIS0_TWIM0_TWIS0_SPI0_TWI0 => twim::InterruptHandler<peripherals::TWISPI0>;
});

/// Events from peripherals to the main coordinator.
pub enum Event {
    /// Keypad key pressed.
    KeyPress(Key),
    /// Start transaction (OK pressed with amount, or event mode trigger).
    StartTransaction { amount: u32 },
    /// NFC tag detected during polling.
    TagDetected,
    /// NFC tag lost.
    TagLost,
    /// Power state changed.
    PowerChanged(ChargeLevel),
    /// Enter config mode (long-press on power-on).
    EnterConfig,
    /// Clear/cancel current operation.
    Cancel,
}

/// Shared event channel.
static EVENT_CHANNEL: Channel<ThreadModeRawMutex, Event, 8> = Channel::new();

/// Maximum time to wait for a tag during polling.
const TAG_POLL_TIMEOUT: Duration = Duration::from_secs(30);

/// Time to wait for sender re-tap (after INITIATE, before TRANSFER).
const SENDER_RETAP_TIMEOUT: Duration = Duration::from_secs(60);

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_nrf::init(Default::default());
    defmt::info!("Briolette solar relay firmware starting");

    // --- Load configuration from flash ---
    let config = Config::load_from_flash();
    defmt::info!("Mode: {}", config.mode);

    // --- Initialize I2C bus (shared by PN7150 + TCA8418) ---
    let twi_config = twim::Config::default();
    let twi = twim::Twim::new(p.TWISPI0, Irqs, p.P0_26, p.P0_27, twi_config);

    // --- Keypad scanner (TCA8418) control pins ---
    // KEY_INT on P0.09: interrupt output from TCA8418 (active low, open drain)
    let _key_int = Input::new(p.P0_09, Pull::Up);
    // KEY_RST on P0.10: reset input to TCA8418 (active low)
    let _key_rst = Output::new(p.P0_10, Level::High, OutputDrive::Standard);
    // TODO: Share the I2C bus between PN7150 and TCA8418 using
    // embassy_embedded_hal::shared_bus, then initialize the Keypad driver
    // and spawn a keypad_task to populate EVENT_CHANNEL with KeyPress events.

    // --- Initialize PN7150 NFC reader ---
    let nfc_irq = Input::new(p.P0_19, Pull::Up);
    let nfc_ven = Output::new(p.P0_20, Level::Low, OutputDrive::Standard);
    let mut nfc = Pn7150::new(twi, nfc_irq, nfc_ven);

    // --- Initialize LEDs ---
    let mut leds = Leds::new(
        Output::new(p.P0_06, Level::High, OutputDrive::Standard),
        Output::new(p.P0_07, Level::High, OutputDrive::Standard),
        Output::new(p.P0_08, Level::High, OutputDrive::Standard),
    );

    // --- Initialize power monitor ---
    let power = Power::new(Input::new(p.P0_04, Pull::None));

    // --- Initialize transaction engine ---
    let mut tx_engine = TransactionEngine::new();

    // Pre-load cached receiver ticket in MerchantPos / EventMode.
    if config.has_saved_receiver() {
        tx_engine.set_cached_receiver_ticket(&config.receiver_ticket);
        defmt::info!("Receiver ticket cached from flash config");
    }

    // --- Amount entry state ---
    let mut amount_entry = AmountEntry::new();

    // Boot indication.
    match power.charge_level() {
        ChargeLevel::Ok => leds.play(Pattern::Pulse(Color::Green, 200)).await,
        ChargeLevel::Low => leds.play(Pattern::Blink(Color::Red, 2, 200, 200)).await,
    }

    // Initialize NFC reader.
    match nfc.init().await {
        Ok(()) => defmt::info!("PN7150 ready"),
        Err(e) => {
            defmt::error!("PN7150 init failed: {}", e);
            leds.play(led::patterns::FAILURE).await;
        }
    }

    defmt::info!("Solar relay ready, entering main loop");

    // --- Main event loop ---
    //
    // Three modes:
    //   Variable:    operator enters amount + taps both wallets each time
    //   MerchantPos: operator enters amount, receiver is cached
    //   EventMode:   amount + receiver both cached, operator just presses OK
    //
    // No PIN or authorization on the relay. The credstick handles its own
    // security (PIN, e-ink confirmation).
    loop {
        // Check power before doing anything expensive.
        if !power.can_transact() {
            defmt::warn!("Low power, waiting for charge");
            leds.play(led::patterns::LOW_POWER).await;
            Timer::after(Duration::from_secs(5)).await;
            continue;
        }

        // Determine transaction amount based on mode.
        let amount = match config.mode {
            Mode::EventMode => {
                // Fixed amount — just wait for OK press to trigger.
                leds.set(Color::Green);
                defmt::info!("Event mode: press OK to charge {} tokens", config.fixed_amount);

                // Wait for OK press via event channel.
                loop {
                    let event = EVENT_CHANNEL.receive().await;
                    match event {
                        Event::KeyPress(Key::Ok) | Event::StartTransaction { .. } => {
                            break config.fixed_amount;
                        }
                        Event::Cancel => continue,
                        _ => continue,
                    }
                }
            }
            Mode::MerchantPos | Mode::Variable => {
                // Variable amount — operator enters via keypad.
                leds.set(Color::Green);
                defmt::info!("Enter amount on keypad, press OK to confirm");

                loop {
                    let event = EVENT_CHANNEL.receive().await;
                    match event {
                        Event::KeyPress(key) => {
                            if let Some(cents) = amount_entry.process(key) {
                                break cents as u32;
                            }
                        }
                        Event::Cancel => {
                            amount_entry.process(Key::Clear);
                            continue;
                        }
                        _ => continue,
                    }
                }
            }
        };

        if amount == 0 {
            continue;
        }

        // --- Execute transaction flow ---
        tx_engine.start(amount);

        // Step 1: Acquire receiver ticket (skip if cached).
        if !tx_engine.has_cached_receiver() {
            defmt::info!("Tap receiver credstick...");
            leds.play(led::patterns::WAITING_SENDER).await;

            if let Err(e) = nfc.start_polling().await {
                defmt::error!("Failed to start polling: {}", e);
                leds.play(led::patterns::FAILURE).await;
                continue;
            }

            match nfc.wait_for_tag(TAG_POLL_TIMEOUT).await {
                Ok(()) => {
                    leds.set(Color::Blue);

                    match nfc.select_briolette_applet().await {
                        Ok(true) => {}
                        _ => {
                            defmt::warn!("Not a Briolette credstick");
                            nfc.deactivate().await.ok();
                            leds.play(led::patterns::FAILURE).await;
                            continue;
                        }
                    }

                    let read_ticket_apdu = tx_engine.build_read_ticket();
                    match nfc.transceive_apdu(&read_ticket_apdu).await {
                        Ok(resp) => {
                            if tx_engine.handle_read_ticket_response(&resp).is_err() {
                                nfc.deactivate().await.ok();
                                leds.play(led::patterns::FAILURE).await;
                                continue;
                            }
                        }
                        Err(e) => {
                            defmt::error!("READ_TICKET failed: {}", e);
                            nfc.deactivate().await.ok();
                            leds.play(led::patterns::FAILURE).await;
                            continue;
                        }
                    }

                    nfc.deactivate().await.ok();
                    defmt::info!("Receiver ticket acquired");
                }
                Err(_) => {
                    defmt::info!("No receiver tag detected (timeout)");
                    leds.play(led::patterns::FAILURE).await;
                    continue;
                }
            }
        }

        // Step 2: Tap sender — INITIATE.
        defmt::info!("Tap sender credstick... ({} tokens)", amount);
        leds.play(Pattern::Blink(Color::Blue, 2, 150, 150)).await;

        if let Err(e) = nfc.start_polling().await {
            defmt::error!("Failed to start polling: {}", e);
            leds.play(led::patterns::FAILURE).await;
            continue;
        }

        match nfc.wait_for_tag(TAG_POLL_TIMEOUT).await {
            Ok(()) => {
                leds.set(Color::Blue);

                match nfc.select_briolette_applet().await {
                    Ok(true) => {}
                    _ => {
                        defmt::warn!("Not a Briolette credstick");
                        nfc.deactivate().await.ok();
                        leds.play(led::patterns::FAILURE).await;
                        continue;
                    }
                }

                let initiate_apdu = tx_engine.build_initiate();
                match nfc.transceive_apdu(&initiate_apdu).await {
                    Ok(resp) => {
                        if tx_engine.handle_initiate_response(&resp).is_err() {
                            nfc.deactivate().await.ok();
                            leds.play(led::patterns::FAILURE).await;
                            continue;
                        }
                    }
                    Err(e) => {
                        defmt::error!("INITIATE failed: {}", e);
                        nfc.deactivate().await.ok();
                        leds.play(led::patterns::FAILURE).await;
                        continue;
                    }
                }

                nfc.deactivate().await.ok();
            }
            Err(_) => {
                defmt::info!("No sender tag detected (timeout)");
                leds.play(led::patterns::FAILURE).await;
                continue;
            }
        }

        // Step 3: Wait for sender re-tap — TRANSFER (get signed tokens).
        defmt::info!("Waiting for sender re-tap (TRANSFER)...");
        leds.play(led::patterns::WAITING_SENDER).await;

        if let Err(e) = nfc.start_polling().await {
            defmt::error!("Failed to start polling: {}", e);
            leds.play(led::patterns::FAILURE).await;
            continue;
        }

        match nfc.wait_for_tag(SENDER_RETAP_TIMEOUT).await {
            Ok(()) => {
                leds.set(Color::Blue);

                match nfc.select_briolette_applet().await {
                    Ok(true) => {}
                    _ => {
                        nfc.deactivate().await.ok();
                        leds.play(led::patterns::FAILURE).await;
                        continue;
                    }
                }

                let transfer_apdu = tx_engine.build_transfer();
                match nfc.transceive_apdu(&transfer_apdu).await {
                    Ok(resp) => {
                        if tx_engine.handle_transfer_response(&resp).is_err() {
                            if tx_engine.phase() == Phase::SenderProposed {
                                defmt::info!("PIN required, sender must enter PIN and re-tap");
                                nfc.deactivate().await.ok();
                                leds.play(Pattern::Blink(Color::Blue, 1, 500, 500)).await;
                            } else {
                                nfc.deactivate().await.ok();
                                leds.play(led::patterns::FAILURE).await;
                            }
                            continue;
                        }
                    }
                    Err(e) => {
                        defmt::error!("TRANSFER failed: {}", e);
                        nfc.deactivate().await.ok();
                        leds.play(led::patterns::FAILURE).await;
                        continue;
                    }
                }

                nfc.deactivate().await.ok();
            }
            Err(_) => {
                defmt::info!("Sender re-tap timeout");
                leds.play(led::patterns::FAILURE).await;
                continue;
            }
        }

        // Step 4: Deliver signed tokens to receiver.
        defmt::info!("Tap receiver to deliver tokens...");
        leds.play(Pattern::Blink(Color::Green, 2, 150, 150)).await;

        if let Err(e) = nfc.start_polling().await {
            defmt::error!("Failed to start polling: {}", e);
            leds.play(led::patterns::FAILURE).await;
            continue;
        }

        match nfc.wait_for_tag(TAG_POLL_TIMEOUT).await {
            Ok(()) => {
                leds.set(Color::Blue);

                match nfc.select_briolette_applet().await {
                    Ok(true) => {}
                    _ => {
                        nfc.deactivate().await.ok();
                        leds.play(led::patterns::FAILURE).await;
                        continue;
                    }
                }

                let receive_apdu = tx_engine.build_receive();
                match nfc.transceive_apdu(&receive_apdu).await {
                    Ok(resp) => {
                        if tx_engine.handle_receive_response(&resp).is_ok() {
                            leds.play(led::patterns::SUCCESS).await;
                            defmt::info!(
                                "Transaction #{} complete!",
                                tx_engine.tx_count()
                            );
                        } else {
                            leds.play(led::patterns::FAILURE).await;
                        }
                    }
                    Err(e) => {
                        defmt::error!("RECEIVE failed: {}", e);
                        nfc.deactivate().await.ok();
                        leds.play(led::patterns::FAILURE).await;
                        continue;
                    }
                }

                nfc.deactivate().await.ok();
            }
            Err(_) => {
                defmt::info!("No receiver tag for delivery (timeout)");
                leds.play(led::patterns::FAILURE).await;
                continue;
            }
        }

        // Brief pause before next transaction.
        Timer::after(Duration::from_secs(1)).await;
    }
}
