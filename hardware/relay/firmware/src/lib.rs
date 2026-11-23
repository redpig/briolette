#![no_std]

// Re-export driver modules for firmware binary use.
pub use briolette_relay_drivers::keypad;
pub use briolette_relay_drivers::protocol;
pub use briolette_relay_drivers::relay_config;
