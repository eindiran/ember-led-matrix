//! Firmware for the Waveshare ESP32-S3-Matrix that lights the whole onboard
//! 8x8 WS2812B LED matrix solid red.
//!
//! The 64 matrix LEDs are chained on a single data line (GPIO14) and are
//! driven with WS2812 timing by the RMT peripheral via the `esp-hal-smartled`
//! adapter. Brightness is deliberately limited: Waveshare warns that running
//! the matrix bright heats the board rapidly and can damage it, and 64 LEDs
//! at full red would draw on the order of an amp from the 5 V rail.

#![no_std]
#![no_main]

use esp_hal::main;
use esp_hal::rmt::Rmt;
use esp_hal::time::Rate;
use esp_hal_smartled::{SmartLedsAdapter, smart_led_buffer};
use smart_leds::{RGB8, SmartLedsWrite as _, brightness, colors::RED};

esp_bootloader_esp_idf::esp_app_desc!();

/// Number of WS2812B LEDs in the 8x8 matrix chain.
const NUM_LEDS: usize = 64;

/// Global brightness scale, 0-255. Kept low per Waveshare's overheating
/// warning; 32/255 is clearly visible while staying thermally safe.
const BRIGHTNESS: u8 = 32;

/// Minimal panic handler: park the CPU. There is no console wired up in
/// this firmware, so there is nowhere useful to report the panic.
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default());

    // The RMT peripheral clocks at 80 MHz from the APB clock; the adapter
    // derives the WS2812 bit timings from that rate.
    let rmt = Rmt::new(peripherals.RMT, Rate::from_mhz(80)).expect("failed to initialize RMT");
    let mut rmt_buffer = smart_led_buffer!(NUM_LEDS);
    // The matrix LEDs take RGB byte order on the wire, not the usual WS2812
    // GRB (verified empirically: a red frame rendered green with the
    // adapter's default GRB encoding), so pin the color type to RGB8.
    let mut matrix: SmartLedsAdapter<'_, { esp_hal_smartled::buffer_size(NUM_LEDS) }, RGB8> =
        SmartLedsAdapter::new_with_color(rmt.channel0, peripherals.GPIO14, &mut rmt_buffer);

    matrix
        .write(brightness([RED; NUM_LEDS].into_iter(), BRIGHTNESS))
        .expect("failed to write LED data");

    // WS2812 LEDs latch the last frame, so a single write suffices.
    loop {
        core::hint::spin_loop();
    }
}
