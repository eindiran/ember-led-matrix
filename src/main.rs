//! Firmware for the Waveshare RP2350-Matrix that renders a glowing-ember
//! effect on the onboard 8x8 WS2812B LED matrix.
//!
//! Two layers of random flicker are composed. Each LED carries an
//! independent "heat" value that random-walks between a dim floor and a
//! bright ceiling, with occasional sparks toward full heat. On top of
//! that, a single slower "glow" walk scales every LED at once, so the
//! whole bed of coals swells bright and cools down together. Heat maps
//! to a deep-red palette (quadratic red, faint cubic green, no blue).
//!
//! The 64 LEDs are chained on a single data line (GP25). The RP2350 has
//! no dedicated LED peripheral, so a small PIO program generates the
//! WS2812 bit timing (the same 10-cycles-per-bit program Waveshare's own
//! demos use); the CPU just feeds one 24-bit color word per LED into the
//! PIO TX FIFO each frame.
//!
//! Output levels are deliberately capped (peak channel value 63/255 at
//! the default brightness scale): at full brightness the matrix draws
//! around 900 mA and Waveshare recommends thermal limiting. Overall
//! brightness is selectable at compile time via the DIM_SCALE_FACTOR
//! environment variable (1-10, default 7; see `DIM_SCALE_FACTOR`).

#![no_std]
#![no_main]

use embedded_hal::delay::DelayNs as _;
use rp235x_hal as hal;

use hal::Sio;
use hal::clocks::{Clock as _, init_clocks_and_plls};
use hal::gpio::{FunctionPio0, Pin};
use hal::pio::{Buffers, PIOBuilder, PIOExt as _, PinDir, ShiftDirection};
use hal::watchdog::Watchdog;

/// Tell the RP2350 Boot ROM about our application.
#[unsafe(link_section = ".start_block")]
#[used]
pub static IMAGE_DEF: hal::block::ImageDef = hal::block::ImageDef::secure_exe();

/// External crystal frequency on the RP2350-Matrix (standard 12 MHz).
const XTAL_FREQ_HZ: u32 = 12_000_000;

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

/// Xorshift32 prng: deterministic but plenty for decorrelating LED flicker.
/// State must be non-zero.
struct XorShift32(u32);

