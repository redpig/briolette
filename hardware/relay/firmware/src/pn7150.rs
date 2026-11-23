//! PN7150 NFC Controller I2C Driver
//!
//! The PN7150 is NXP's NFC controller with reader/writer, P2P, and card
//! emulation modes. We use it in **reader/writer mode** to communicate with
//! credstick NFC tags (ISO 14443-4 / ISO-DEP / Type 4 Tag).
//!
//! Communication flow:
//! 1. MCU sends NCI commands over I2C to configure the PN7150
//! 2. PN7150 generates RF field and polls for NFC-A tags
//! 3. When a tag is activated, MCU exchanges ISO-DEP APDUs via NCI data messages
//! 4. PN7150 handles RF framing, anti-collision, and ISO-DEP protocol
//!
//! I2C address: 0x28 (7-bit)
//! INT pin: active-low, asserts when PN7150 has data ready
//! VEN pin: active-high enable (low = power down)
//!
//! NCI (NFC Controller Interface) is the standard protocol for host↔NFCC
//! communication (NFC Forum NCI 1.0 / 2.0).
//!
//! Power notes:
//! - Active RF field: 20-50mA (dominates transaction power budget)
//! - Standby: ~10µA
//! - Shutdown (VEN low): <1µA
//! - Transaction sizes scale with token history length — longer disconnected
//!   periods mean bigger APDUs and more RF time per transaction.

use embassy_nrf::gpio::{Input, Output};
use embassy_time::{Duration, Timer};
use embedded_hal_async::i2c::I2c;
use heapless::Vec;

const PN7150_I2C_ADDR: u8 = 0x28;

/// Maximum NCI packet size (header + payload).
const NCI_MAX_PACKET: usize = 258;

/// Briolette application AID for ISO 7816-4 SELECT.
const BRIOLETTE_AID: [u8; 7] = [0xA0, 0x00, 0x00, 0x00, 0x62, 0x03, 0x01];

/// NCI Message Type (MT) field values (bits 7:5 of header byte 0).
mod nci_mt {
    pub const DATA: u8 = 0x00;
    pub const COMMAND: u8 = 0x20;
    pub const RESPONSE: u8 = 0x40;
    pub const NOTIFICATION: u8 = 0x60;
}

/// NCI Group ID (GID) values (bits 3:0 of header byte 0).
mod nci_gid {
    pub const CORE: u8 = 0x00;
    pub const RF: u8 = 0x01;
    pub const NFCEE: u8 = 0x02;
    pub const PROP: u8 = 0x0F;
}

/// NCI Opcode ID (OID) values (bits 5:0 of header byte 1).
mod nci_oid {
    // Core group
    pub const CORE_RESET: u8 = 0x00;
    pub const CORE_INIT: u8 = 0x01;
    pub const CORE_SET_CONFIG: u8 = 0x02;
    pub const CORE_CONN_CREATE: u8 = 0x04;

    // RF group
    pub const RF_DISCOVER_MAP: u8 = 0x00;
    pub const RF_DISCOVER: u8 = 0x03;
    pub const RF_DEACTIVATE: u8 = 0x06;
    pub const RF_INTF_ACTIVATED: u8 = 0x05; // Notification
}

/// NCI status codes.
mod nci_status {
    pub const OK: u8 = 0x00;
    pub const REJECTED: u8 = 0x01;
}

/// RF technology types.
mod rf_tech {
    pub const NFC_A_PASSIVE_POLL: u8 = 0x00;
}

/// RF interface types.
mod rf_intf {
    pub const ISO_DEP: u8 = 0x02;
}

/// RF discovery action.
mod rf_discover_action {
    pub const START: u8 = 0x00;
    pub const STOP: u8 = 0x01;
}

/// Deactivation types.
mod deactivate_type {
    pub const IDLE: u8 = 0x00;
    pub const DISCOVERY: u8 = 0x03;
}

/// PN7150 driver state.
#[derive(Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum State {
    /// Not initialized.
    Uninitialized,
    /// Initialized, not polling.
    Ready,
    /// Actively polling for NFC-A tags.
    Polling,
    /// Tag activated, ISO-DEP session open.
    Connected,
    /// Error state.
    Error,
}

