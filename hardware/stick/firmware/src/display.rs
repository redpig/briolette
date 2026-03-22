//! E-ink display driver and UI rendering.
//!
//! Drives a small e-ink display (e.g., GDEW0154T8, 1.54" 200x200 or similar)
//! via SPI. The e-ink retains its image with zero power — perfect for a
//! supercap-powered device.
//!
//! UI screens:
//! - Balance (home screen): shows token count and last transaction
//! - PayConfirm: "Pay N tokens?" with description
//! - PayWithPin: "Pay N tokens?" + PIN entry prompt
//! - PinProgress: PIN entry in progress (showing * dots)
//! - PinAccepted: "✓ Authorized, tap to confirm"
//! - PinRejected: "✗ Wrong PIN, N attempts left"
//! - Sent: "✓ Sent N" with remaining balance
//! - Received: "+N tokens (unvalidated)"
//! - Rejected: Transaction rejected
//! - Locked: Device locked (PIN attempts exhausted)
//! - LowPower: "Charge via USB-C"
//!
//! The display is 1-bit (black/white). Partial refresh (~300ms, 6mAs) is
//! used for most updates; full refresh (~800ms, 32mAs) only on power-up
//! or when ghosting accumulates.

use embassy_nrf::gpio::{Input, Output};
use embassy_nrf::peripherals;
use embassy_nrf::spim::Spim;

/// Display update commands sent from the main loop.
pub enum DisplayUpdate {
    /// Home screen: show balance.
    Balance { tokens: u32 },
    /// Payment proposal (no PIN required).
    PayConfirm {
        tokens: u32,
        description: heapless::String<32>,
    },
    /// Payment proposal with PIN entry prompt.
    PayWithPin {
        tokens: u32,
        description: heapless::String<32>,
    },
    /// PIN entry in progress.
    PinProgress { entered: usize },
    /// PIN accepted.
    PinAccepted { tokens: u32 },
    /// PIN rejected.
    PinRejected { attempts_left: u8 },
    /// Transaction completed (sent tokens).
    Sent { tokens: u32, sent: u32 },
    /// Tokens received.
    Received { tokens: u32 },
    /// Transaction rejected.
    Rejected,
    /// Device locked.
    Locked,
    /// Low power warning.
    LowPower,
}

/// E-ink display dimensions (adjust for actual display model).
const WIDTH: usize = 200;
const HEIGHT: usize = 200;
/// Framebuffer: 1 bit per pixel, packed into bytes (200*200/8 = 5000 bytes).
const FB_SIZE: usize = WIDTH * HEIGHT / 8;

/// SPI-connected e-ink display driver.
pub struct Display<'d> {
    /// SPI bus for data transfer (SCK + MOSI, write-only).
    spi: Spim<'d, peripherals::TWISPI1>,
    /// Data/Command pin.
    dc: Output<'d>,
    /// Chip Select pin.
    cs: Output<'d>,
    /// Busy pin (input, active low).
    busy: Input<'d>,
    /// Reset pin.
    rst: Output<'d>,
    /// Framebuffer.
    fb: [u8; FB_SIZE],
    /// Partial refresh counter (full refresh every N partials to clear ghosting).
    partial_count: u8,
}

impl<'d> Display<'d> {
    pub fn new(
        spi: Spim<'d, peripherals::TWISPI1>,
        dc: Output<'d>,
        cs: Output<'d>,
        busy: Input<'d>,
        rst: Output<'d>,
    ) -> Self {
        Self {
            spi,
            dc,
            cs,
            busy,
            rst,
            fb: [0xFF; FB_SIZE], // White background.
            partial_count: 0,
        }
    }

