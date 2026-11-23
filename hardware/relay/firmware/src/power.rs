//! BQ25504 power management for the solar relay.
//!
//! The BQ25504 is a nano-power boost charger with MPPT that harvests energy
//! from the solar cell and charges the supercap bank (2× 10F, 20F total).
//!
//! Key signals:
//!   VBAT_OK (output): High when supercap voltage is within operating range.
//!                     Configured by external resistor divider (VBAT_OK_PROG).
//!                     Low = supercap depleted, MCU should avoid NFC operations.
//!
//! Power budget per transaction (4 taps):
//!   PN7150 RF field: 20-50mA × 3-5s = 60-250 mAs per tap
//!   nRF52840 active: 5mA × 5s = 25 mAs per tap
//!   Total per transaction: ~360-1120 mAs (~1-3.4J at 3V)
//!
//! Token history length affects power consumption:
//!   - Fresh tokens (short history): ~200B APDU, fast RF exchange
//!   - Long-disconnected tokens: may carry 1-2KB history, longer RF time
//!   - Worst case (months disconnected): several KB per token = 2-3× more
//!     RF time per transaction
//!
//! Supercap budget (20F at 3V):
//!   Usable energy: ~50J → 15-50 transactions per charge
//!   USB-C recharge: ~60 seconds

use embassy_nrf::gpio::Input;

/// Charge level thresholds (estimated from supercap voltage).
///
/// The BQ25504 VBAT_OK threshold is set by hardware resistors, typically:
///   - VBAT_OK high (OK to operate): ~2.4V
///   - VBAT_OK low (undervoltage): ~2.2V
///
/// We map VBAT_OK to a simple ok/not-ok signal. For finer granularity,
/// the nRF52840's SAADC could measure VBAT directly.
#[derive(Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum ChargeLevel {
    /// Supercap voltage above threshold — safe for NFC operations.
    Ok,
    /// Supercap voltage below threshold — avoid NFC, conserve power.
    Low,
}

/// Power manager using BQ25504's VBAT_OK signal.
pub struct Power<'a> {
    /// VBAT_OK pin from BQ25504 (high = voltage OK).
    vbat_ok: Input<'a>,
    /// Number of transactions performed since last charge check.
    tx_since_check: u32,
}

impl<'a> Power<'a> {
    pub fn new(vbat_ok: Input<'a>) -> Self {
        Self {
            vbat_ok,
            tx_since_check: 0,
        }
    }

    /// Check if the supercap has enough charge for NFC operations.
    pub fn charge_level(&self) -> ChargeLevel {
        if self.vbat_ok.is_high() {
            ChargeLevel::Ok
        } else {
            ChargeLevel::Low
        }
    }

    /// Check if it's safe to start an NFC transaction.
    /// Returns false if the supercap is too depleted.
    pub fn can_transact(&self) -> bool {
        self.charge_level() == ChargeLevel::Ok
    }

    /// Record that a transaction was performed (for power tracking).
    pub fn record_transaction(&mut self) {
        self.tx_since_check += 1;
    }

    /// Get the number of transactions since last power check.
    pub fn transactions_since_check(&self) -> u32 {
        self.tx_since_check
    }

    /// Reset the transaction counter (e.g., after a charge level check).
    pub fn reset_counter(&mut self) {
        self.tx_since_check = 0;
    }
}