#[derive(Debug, defmt::Format)]
pub enum Error {
    /// I2C communication failed.
    I2cError,
    /// NCI protocol error (unexpected response, wrong status).
    NciError,
    /// Timeout waiting for PN7150 response.
    Timeout,
    /// Tag not present or lost during communication.
    TagLost,
    /// Response too large for buffer.
    BufferOverflow,
    /// PN7150 not in correct state for this operation.
    InvalidState,
}

/// PN7150 NFC reader driver.
///
/// Manages the PN7150 over I2C with interrupt-driven reads.
/// The INT pin signals when the PN7150 has data ready to read.
/// Generic over the I2C bus type to support shared bus patterns.
pub struct Pn7150<'a, I2C> {
    state: State,
    i2c: I2C,
    /// Interrupt pin from PN7150 (active-low, open-drain).
    irq: Input<'a>,
    /// VEN (enable) pin — high to enable PN7150, low to power down.
    ven: Output<'a>,
    /// Receive buffer for NCI packets.
    rx_buf: [u8; NCI_MAX_PACKET],
    /// Connection ID for the active ISO-DEP session.
    conn_id: u8,
}

impl<'a, I2C, E> Pn7150<'a, I2C>
where
    I2C: I2c<Error = E>,
{
    pub fn new(i2c: I2C, irq: Input<'a>, ven: Output<'a>) -> Self {
        Self {
            state: State::Uninitialized,
            i2c,
            irq,
            ven,
            rx_buf: [0u8; NCI_MAX_PACKET],
            conn_id: 0,
        }
    }

    pub fn state(&self) -> State {
        self.state
    }

    /// Initialize the PN7150: hardware reset, NCI CORE_RESET, CORE_INIT,
    /// configure for NFC-A ISO-DEP reader mode.
    pub async fn init(&mut self) -> Result<(), Error> {
        // Hardware reset: toggle VEN.
        self.ven.set_low();
        Timer::after(Duration::from_millis(10)).await;
        self.ven.set_high();
        Timer::after(Duration::from_millis(10)).await;

        // Wait for PN7150 boot (INT asserts when ready).
        self.wait_irq(Duration::from_millis(100)).await?;
        // Read and discard boot notification.
        self.nci_read().await?;

        // NCI CORE_RESET (reset to NCI mode).
        self.nci_write(&[
            nci_mt::COMMAND | nci_gid::CORE,
            nci_oid::CORE_RESET,
            0x01, // payload length
            0x01, // Reset Configuration: keep config
        ])
        .await?;
        let resp = self.nci_read_response().await?;
        self.check_nci_status(&resp)?;

        // NCI CORE_INIT.
        self.nci_write(&[
            nci_mt::COMMAND | nci_gid::CORE,
            nci_oid::CORE_INIT,
            0x00, // no payload
        ])
        .await?;
        let resp = self.nci_read_response().await?;
        self.check_nci_status(&resp)?;

        // RF_DISCOVER_MAP: map NFC-A poll to ISO-DEP interface.
        self.nci_write(&[
            nci_mt::COMMAND | nci_gid::RF,
            nci_oid::RF_DISCOVER_MAP,
            0x04,                       // payload length
            0x01,                       // number of mapping entries
            rf_tech::NFC_A_PASSIVE_POLL, // technology
            0x01,                       // mode: poll
            rf_intf::ISO_DEP,           // interface: ISO-DEP
        ])
        .await?;
        let resp = self.nci_read_response().await?;
        self.check_nci_status(&resp)?;

        self.state = State::Ready;
        defmt::info!("PN7150 initialized, NFC-A ISO-DEP reader mode configured");
        Ok(())
    }

    /// Start polling for NFC-A tags.
    pub async fn start_polling(&mut self) -> Result<(), Error> {
        if self.state != State::Ready {
            return Err(Error::InvalidState);
        }

        // RF_DISCOVER: start polling for NFC-A.
        self.nci_write(&[
            nci_mt::COMMAND | nci_gid::RF,
            nci_oid::RF_DISCOVER,
            0x03,                        // payload length
            0x01,                        // number of configurations
            rf_tech::NFC_A_PASSIVE_POLL, // NFC-A poll mode
            0x01,                        // frequency: every cycle
        ])
        .await?;
        let resp = self.nci_read_response().await?;
        self.check_nci_status(&resp)?;

        self.state = State::Polling;
        defmt::debug!("PN7150 polling for NFC-A tags");
        Ok(())
    }

    /// Wait for a tag to be activated (ISO-DEP session established).
    ///
    /// Blocks until a tag is detected and ISO-DEP activation completes,
    /// or the timeout expires.
    pub async fn wait_for_tag(&mut self, timeout: Duration) -> Result<(), Error> {
        if self.state != State::Polling {
            return Err(Error::InvalidState);
        }

        // Wait for RF_INTF_ACTIVATED notification.
        let deadline = embassy_time::Instant::now() + timeout;
        loop {
            let remaining = deadline - embassy_time::Instant::now();
            if remaining.as_millis() == 0 {
                return Err(Error::Timeout);
            }

            self.wait_irq(remaining).await?;
            let len = self.nci_read().await?;

            // Check if this is an RF_INTF_ACTIVATED notification.
            if len >= 3
                && (self.rx_buf[0] & 0xE0) == nci_mt::NOTIFICATION
                && (self.rx_buf[0] & 0x0F) == nci_gid::RF
                && self.rx_buf[1] == nci_oid::RF_INTF_ACTIVATED
            {
                // Extract connection ID from the notification.
                if len > 3 {
                    self.conn_id = self.rx_buf[3]; // Discovery ID
                }
                self.state = State::Connected;
                defmt::info!("NFC tag activated (ISO-DEP)");
                return Ok(());
            }
            // Else: some other notification (discovery, etc.) — keep waiting.
        }
    }

    /// Select the Briolette applet on the connected tag via ISO 7816-4 SELECT.
    ///
    /// Must be called after `wait_for_tag()` succeeds. Returns true if the
    /// Briolette AID was found and selected.
    pub async fn select_briolette_applet(&mut self) -> Result<bool, Error> {
        if self.state != State::Connected {
            return Err(Error::InvalidState);
        }

        // Build SELECT APDU: CLA=00 INS=A4 P1=04 P2=00 Lc=07 AID
        let mut select_apdu: Vec<u8, 32> = Vec::new();
        select_apdu.extend_from_slice(&[0x00, 0xA4, 0x04, 0x00]).ok();
        select_apdu.push(BRIOLETTE_AID.len() as u8).ok();
        select_apdu.extend_from_slice(&BRIOLETTE_AID).ok();

        let response = self.transceive_apdu(&select_apdu).await?;

        // Check SW1SW2 = 9000.
        if response.len() >= 2 {
            let sw1 = response[response.len() - 2];
            let sw2 = response[response.len() - 1];
            if sw1 == 0x90 && sw2 == 0x00 {
                defmt::info!("Briolette applet selected");
                return Ok(true);
            }
            defmt::warn!("SELECT failed: SW={:02X}{:02X}", sw1, sw2);
        }

        Ok(false)
    }

    /// Send a Briolette APDU to the connected tag and receive the response.
    ///
    /// The APDU is wrapped in an NCI data message for transmission.
    /// Token history length affects APDU size — longer histories from
    /// extended disconnected operation produce larger APDUs and require
    /// more RF time (and thus more power).
    pub async fn transceive_apdu(
        &mut self,
        apdu: &[u8],
    ) -> Result<Vec<u8, 2048>, Error> {
        if self.state != State::Connected {
            return Err(Error::InvalidState);
        }

        // Wrap APDU in NCI data message.
        // NCI Data: [conn_id, 0x00, length, ...payload]
        let mut nci_data: Vec<u8, 264> = Vec::new();
        nci_data.push(self.conn_id).ok();
        nci_data.push(0x00).ok(); // RFU
        nci_data.push(apdu.len() as u8).ok();
        nci_data.extend_from_slice(apdu).ok();

        self.nci_write(&nci_data).await?;

        // Wait for response data message.
        self.wait_irq(Duration::from_millis(5000)).await?;
        let len = self.nci_read().await?;

        // Parse NCI data response.
        if len < 3 {
            return Err(Error::NciError);
        }

        // Check it's a data message (MT=0x00).
        if (self.rx_buf[0] & 0xE0) != nci_mt::DATA {
            // Could be a notification (e.g., tag lost).
            if (self.rx_buf[0] & 0xE0) == nci_mt::NOTIFICATION {
                self.state = State::Ready;
                return Err(Error::TagLost);
            }
            return Err(Error::NciError);
        }

        let payload_len = self.rx_buf[2] as usize;
        if len < 3 + payload_len {
            return Err(Error::NciError);
        }

        let mut response: Vec<u8, 2048> = Vec::new();
        response
            .extend_from_slice(&self.rx_buf[3..3 + payload_len])
            .map_err(|_| Error::BufferOverflow)?;

        Ok(response)
    }

    /// Deactivate the current tag (end the ISO-DEP session).
    ///
    /// After deactivation, the PN7150 returns to Ready state.
    /// Call `start_polling()` to poll for the next tag.
    pub async fn deactivate(&mut self) -> Result<(), Error> {
        // RF_DEACTIVATE: return to discovery or idle.
        let deact_type = if self.state == State::Connected {
            deactivate_type::IDLE
        } else {
            deactivate_type::IDLE
        };

        self.nci_write(&[
            nci_mt::COMMAND | nci_gid::RF,
            nci_oid::RF_DEACTIVATE,
            0x01,      // payload length
            deact_type, // deactivation type
        ])
        .await?;

        // Wait for response.
        let resp = self.nci_read_response().await?;
        self.check_nci_status(&resp)?;

        // Also consume the deactivation notification.
        self.wait_irq(Duration::from_millis(100)).await.ok();
        self.nci_read().await.ok();

        self.state = State::Ready;
        defmt::debug!("Tag deactivated");
        Ok(())
    }

    /// Power down the PN7150 (VEN low) for minimum power consumption.
    pub fn power_down(&mut self) {
        self.ven.set_low();
        self.state = State::Uninitialized;
        defmt::debug!("PN7150 powered down");
    }

    // --- Low-level NCI transport ---

    /// Write an NCI packet over I2C.
    async fn nci_write(&mut self, data: &[u8]) -> Result<(), Error> {
        self.i2c
            .write(PN7150_I2C_ADDR, data)
            .await
            .map_err(|_| Error::I2cError)
    }

    /// Read an NCI packet from the PN7150 over I2C.
    /// Returns the number of bytes read.
    async fn nci_read(&mut self) -> Result<usize, Error> {
        // First read the 3-byte NCI header to get payload length.
        self.i2c
            .read(PN7150_I2C_ADDR, &mut self.rx_buf[..3])
            .await
            .map_err(|_| Error::I2cError)?;

        let payload_len = self.rx_buf[2] as usize;
        let total_len = 3 + payload_len;

        if total_len > NCI_MAX_PACKET {
            return Err(Error::BufferOverflow);
        }

        // Read the full packet if there's a payload.
        if payload_len > 0 {
            self.i2c
                .read(PN7150_I2C_ADDR, &mut self.rx_buf[3..total_len])
                .await
                .map_err(|_| Error::I2cError)?;
        }

        Ok(total_len)
    }

    /// Read an NCI response (waits for IRQ, reads, returns payload).
    async fn nci_read_response(&mut self) -> Result<Vec<u8, 64>, Error> {
        self.wait_irq(Duration::from_millis(1000)).await?;
        let len = self.nci_read().await?;

        let mut resp: Vec<u8, 64> = Vec::new();
        resp.extend_from_slice(&self.rx_buf[..len])
            .map_err(|_| Error::BufferOverflow)?;
        Ok(resp)
    }

    /// Wait for the PN7150 IRQ pin to assert (go low).
    async fn wait_irq(&mut self, timeout: Duration) -> Result<(), Error> {
        if self.irq.is_low() {
            return Ok(());
        }

        let result = embassy_time::with_timeout(timeout, self.irq.wait_for_low()).await;
        match result {
            Ok(()) => Ok(()),
            Err(_) => Err(Error::Timeout),
        }
    }

    /// Check NCI response status byte (byte 3 of response).
    fn check_nci_status(&self, resp: &[u8]) -> Result<(), Error> {
        if resp.len() > 3 && resp[3] == nci_status::OK {
            Ok(())
        } else {
            Err(Error::NciError)
        }
    }
}