impl XorShift32 {
    /// Advance the state and return the next raw u32.
    fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// Uniform-ish value in `lo..=hi`
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

/// Overall brightness scale, 1 (dimmest) to 10 (brightest), set at
/// compile time via the DIM_SCALE_FACTOR environment variable, e.g.
/// `DIM_SCALE_FACTOR=3 cargo build --release`. Unset defaults to 7,
/// which reproduces the baseline output exactly. Invalid values fail
/// the build.
const DIM_SCALE_FACTOR: usize = parse_dim_scale(option_env!("DIM_SCALE_FACTOR"));

/// Fixed-point (x256) luminance multipliers for scales 1..=10:
/// round(1.55^(s - 7) * 256). Steps are geometric because perceived
/// brightness tracks luminance ratios, not increments (Weber-Fechner),
/// so each step feels like a similar change. The ratio is chosen so
/// scale 10 peaks at red 63 x 1.55^3 = 234, just under saturation.
const DIM_MULT_256: [u32; 10] = [18, 29, 44, 69, 107, 165, 256, 397, 615, 953];

/// Parse DIM_SCALE_FACTOR at compile time; a bad value aborts the
/// build with this panic message.
const fn parse_dim_scale(v: Option<&str>) -> usize {
    match v {
        None => 7,
        Some(s) => match s.as_bytes() {
            [d @ b'1'..=b'9'] => (*d - b'0') as usize,
            [b'1', b'0'] => 10,
            _ => panic!("DIM_SCALE_FACTOR must be an integer in 1..=10"),
        },
    }
}

/// Map an LED's heat, scaled by the global glow factor, to a PIO FIFO
/// word. Perceived brightness is roughly logarithmic, so red follows a
/// quadratic (gamma-like) curve: at the default scale, full glow spans
/// red 2..63, putting cool coals at a barely-visible glow while sparks
/// stay bright (63/255 cap for thermal safety). Green rises cubically
/// to at most 3/255 so hot sparks get only a faint warm tint; blue
/// stays off. Because glow scales heat *before* the curve, a global
/// dip dims the whole bed super-linearly, which reads as the entire
/// ember cooling at once. Both channels are then scaled by the
/// DIM_SCALE_FACTOR luminance multiplier, preserving the red:green
/// ratio (hue) at every brightness.
///
/// The matrix LEDs take red as the first byte on the wire (verified from
/// Waveshare's own demo, which packs R<<24 | G<<16 | B<<8); the PIO
/// program shifts the word out MSB-first with a 24-bit autopull.
fn ember_color(heat: u8, glow: u8) -> u32 {
    let h = (u32::from(heat) * u32::from(glow)) >> 8;
    let m = DIM_MULT_256[DIM_SCALE_FACTOR - 1];
    // Clamps are belt-and-braces: the multiplier table is sized so
    // even scale 10 stays below 255.
    let red = ((((h * h) >> 10) * m) >> 8).min(255);
    let green = ((((h * h * h) >> 22) * m) >> 8).min(255);
    (red << 24) | (green << 16)
}

/// Assemble the classic WS2812 PIO program (pico-examples / ws2812-pio):
/// each bit takes 10 PIO cycles split into a 2-cycle high start, a
/// 5-cycle data segment, and a 3-cycle low tail, yielding the 800 kHz
/// one-wire timing. Returns the program plus its cycles-per-bit count
/// for clock divider math.
fn ws2812_program() -> (pio::Program<32>, u32) {
    const T1: u8 = 2; // start bit
    const T2: u8 = 5; // data bit
    const T3: u8 = 3; // stop bit

    let side_set = pio::SideSet::new(false, 1, false);
    let mut a = pio::Assembler::<32>::new_with_side_set(side_set);

    let mut wrap_target = a.label();
    let mut wrap_source = a.label();
    let mut do_zero = a.label();
    a.bind(&mut wrap_target);
    a.out_with_delay_and_side_set(pio::OutDestination::X, 1, T3 - 1, 0);
    a.jmp_with_delay_and_side_set(pio::JmpCondition::XIsZero, &mut do_zero, T1 - 1, 1);
    a.jmp_with_delay_and_side_set(pio::JmpCondition::Always, &mut wrap_target, T2 - 1, 1);
    a.bind(&mut do_zero);
    a.nop_with_delay_and_side_set(T2 - 1, 0);
    a.bind(&mut wrap_source);

    (
        a.assemble_with_wrap(wrap_source, wrap_target),
        u32::from(T1 + T2 + T3),
    )
}

/// Entry point: set up clocks, configure PIO0 to drive the matrix on
/// GP25, then run the ember animation forever.
#[hal::entry]
fn main() -> ! {
    let mut pac = hal::pac::Peripherals::take().unwrap();
    let mut watchdog = Watchdog::new(pac.WATCHDOG);
    let clocks = init_clocks_and_plls(
        XTAL_FREQ_HZ,
        pac.XOSC,
        pac.CLOCKS,
        pac.PLL_SYS,
        pac.PLL_USB,
        &mut pac.RESETS,
        &mut watchdog,
    )
    .ok()
    .unwrap();
    let sio = Sio::new(pac.SIO);
    let pins = hal::gpio::Pins::new(
        pac.IO_BANK0,
        pac.PADS_BANK0,
        sio.gpio_bank0,
        &mut pac.RESETS,
    );
    let mut timer = hal::Timer::new_timer0(pac.TIMER0, &mut pac.RESETS, &clocks);

    // Hand GP25 (the matrix data line) to PIO0.
    let led: Pin<_, FunctionPio0, _> = pins.gpio25.into_function();
    let led_pin_id = led.id().num;

    let (mut pio, sm0, _, _, _) = pac.PIO0.split(&mut pac.RESETS);
    let (program, cycles_per_bit) = ws2812_program();
    let installed = pio.install(&program).unwrap();

    // Divide the system clock down so one PIO cycle is 1/10 of a WS2812
    // bit period (16.8 fixed-point divider; frac is in 1/256ths).
    let bit_hz = 800_000 * cycles_per_bit;
    let sys_hz = clocks.system_clock.freq().to_Hz();
    let div_int = (sys_hz / bit_hz) as u16;
    let div_frac = (((sys_hz % bit_hz) * 256) / bit_hz) as u8;

    let (mut sm, _, mut tx) = PIOBuilder::from_installed_program(installed)
        .buffers(Buffers::OnlyTx)
        .side_set_pin_base(led_pin_id)
        .out_shift_direction(ShiftDirection::Left)
        .autopull(true)
        .pull_threshold(24)
        .clock_divisor_fixed_point(div_int, div_frac)
        .build(sm0);
    sm.set_pindirs([(led_pin_id, PinDir::Output)]);
    sm.start();

    // Fixed non-zero seed: the pattern repeats across boots, which is fine
    // for a decorative effect and avoids depending on a hardware RNG.
    let mut rng = XorShift32(0x2A3C_4D5E);
    let mut embers: [Flicker; NUM_LEDS] = core::array::from_fn(|_| Flicker::new(&mut rng, &EMBER));
    let mut glow = Flicker::new(&mut rng, &GLOW);

    loop {
        glow.tick(&mut rng, &GLOW);
        for ember in &mut embers {
            ember.tick(&mut rng, &EMBER);
        }
        for ember in &embers {
            let word = ember_color(ember.value, glow.value);
            // The TX FIFO is only 8 words deep; spin until there is room.
            while !tx.write(word) {
                core::hint::spin_loop();
            }
        }
        // The frame delay dwarfs the 60 us WS2812 latch time, so the
        // strip latches between frames without an explicit reset wait.
        timer.delay_ms(FRAME_MS);
    }
}
