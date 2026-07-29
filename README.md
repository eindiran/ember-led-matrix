# ember-led-matrix

Bare-metal Rust firmware for the Waveshare ESP32-S3-Matrix that renders a
glowing-ember effect on the onboard 8x8 WS2812B LED matrix: every LED
flickers independently through deep reds and oranges, with occasional
bright sparks.

## Hardware

- Board: Waveshare ESP32-S3-Matrix (ESP32-S3FH4R2)
- LEDs: 64x WS2812B in an 8x8 grid, chained on a single data line at GPIO14
- Connection: native USB (USB Serial/JTAG), enumerates as `/dev/cu.usbmodem*`
  on macOS

Output is capped at 63/255 per channel by the heat-to-color mapping in
`src/main.rs` (`Ember::color`). Waveshare warns that high matrix brightness
heats the board quickly and can damage it; raise the caps with care.

## Prerequisites

- Xtensa Rust toolchain installed via [espup](https://github.com/esp-rs/espup)
  (`rust-toolchain.toml` selects the `esp` channel automatically)
- [espflash](https://github.com/esp-rs/espflash) 4.x for flashing

## Build

```bash
source ~/export-esp.sh  # puts the Xtensa GCC linker on PATH
cargo build --release
```

The target (`xtensa-esp32s3-none-elf`) and `build-std` settings come from
`.cargo/config.toml`.

## Flash

```bash
espflash flash --port /dev/cu.usbmodem11101 \
    target/xtensa-esp32s3-none-elf/release/ember-led-matrix
```

Or `cargo run --release`, which flashes and attaches a serial monitor.

## Design notes

- The WS2812 chain is driven by the ESP32-S3 RMT peripheral through
  `esp-hal-smartled`; no bit-banging, so timing is exact regardless of CPU
  load.
- The LEDs on this board take RGB byte order on the wire, not the usual
  WS2812 GRB (verified empirically: a red frame rendered green with GRB
  encoding), so the adapter is instantiated with the `RGB8` color type.
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
  dependency on the RNG peripheral.