    /// Update the display with a new screen.
    pub fn update(&mut self, screen: DisplayUpdate) {
        // Clear framebuffer to white.
        self.fb.fill(0xFF);

        // Render the screen to the framebuffer.
        match screen {
            DisplayUpdate::Balance { tokens } => {
                self.render_balance(tokens);
            }
            DisplayUpdate::PayConfirm { tokens, description } => {
                self.render_pay_confirm(tokens, &description, false);
            }
            DisplayUpdate::PayWithPin { tokens, description } => {
                self.render_pay_confirm(tokens, &description, true);
            }
            DisplayUpdate::PinProgress { entered } => {
                self.render_pin_progress(entered);
            }
            DisplayUpdate::PinAccepted { tokens } => {
                self.render_pin_accepted(tokens);
            }
            DisplayUpdate::PinRejected { attempts_left } => {
                self.render_pin_rejected(attempts_left);
            }
            DisplayUpdate::Sent { tokens, sent } => {
                self.render_sent(tokens, sent);
            }
            DisplayUpdate::Received { tokens } => {
                self.render_received(tokens);
            }
            DisplayUpdate::Rejected => {
                self.render_text_centered("Rejected");
            }
            DisplayUpdate::Locked => {
                self.render_text_centered("LOCKED");
            }
            DisplayUpdate::LowPower => {
                self.render_low_power();
            }
        }

        // Send framebuffer to display.
        self.flush();
    }

    // --- Screen renderers ---
    // These write into self.fb using a simple bitmap font.
    // In production, use an embedded font rasterizer (e.g., embedded-graphics).

    fn render_balance(&mut self, tokens: u32) {
        // ┌────────────────┐
        // │   ◉ 42 tokens  │
        // │                 │
        // │                 │
        // │                 │
        // └────────────────┘
        self.draw_text(10, 40, &format_u32(tokens));
        self.draw_text(10, 60, "tokens");
    }

    fn render_pay_confirm(&mut self, tokens: u32, desc: &str, with_pin: bool) {
        // ┌────────────────┐
        // │  Pay 5 tokens?  │
        // │  "Coffee"       │
        // │                 │
        // │  Enter PIN:     │  (if with_pin)
        // │  _ _ _ _       │
        // └────────────────┘
        self.draw_text(10, 20, "Pay ");
        self.draw_text(50, 20, &format_u32(tokens));
        self.draw_text(10, 40, desc);

        if with_pin {
            self.draw_text(10, 80, "Enter PIN:");
            self.draw_text(10, 100, "_ _ _ _");
        } else {
            self.draw_text(10, 100, "Tap to confirm");
        }
    }

    fn render_pin_progress(&mut self, entered: usize) {
        self.draw_text(10, 80, "Enter PIN:");
        let mut pin_display = [b' '; 16];
        for i in 0..entered.min(8) {
            pin_display[i * 2] = b'*';
        }
        if entered < 8 {
            pin_display[entered * 2] = b'_';
        }
        // TODO: render pin_display
    }

    fn render_pin_accepted(&mut self, tokens: u32) {
        self.draw_text(10, 20, "Pay ");
        self.draw_text(50, 20, &format_u32(tokens));
        self.draw_text(10, 50, "Authorized");
        self.draw_text(10, 80, "Tap to confirm");
    }

    fn render_pin_rejected(&mut self, attempts_left: u8) {
        self.draw_text(10, 30, "Wrong PIN");
        self.draw_text(10, 60, &format_u32(attempts_left as u32));
        self.draw_text(10, 80, "attempts left");
    }

    fn render_sent(&mut self, remaining: u32, sent: u32) {
        self.draw_text(10, 20, &format_u32(remaining));
        self.draw_text(10, 40, "tokens");
        self.draw_text(10, 80, "Sent ");
        self.draw_text(60, 80, &format_u32(sent));
    }

    fn render_received(&mut self, tokens: u32) {
        self.draw_text(10, 30, "+");
        self.draw_text(20, 30, &format_u32(tokens));
        self.draw_text(10, 50, "tokens");
        self.draw_text(10, 80, "(unvalidated)");
    }

    fn render_low_power(&mut self) {
        self.draw_text(10, 40, "Low power");
        self.draw_text(10, 70, "Charge via");
        self.draw_text(10, 90, "USB-C");
    }

    fn render_text_centered(&mut self, text: &str) {
        // Simple centered text at vertical midpoint.
        self.draw_text(10, 80, text);
    }

