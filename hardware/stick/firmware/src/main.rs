#![no_std]
#![no_main]

mod apdu;
mod atecc608b;
mod bloom;
mod button;
mod display;
mod ecdaa;
mod nfc;
mod storage;

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

use crate::apdu::TransactionState;
use crate::display::DisplayUpdate;

bind_interrupts!(struct Irqs {
    SPIM0_SPIS0_TWIM0_TWIS0_SPI0_TWI0 => twim::InterruptHandler<peripherals::TWISPI0>;
});

/// Messages from NFC/button tasks to the main coordinator.
pub enum Event {
    /// NFC field detected, APDU received.
    ApduReceived,
    /// NFC field lost.
    FieldLost,
    /// Left button pressed (short or long).
    ButtonLeft { long: bool },
    /// Right button pressed (short or long).
    ButtonRight { long: bool },
    /// Both buttons held simultaneously.
    ButtonBoth,
}

/// Shared event channel for inter-task communication.
static EVENT_CHANNEL: Channel<ThreadModeRawMutex, Event, 4> = Channel::new();

/// Shared transaction state, protected by a mutex.
/// The APDU handler updates this; the main loop reads it for display.
static TX_STATE: StaticCell<TransactionState> = StaticCell::new();

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_nrf::init(Default::default());
    defmt::info!("Briolette credstick firmware starting");

    // Initialize persistent storage (loads keys, bloom filter, balance).
    let store = storage::Storage::init();
    defmt::info!("Storage initialized, balance: {}", store.balance());

    // Initialize I2C for ATECC608B secure element.
    let twi_config = twim::Config::default();
    let twi = twim::Twim::new(p.TWISPI0, Irqs, p.P0_26, p.P0_27, twi_config);
    let _atecc = atecc608b::Atecc608b::new(twi);
    defmt::info!("ATECC608B initialized");

    // Initialize e-ink display (SPI).
    // Pins: DC=P0_13, CS=P0_14, BUSY=P0_15, RST=P0_16
    // SPI: SCK=P0_17, MOSI=P0_18
    let display = display::Display::new(
        Output::new(p.P0_13, Level::Low, OutputDrive::Standard),  // DC
        Output::new(p.P0_14, Level::High, OutputDrive::Standard), // CS
        Input::new(p.P0_15, Pull::None),                          // BUSY
        Output::new(p.P0_16, Level::High, OutputDrive::Standard), // RST
    );

    // Show initial balance on e-ink.
    display.update(DisplayUpdate::Balance {
        tokens: store.balance(),
    });

    // Initialize transaction state.
    let tx_state = TX_STATE.init(TransactionState::new(store));

    // Initialize buttons (L/R for PIN entry and navigation).
    // Left = P0_11, Right = P0_12
    let btn_left = Input::new(p.P0_11, Pull::Up);
    let btn_right = Input::new(p.P0_12, Pull::Up);

    // Spawn the button handler task.
    spawner
        .spawn(button::button_task(btn_left, btn_right))
        .unwrap();

    defmt::info!("Credstick ready, entering main loop");

    // Main event loop: coordinate NFC, buttons, and display.
    loop {
        let event = EVENT_CHANNEL.receive().await;

        match event {
            Event::ApduReceived => {
                // The NFC interrupt handler has already dispatched to
                // apdu::handle_apdu(). We just need to update the display
                // based on the resulting state.
                match tx_state.phase() {
                    apdu::Phase::Proposed { amount, desc } => {
                        if tx_state.pin_required() {
                            display.update(DisplayUpdate::PayWithPin {
                                tokens: amount,
                                description: desc,
                            });
                        } else {
                            display.update(DisplayUpdate::PayConfirm {
                                tokens: amount,
                                description: desc,
                            });
                        }
                    }
                    apdu::Phase::Signed { remaining } => {
                        display.update(DisplayUpdate::Sent {
                            tokens: remaining,
                            sent: tx_state.last_amount(),
                        });
                    }
                    apdu::Phase::Rejected => {
                        display.update(DisplayUpdate::Rejected);
                    }
                    _ => {}
                }
            }

            Event::FieldLost => {
                defmt::debug!("NFC field lost");
            }

            Event::ButtonLeft { long } => {
                if tx_state.phase().is_proposed() {
                    if tx_state.pin_required() && tx_state.pin_in_progress() {
                        // PIN entry: add Left symbol (short or long).
                        tx_state.pin_input(button::PinSymbol::Left { long });
                        display.update(DisplayUpdate::PinProgress {
                            entered: tx_state.pin_entered_count(),
                        });
                    } else {
                        // Cancel the proposal.
                        tx_state.cancel();
                        display.update(DisplayUpdate::Balance {
                            tokens: tx_state.balance(),
                        });
                    }
                }
            }

            Event::ButtonRight { long } => {
                if tx_state.phase().is_proposed() && tx_state.pin_required() {
                    // PIN entry: add Right symbol.
                    tx_state.pin_input(button::PinSymbol::Right { long });
                    display.update(DisplayUpdate::PinProgress {
                        entered: tx_state.pin_entered_count(),
                    });
                }
            }

            Event::ButtonBoth => {
                if tx_state.pin_in_progress() {
                    // Submit PIN.
                    if tx_state.verify_pin() {
                        display.update(DisplayUpdate::PinAccepted {
                            tokens: tx_state.last_amount(),
                        });
                    } else {
                        let remaining = tx_state.pin_attempts_remaining();
                        display.update(DisplayUpdate::PinRejected {
                            attempts_left: remaining,
                        });
                        if remaining == 0 {
                            display.update(DisplayUpdate::Locked);
                        }
                    }
                }
            }
        }
    }
}

/// Enter System OFF for lowest power consumption.
/// Wakes on NFC field detect or button press.
fn enter_system_off() -> ! {
    defmt::info!("Entering System OFF");
    // Configure NFC pins for field detect wakeup.
    // The nRF52840 NFC peripheral can wake from System OFF on field detect.
    cortex_m::asm::dsb();
    cortex_m::asm::wfi();
    // Should not reach here after System OFF; reset path re-enters main.
    loop {
        cortex_m::asm::wfi();
    }
}
