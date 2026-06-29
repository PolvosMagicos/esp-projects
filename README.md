# ESP Projects

A collection of small ESP projects built to learn embedded development. The
goal is to write the firmware, supporting tools, and interfaces in Rust while
experimenting with hardware, protocols, and constrained systems.

And some projects (like the first one) will be just tik-toks that I saw and want to replicate :b.

Most firmware projects will require a Rust ESP toolchain and a supported ESP
development board. Refer to the individual project documentation before
building or flashing.

## Projects

### [OLED Video Player](./oled-video-player/)

Streams video from a Rust web application to an ESP32-S3 connected to a
128 × 64 monochrome OLED. The project includes ESP-IDF firmware, a Dioxus web
interface, and a shared Rust protocol crate.

See the [project README](./oled-video-player/README.md) for hardware
requirements, setup, and usage.

## Approach

- Use Rust across the stack wherever possible.
- Keep each project self-contained and documented.
- Record wiring, configuration, build, and flashing instructions with each
  project.