    // --- Low-level rendering ---

    /// Draw text at (x, y) using a simple bitmap font.
    ///
    /// In production, use embedded-graphics with a proper font.
    /// For now, this is a placeholder that sets pixels.
    fn draw_text(&mut self, _x: usize, _y: usize, _text: &str) {
        // TODO: Implement with embedded-graphics crate.
        // Font options:
        // - profont (7x13, good readability)
        // - mono_font (various sizes)
        // - custom bitmap font for the specific display size
        //
        // Example with embedded-graphics:
        //   use embedded_graphics::{
        //       mono_font::{ascii::FONT_10X20, MonoTextStyle},
        //       pixelcolor::BinaryColor,
        //       text::Text,
        //       Drawable,
        //   };
        //   let style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
        //   Text::new(text, Point::new(x as i32, y as i32), style)
        //       .draw(&mut self.display_target)?;
    }

    /// Send a command byte over SPI (DC low = command).
    fn send_cmd(&mut self, cmd: u8) {
        self.dc.set_low();
        self.cs.set_low();
        // Use blocking write since embassy Spim requires async context.
        // In a real async context, these would be .await calls.
        let _ = embassy_futures::block_on(self.spi.write(&[cmd]));
        self.cs.set_high();
    }

    /// Send data bytes over SPI (DC high = data).
    fn send_data(&mut self, data: &[u8]) {
        self.dc.set_high();
        self.cs.set_low();
        let _ = embassy_futures::block_on(self.spi.write(data));
        self.cs.set_high();
    }

    /// Wait for the display busy pin to go high (not busy).
    fn wait_busy(&self) {
        while self.busy.is_low() {
            cortex_m::asm::nop();
        }
    }

    /// Send the framebuffer to the e-ink display.
    ///
    /// Uses SSD1681/IL0373-compatible SPI protocol.
    fn flush(&mut self) {
        self.partial_count += 1;
        let full_refresh = self.partial_count >= 10;
        if full_refresh {
            self.partial_count = 0;
        }

        if full_refresh {
            defmt::debug!("Display: full refresh");
            // SW Reset
            self.send_cmd(0x12);
            self.wait_busy();

            // Driver output control: 200 lines
            self.send_cmd(0x01);
            self.send_data(&[0xC7, 0x00, 0x00]); // 199 = 0xC7, gate scan direction

            // Data entry mode: X increment, Y increment
            self.send_cmd(0x11);
            self.send_data(&[0x03]);

            // Set RAM X address range: 0 to 24 (200/8 - 1)
            self.send_cmd(0x44);
            self.send_data(&[0x00, 0x18]);

            // Set RAM Y address range: 0 to 199
            self.send_cmd(0x45);
            self.send_data(&[0x00, 0x00, 0xC7, 0x00]);

            // Set RAM X counter
            self.send_cmd(0x4E);
            self.send_data(&[0x00]);

            // Set RAM Y counter
            self.send_cmd(0x4F);
            self.send_data(&[0x00, 0x00]);
        }

        // Write RAM data (0x24)
        self.send_cmd(0x24);
        self.send_data(&self.fb);

        // Display update control 2: full or partial refresh
        self.send_cmd(0x22);
        if full_refresh {
            self.send_data(&[0xF7]); // Full update sequence
        } else {
            self.send_data(&[0xFF]); // Partial update sequence
        }

        // Master activation
        self.send_cmd(0x20);
        self.wait_busy();
    }
}

/// Format u32 into a stack-allocated string (no alloc).
fn format_u32(n: u32) -> heapless::String<12> {
    let mut s = heapless::String::new();
    if n == 0 {
        s.push('0').ok();
        return s;
    }
    let mut buf = [0u8; 10];
    let mut i = 0;
    let mut val = n;
    while val > 0 {
        buf[i] = b'0' + (val % 10) as u8;
        val /= 10;
        i += 1;
    }
    for j in (0..i).rev() {
        s.push(buf[j] as char).ok();
    }
    s
}
