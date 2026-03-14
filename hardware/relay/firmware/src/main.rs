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
    /// Start transaction (OK pressed with amount, or merchant POS trigger).
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

/// Idle status pulse interval.
const IDLE_PULSE_INTERVAL: Duration = Duration::from_secs(10);

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

    // --- Initialize PN7150 NFC reader ---
    // IRQ = P0_19 (active-low), VEN = P0_20
    let nfc_irq = Input::new(p.P0_19, Pull::Up);
    let nfc_ven = Output::new(p.P0_20, Level::Low, OutputDrive::Standard);
    let mut nfc = Pn7150::new(twi, nfc_irq, nfc_ven);

    // --- Initialize LEDs ---
    // Red = P0_06, Green = P0_07, Blue = P0_08 (active-low)
    let mut leds = Leds::new(
        Output::new(p.P0_06, Level::High, OutputDrive::Standard),
        Output::new(p.P0_07, Level::High, OutputDrive::Standard),
        Output::new(p.P0_08, Level::High, OutputDrive::Standard),
    );

    // --- Initialize power monitor ---
    // VBAT_OK = P0_04
    let power = Power::new(Input::new(p.P0_04, Pull::None));

    // --- Initialize transaction engine ---
    let mut tx_engine = TransactionEngine::new();

    // Pre-load cached receiver ticket in merchant POS mode.
    if config.is_merchant_pos() {
        tx_engine.set_cached_receiver_ticket(&config.receiver_ticket);
        defmt::info!(
            "Merchant POS mode: {} tokens, receiver cached",
            config.fixed_amount
        );
    }

    // --- Initialize keypad (TCA8418 over I2C) ---
    // The keypad shares the I2C bus. In a production design, we'd use
    // an I2C bus multiplexer or coordinate access. For now, the keypad
    // is initialized separately and polled via interrupt.
    //
    // TCA8418 INT = P0_21
    // Note: The TCA8418 Keypad driver takes ownership of the I2C bus.
    // Since PN7150 also needs I2C, in production we'd share via a mutex.
    // For this firmware, NFC and keypad operations are mutually exclusive
    // (you're either entering an amount OR polling for tags).

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
    // The relay has two primary modes of operation:
    //
    // 1. **Variable amount mode**: Operator enters amount on keypad → OK →
    //    relay polls for tags → execute transaction flow.
    //
    // 2. **Merchant POS mode** (fixed amount + fixed receiver): Operator
    //    presses OK → relay immediately polls for customer credstick →
    //    one-tap payment. This is the "personalized POS" mode — the relay
    //    is configured for a specific merchant and price point.
    //
    // In both modes, the relay is a simple, dedicated device. No PIN,
    // no authorization on the relay side. The credstick handles its own
    // security (PIN, e-ink confirmation).
    loop {
        // Check power before doing anything expensive.
        if !power.can_transact() {
            defmt::warn!("Low power, waiting for charge");
            leds.play(led::patterns::LOW_POWER).await;
            Timer::after(Duration::from_secs(5)).await;
            continue;
        }

        // In merchant POS mode, auto-start with fixed amount.
        let amount = match config.mode {
            Mode::MerchantPos | Mode::FixedAmount => {
                // Show ready indicator and wait for OK press to start.
                leds.set(Color::Green);
                defmt::info!("Ready: press OK to charge {} tokens", config.fixed_amount);

                // TODO: Wait for keypad OK press via EVENT_CHANNEL.
                // For now, use a simple delay as placeholder.
                Timer::after(Duration::from_millis(100)).await;

                config.fixed_amount
            }
            Mode::Variable => {
                // Wait for amount entry via keypad.
                leds.set(Color::Green);
                defmt::info!("Enter amount on keypad, press OK to confirm");

                // TODO: Poll TCA8418 for key events and feed to AmountEntry.
                // The keypad task would send KeyPress events via EVENT_CHANNEL.
                // For now, placeholder loop:
                loop {
                    let event = EVENT_CHANNEL.receive().await;
                    match event {
                        Event::KeyPress(key) => {
                            if let Some(cents) = amount_entry.process(key) {
                                // Convert cents to token base units.
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

        // Step 1: Acquire receiver ticket (skip if cached in merchant POS mode).
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

                    // Select Briolette applet.
                    match nfc.select_briolette_applet().await {
                        Ok(true) => {}
                        _ => {
                            defmt::warn!("Not a Briolette credstick");
                            nfc.deactivate().await.ok();
                            leds.play(led::patterns::FAILURE).await;
                            continue;
                        }
                    }

                    // READ_TICKET.
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

        // Step 2: Tap sender — INITIATE + (implicit TRANSACT in 2-tap mode).
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

                // INITIATE — sends amount + receiver ticket to sender.
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
        //
        // The sender has lifted their credstick, seen the proposed amount
        // on its e-ink display, and (if needed) entered their PIN.
        // The physical re-tap IS consent.
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

                // TRANSFER — sender signs tokens.
                let transfer_apdu = tx_engine.build_transfer();
                match nfc.transceive_apdu(&transfer_apdu).await {
                    Ok(resp) => {
                        if tx_engine.handle_transfer_response(&resp).is_err() {
                            // Could be PIN_REQUIRED — the user needs to enter
                            // PIN on their credstick and tap again.
                            if tx_engine.phase() == Phase::SenderProposed {
                                defmt::info!("PIN required, sender must enter PIN and re-tap");
                                nfc.deactivate().await.ok();
                                // Loop back to wait for re-tap.
                                // In a real implementation, we'd loop here.
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
        //
        // In merchant POS mode with a cached receiver, we still need to
        // tap the receiver credstick to deliver the tokens. This is the
        // final step.
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

                // RECEIVE — deliver signed tokens to receiver.
                let receive_apdu = tx_engine.build_receive();
                match nfc.transceive_apdu(&receive_apdu).await {
                    Ok(resp) => {
                        if tx_engine.handle_receive_response(&resp).is_ok() {
                            // Success!
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
