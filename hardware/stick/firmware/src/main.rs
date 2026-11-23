#![no_std]
#![no_main]

mod apdu;
mod bloom;
mod button;
mod display;
mod ecdaa;
mod nfc;
mod sim_card;
mod storage;

use defmt_rtt as _;
use panic_probe as _;

use embassy_executor::Spawner;
use embassy_futures::join::join3;
use embassy_nrf::gpio::{Input, Level, Output, OutputDrive, Pull};
use embassy_nrf::peripherals;
use embassy_nrf::pwm::{self, Prescaler, SimplePwm};
use embassy_nrf::spim::{self, Spim};
use embassy_nrf::{bind_interrupts, uarte};
use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::{Duration, Timer};
use static_cell::StaticCell;

use crate::apdu::TransactionState;
use crate::display::DisplayUpdate;

bind_interrupts!(struct Irqs {
    UARTE0_UART0 => uarte::InterruptHandler<peripherals::UARTE0>;
    SPIM1_SPIS1_TWIM1_TWIS1_SPI1_TWI1 => spim::InterruptHandler<peripherals::TWISPI1>;
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

    // === Phase 1: Concurrent initialization ===
    // Storage, SIM card, and display can all initialize independently.
    // Use join3 to run them concurrently.

    // Set up UARTE for SIM card ISO 7816 communication.
    // SIM_IO on P0.26 (TX/RX half-duplex), baud rate per ATR (default 9600).
    let uart_config = uarte::Config::default();
    let uart = uarte::Uarte::new(
        p.UARTE0,
        Irqs,
        p.P0_26, // SIM_IO (RX)
        p.P0_26, // SIM_IO (TX) — same pin, half-duplex
        uart_config,
    );

    // SIM_RST on P0.22 (active low, start asserted).
    let sim_rst = Output::new(p.P0_22, Level::Low, OutputDrive::Standard);
    // SIM_DET on P0.24 (card detect, active low with external pull-up).
    let sim_det = Input::new(p.P0_24, Pull::Up);
    // SIM_CLK on P0.27: ISO 7816 requires 1-5 MHz clock (40-60% duty).
    // PWM0 at 16 MHz base / countertop 5 = 3.2 MHz, within spec.
    let mut sim_clk = SimplePwm::new_1ch(p.PWM0, p.P0_27);
    sim_clk.set_prescaler(Prescaler::Div1);
    sim_clk.set_max_duty(5);
    sim_clk.set_duty(0, 2); // 2/5 = 40% duty, 3.2 MHz

    let mut sim = sim_card::SimCard::new(uart, sim_rst, sim_det);

    // Initialize SPI for e-ink display data transfer.
    // P0.13 = SCK, P0.14 = MOSI (write-only, no MISO needed).
    let mut spi_config = spim::Config::default();
    spi_config.frequency = spim::Frequency::M4;
    let spi = Spim::new_txonly(p.TWISPI1, Irqs, p.P0_13, p.P0_14, spi_config);

    // Initialize e-ink display with SPI + control pins (match schematic MCU sheet).
    let display = display::Display::new(
        spi,
        Output::new(p.P0_16, Level::Low, OutputDrive::Standard),  // DC
        Output::new(p.P0_15, Level::High, OutputDrive::Standard), // CS
        Input::new(p.P0_18, Pull::None),                          // BUSY
        Output::new(p.P0_17, Level::High, OutputDrive::Standard), // RST
    );

    // Run storage init, SIM init, and initial display update concurrently.
    let (store, sim_ok, _) = join3(
        async {
            let store = storage::Storage::init();
            defmt::info!("Storage initialized, balance: {}", store.balance());
            store
        },
        async {
            let ok = sim.init().await;
            if ok {
                defmt::info!("SIM card initialized");
            }
            ok
        },
        async {
            // Small delay to let storage init provide balance, then we'll
            // update display below once we have the actual balance.
            Timer::after(Duration::from_millis(10)).await;
        },
    )
    .await;

    if !sim_ok {
        defmt::warn!("SIM card not available — attestation disabled");
        // Continue without SIM; ECDAA signing still works, but
        // manufacturer attestation will be unavailable.
    }

    // Now that storage is initialized, show balance on e-ink.
    display.update(DisplayUpdate::Balance {
        tokens: store.balance(),
    });

    // Initialize transaction state.
    let tx_state = TX_STATE.init(TransactionState::new(store));

    // === Phase 2: Spawn concurrent tasks ===
    // Button handler and event loop run as independent async tasks.

    let btn_left = Input::new(p.P0_11, Pull::Up);
    let btn_right = Input::new(p.P0_12, Pull::Up);

    spawner
        .spawn(button::button_task(btn_left, btn_right))
        .unwrap();

    defmt::info!("Credstick ready, entering main loop");

    // === Phase 3: Main event loop ===
    // Coordinate NFC, buttons, and display.
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
                        tx_state.pin_input(button::PinSymbol::Left { long });
                        display.update(DisplayUpdate::PinProgress {
                            entered: tx_state.pin_entered_count(),
                        });
                    } else {
                        tx_state.cancel();
                        display.update(DisplayUpdate::Balance {
                            tokens: tx_state.balance(),
                        });
                    }
                }
            }

            Event::ButtonRight { long } => {
                if tx_state.phase().is_proposed() && tx_state.pin_required() {
                    tx_state.pin_input(button::PinSymbol::Right { long });
                    display.update(DisplayUpdate::PinProgress {
                        entered: tx_state.pin_entered_count(),
                    });
                }
            }

            Event::ButtonBoth => {
                if tx_state.pin_in_progress() {
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
