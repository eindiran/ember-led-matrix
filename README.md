# ember-led-matrix

Rust firmware for the Waveshare RP2350-Matrix that renders a glowing-ember effect on the onboard 8x8 WS2812B LED matrix.

## Hardware

- Board: Waveshare RP2350-Matrix (RP2350A, Cortex-M33)
- LEDs: 64x WS2812B in an 8x8 grid, chained on a single data line at GP25
- Connection: native USB; flashing uses the RP2350 Boot ROM (BOOTSEL mode)

On default brightness, equivalent to `DIM_SCALE_FACTOR=7`, output is capped at 63/255 (for each LED) by the heat-to-color mapping in `src/main.rs` (`ember_color`).
Waveshare quotes ~900 mA for the matrix at full white and recommends thermal limiting. Avoid raising brightness above 7 if the intent is to keep the board running for an extended period.

## Prerequisites

- Stable Rust with the `thumbv8m.main-none-eabihf` target (`rust-toolchain.toml` installs both automatically via rustup)
- [picotool](https://github.com/raspberrypi/picotool) 2.x for flashing (`brew install picotool`)

## Build

Run `make help` or bare `make` to see all commands.

Build with
```bash
make build
```

Flash with
```bash
make flash
```

### BOOTSEL mode

To reflash the device, put it into BOOTSEL mode by holding the BOOT button down and then plugging in the USB-C cable.
It will NOT trigger the fw/LED animation, so if the device is in BOOTSEL mode, you won't see the LEDs running.

### Brightness scale

Overall brightness is a compile-time setting: set `DIM_SCALE_FACTOR` to an integer 1 (dimmest) through 10 (brightest) in the build environment.
Unset defaults to 7, which reproduces the baseline output; invalid values fail the build.

```bash
make flash DIM_SCALE_FACTOR=3
```

Equivalently as an environment variable: `DIM_SCALE_FACTOR=3 cargo build --release`

or:

```bash
export DIM_SCALE_FACTOR=10
# do other stuff
cargo build --release
```

Steps are geometric (luminance ratio 1.55 per step) so each step is a similar perceived change; scale 10 peaks at red 234/255.
Cargo tracks the variable, so changing it triggers a rebuild without touching source.
