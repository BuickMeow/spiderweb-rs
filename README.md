# Spiderweb (Rust)

Draw on a piano roll, get MIDI notes. Spiderweb turns lines, curves, shapes,
funnels and text into black-MIDI note structures (the "spiderweb" technique)
and copies them straight into Domino.

This is a **native Rust port** of
[Spiderweb](https://github.com/UnPrioritized/Spiderweb) by Kanade Tachibana
(Python/Tkinter, MIT). The goal is the same workflow with no lag: a real GPU
renderer, a pure-Rust note engine and no Python runtime.

- **Original:** Spiderweb © 2026 Kanade Tachibana — MIT
- **Port:** © 2026 节能降耗 — MIT (see [LICENSE](LICENSE))

## Status

| Area | State |
|---|---|
| Feature parity | upstream **1.2.0** complete (256-key mode, custom fill upgrade, join/split, turn-into-live-shape, tumour graphs/rotation/slant, snap system, Domino start, history panel) |
| Correctness | **3000+ differential test cases** against the original Python engine (paths, bezier/arc, smooth, tumour, text, custom, convert, funnel, joined, engine) |
| Formats | MIDI export is **byte-identical** to the original; project JSON and the Domino clipboard format are cross-checked with Python |
| Tests | 215 workspace tests, `clippy -D warnings` clean, `cargo fmt --check` clean |
| CI | `.github/workflows/ci.yml` — fmt / clippy / test / release build on Ubuntu, macOS and Windows |
| Performance | see below; tracked per commit on the [benchmark dashboard](https://buickmeow.github.io/spiderweb-rs/dev/bench/) |

## Performance

Rendering is a custom wgpu instanced-quad pipeline (16 bytes per note).
Panning and zooming only update a uniform buffer — the instance buffers are
re-uploaded only when the notes or the selection change, and are split into
chunks smaller than `max_buffer_size`, so tens of millions of notes fit.
The note model is compact too: every rendered note is a 16-byte struct
(`start` / `end` ticks, key, velocity, slot, owner) instead of the original's
int64 NumPy rows, so 100 M notes hold their `rendered` data in ≈1.6 GB
instead of ≈4.8 GB.
Core algorithms are scalar Rust ports of the vectorised Python code.

Measured on a **MacBook Air M5 (32 GB, macOS 27.0)**, release build; frame
times need the window to be visible. The
[CI dashboard](https://buickmeow.github.io/spiderweb-rs/dev/bench/) tracks
regressions per commit, and `TESTING.md` has the full procedure.

Core algorithms (per shape):

| Case | Time |
|---|---|
| Generate notes for a 64-key line | ~6 µs |
| Tumour line (64 keys) | ~0.66 ms |
| Fill a spam shape, 100k notes | ~1.8 ms |
| Fill a spam funnel, 100k notes | ~4.7 ms |
| `render` 100k notes (overlaps + channels) | ~2.5 ms |

Large projects, whole roll in view (`cargo xtask bench-project`):

| Notes | Load + first render | Peak RSS (load) | Frame p50 | Frame p95 |
|---|---|---|---|---|
| 10 M | 0.55 s | 0.9 GB | 100 ms | 117 ms |
| 25 M | 1.7 s | 1.6 GB | 267 ms | 284 ms |
| 50 M | 4.1 s | 6.4 GB | 431 ms | 456 ms |
| 100 M | 8.2 s | 10.4 GB | 533 ms | 1069 ms |

Frame time scales with the number of **visible** notes: a start-tick index
culls notes outside the viewport (expanded by one screen of ticks and 8 keys on
each side) before upload, and the uploaded slice is reused while panning inside
that margin. Zoomed in, a 25 M-note project uploads a few thousand instances
(tens of KB) instead of 432 MB and runs at the 60 Hz vsync cap. The table above
is the fully zoomed-out (fit) case, which still draws every note; summary/LOD
blocks for that case are the follow-up. Instance buffers are chunked, so
projects with tens of millions of notes no longer hit wgpu's 256 MiB default
buffer limit.

Reproduce locally:

```bash
cargo bench -p spiderweb-core --bench engine   # core micro-benchmarks
cargo xtask bench-project --notes 5000000 -o bench.json
SPIDERWEB_PERF=1 cargo run --release -p spiderweb-app   # frame-time HUD
```

## Build and run

```bash
cargo run -p spiderweb-app --release
```

Requirements: a stable Rust toolchain. On Linux, `libxkbcommon-dev`,
`libwayland-dev`, `libx11-dev`, `libasound2-dev` and `pkg-config`.
Playback uses the system MIDI output (`midir`); the Domino clipboard is
Windows-only by design (the button reports that on other platforms).

## Compatibility

- **Project files**: reads and writes the original `autosave.json` / project
  format (`shapes/*.json` library included).
- **MIDI export**: byte-for-byte identical to the Python original.
- **Domino clipboard**: same `MidiPortalSequence` format; with `Keys: 256` the
  port copies keys 128–255 too (standard Domino reads 0–127).

## Crates

| Crate | Contents |
|---|---|
| `spiderweb-core` | shapes → notes: paths, bezier, arc, smooth, tumour, envelope, custom, convert, funnel, joined, text, fonts, engine (no UI, no platform code) |
| `spiderweb-domino` | Domino clipboard codec + Windows clipboard FFI |
| `spiderweb-io` | project files / autosave, MIDI writer, math expressions, atomic writes |
| `spiderweb-app` | eframe/egui app, wgpu note renderer, tools, panels, drawers, help |

`spiderweb-core` is UI-free on purpose: it is the part meant to move into
Yinhe (the author's main black-MIDI editor) once the port is done.

## Testing

See [TESTING.md](TESTING.md) for the automated suites, the manual GUI
checklist, the performance procedure and the CI/benchmark setup.

## License

MIT. This is a derivative work of Spiderweb; the original copyright notice is
kept in [LICENSE](LICENSE).
