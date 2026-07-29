//! Firmware for the Waveshare ESP32-S3-Matrix that renders a glowing-ember
//! effect on the onboard 8x8 WS2812B LED matrix.
//!
//! Two layers of random flicker are composed. Each LED carries an
//! independent "heat" value that random-walks between a dim floor and a
//! bright ceiling, with occasional sparks toward full heat. On top of
//! that, a single slower "glow" walk scales every LED at once, so the
//! whole bed of coals swells bright and cools down together. Heat maps
//! to a deep-red palette (quadratic red, faint cubic green, no blue).
//! The 64 LEDs are chained on a single data line (GPIO14) and driven with
//! WS2812 timing by the RMT peripheral via `esp-hal-smartled`.
//!
//! Output levels are deliberately capped (peak channel value 63/255):
//! Waveshare warns that running the matrix bright heats the board rapidly
//! and can damage it.

#![no_std]
#![no_main]

use esp_hal::delay::Delay;
use esp_hal::main;
use esp_hal::rmt::Rmt;
use esp_hal::time::Rate;
use esp_hal_smartled::{SmartLedsAdapter, smart_led_buffer};
use smart_leds::{RGB8, SmartLedsWrite as _};

esp_bootloader_esp_idf::esp_app_desc!();

/// Number of WS2812B LEDs in the 8x8 matrix chain.
const NUM_LEDS: usize = 64;

/// Frame period in milliseconds (about 30 frames per second).
const FRAME_MS: u32 = 33;

/// Per-LED heat walk: fast steps between a barely-glowing floor and a
/// bright ceiling, sparking toward full heat now and then.
const EMBER: FlickerParams = FlickerParams {
    min: 48,
    calm_max: 180,
    spark_min: 220,
    step_min: 2,
    step_max: 6,
};

/// Whole-matrix glow walk: slow steps over a narrower range, so the bed
/// breathes over a couple of seconds rather than flickering. Values act
/// as a 0-255 scale factor on every LED's heat.
const GLOW: FlickerParams = FlickerParams {
    min: 120,
    calm_max: 220,
    spark_min: 240,
    step_min: 1,
    step_max: 2,
};

/// Minimal panic handler: park the CPU. There is no console wired up in
/// this firmware, so there is nowhere useful to report the panic.
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

/// Xorshift32 pseudo-random number generator (Marsaglia). Deterministic
/// but plenty for decorrelating LED flicker; state must be non-zero.
struct XorShift32(u32);

impl XorShift32 {
    /// Advance the state and return the next raw 32-bit value.
    fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// Uniform-ish value in `lo..=hi` (modulo bias is irrelevant here).
    fn range(&mut self, lo: u8, hi: u8) -> u8 {
        lo + (self.next() % (u32::from(hi - lo) + 1)) as u8
    }
}

/// Bounds and speed for a [Flicker] random walk.
struct FlickerParams {
    /// Lowest value the walk settles at.
    min: u8,
    /// Ceiling for ordinary (non-spark) targets.
    calm_max: u8,
    /// Sparks jump the target into `spark_min..=255`.
    spark_min: u8,
    /// Slowest per-frame drift rate.
    step_min: u8,
    /// Fastest per-frame drift rate.
    step_max: u8,
}

/// A value that random-walks within [FlickerParams] bounds: it drifts
/// toward a target at a random speed and picks a fresh target (sometimes
/// a spark) on arrival. Used both per LED (heat) and globally (glow).
struct Flicker {
    value: u8,
    target: u8,
    step: u8,
}

impl Flicker {
    /// Create a walk with randomized initial state so multiple instances
    /// do not move in lockstep.
    fn new(rng: &mut XorShift32, p: &FlickerParams) -> Self {
        Self {
            value: rng.range(p.min, p.calm_max),
            target: rng.range(p.min, p.calm_max),
            step: rng.range(p.step_min, p.step_max),
        }
    }

    /// Advance one frame: drift toward the target, and pick a fresh
    /// target (occasionally a spark) once it is reached.
    fn tick(&mut self, rng: &mut XorShift32, p: &FlickerParams) {
        if self.value == self.target {
            // 1-in-8 chance the new target is a spark near the top.
            self.target = if rng.next().is_multiple_of(8) {
                rng.range(p.spark_min, 255)
            } else {
                rng.range(p.min, p.calm_max)
            };
            self.step = rng.range(p.step_min, p.step_max);
        }
        if self.value < self.target {
            self.value = self.value.saturating_add(self.step).min(self.target);
        } else {
            self.value = self.value.saturating_sub(self.step).max(self.target);
        }
    }
}

/// Map an LED's heat, scaled by the global glow factor, to an ember color.
/// Perceived brightness is roughly logarithmic, so red follows a quadratic
/// (gamma-like) curve: full glow spans red 2..63, putting cool coals at a
/// barely-visible glow while sparks stay bright (63/255 cap for thermal
/// safety). Green rises cubically to at most 3/255 so hot sparks get only
/// a faint warm tint; blue stays off. Because glow scales heat *before*
/// the curve, a global dip dims the whole bed super-linearly, which reads
/// as the entire ember cooling at once.
fn ember_color(heat: u8, glow: u8) -> RGB8 {
    let h = (u32::from(heat) * u32::from(glow)) >> 8;
    RGB8::new(((h * h) >> 10) as u8, ((h * h * h) >> 22) as u8, 0)
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

    let delay = Delay::new();
    // Fixed non-zero seed: the pattern repeats across boots, which is fine
    // for a decorative effect and avoids depending on the RNG peripheral.
    let mut rng = XorShift32(0x2A3C_4D5E);
    let mut embers: [Flicker; NUM_LEDS] = core::array::from_fn(|_| Flicker::new(&mut rng, &EMBER));
    let mut glow = Flicker::new(&mut rng, &GLOW);

    loop {
        glow.tick(&mut rng, &GLOW);
        for ember in &mut embers {
            ember.tick(&mut rng, &EMBER);
        }
        matrix
            .write(embers.iter().map(|e| ember_color(e.value, glow.value)))
            .expect("failed to write LED data");
        delay.delay_millis(FRAME_MS);
    }
}
