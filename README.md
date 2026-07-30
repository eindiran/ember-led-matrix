# ember-led-matrix

Bare-metal Rust firmware for the Waveshare RP2350-Matrix that renders a
glowing-ember effect on the onboard 8x8 WS2812B LED matrix: every LED
flickers independently through deep reds, with occasional bright sparks
and a slow whole-matrix swell.

## Hardware

- Board: Waveshare RP2350-Matrix (RP2350A, Cortex-M33)
- LEDs: 64x WS2812B in an 8x8 grid, chained on a single data line at GP25
- Connection: native USB; flashing uses the RP2350 Boot ROM (BOOTSEL mode)

Output is capped at 63/255 per channel by the heat-to-color mapping in
`src/main.rs` (`ember_color`). Waveshare quotes ~900 mA for the matrix at
full white and recommends thermal limiting; raise the caps with care.

## Prerequisites

- Stable Rust with the `thumbv8m.main-none-eabihf` target
  (`rust-toolchain.toml` installs both automatically via rustup)
- [picotool](https://github.com/raspberrypi/picotool) 2.x for flashing
  (`brew install picotool`)

## Build

```bash
cargo build --release
```

The target and linker configuration come from `.cargo/config.toml`;
`memory.x` provides the RP2350 memory map and boot block sections.

## Flash

```bash
picotool load -u -v -x -t elf \
    target/thumbv8m.main-none-eabihf/release/ember-led-matrix -f
```

The trailing `-f` force-reboots a running device into BOOTSEL mode first;
alternatively hold the BOOT button while plugging in, then run the same
command without `-f`. `cargo run --release` invokes the picotool runner
configured in `.cargo/config.toml` (expects BOOTSEL mode).

## Design notes

- The RP2350 has no dedicated smart-LED peripheral, so a small PIO
  program (the classic 10-cycles-per-bit WS2812 program from
  pico-examples) generates the 800 kHz one-wire timing on GP25; the CPU
  feeds one 24-bit color word per LED into the PIO TX FIFO each frame.
- The matrix LEDs take RGB byte order on the wire, not the usual WS2812
  GRB (confirmed against Waveshare's own demo, which packs
  `R<<24 | G<<16 | B<<8`), so `ember_color` emits red in the top byte.
- Each LED holds an independent heat value that random-walks between a dim
  floor and a bright ceiling, with a 1-in-8 chance of sparking near full
  heat. A second, slower random walk ("glow") scales every LED's heat at
  once, so the whole bed of coals brightens and cools together on top of
  the per-LED flicker. Heat maps to color through a quadratic (gamma-like)
  red curve -- perceived brightness is roughly logarithmic, so this
  spreads the effect from a barely-visible glow (2/255) up to bright
  sparks (63/255) -- plus a small cubic green term (peak 3/255) that keeps
  the palette firmly red with only a faint warm tint at full heat. Frames
  render at ~30 Hz.
- Randomness comes from a fixed-seed xorshift32 PRNG: the pattern repeats
  across boots, which is fine for a decorative effect and avoids a
  dependency on a hardware RNG.
