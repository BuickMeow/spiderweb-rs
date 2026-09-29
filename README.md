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
| Feature parity | upstream **1.1.0** complete; **1.2.0** in progress (done: 256-key mode, custom fill upgrade, turn-into-live-shape; remaining: join/split, tumour graphs, snap system, history panel, Domino start) |
| Correctness | **2700+ differential test cases** against the original Python engine (paths, bezier/arc, smooth, tumour, text, custom, convert, funnel, engine) |
| Formats | MIDI export is **byte-identical** to the original; project JSON and the Domino clipboard format are cross-checked with Python |
| Tests | 164 workspace tests, `clippy -D warnings` clean, `cargo fmt --check` clean |
| CI | `.github/workflows/ci.yml` — fmt / clippy / test / release build on Ubuntu, macOS and Windows |
| Performance | see below; tracked per commit on the [benchmark dashboard](https://buickmeow.github.io/spiderweb-rs/dev/bench/) |

## Performance

Rendering is a custom wgpu instanced-quad pipeline (16 bytes per note).
Panning and zooming only update a uniform buffer — the instance buffer is
re-uploaded only when the notes or the selection change. Core algorithms are
scalar Rust ports of the vectorised Python code.

Measured on an Apple Silicon Mac (release build), for reference only —
the [CI dashboard](https://buickmeow.github.io/spiderweb-rs/dev/bench/) tracks
regressions per commit, and `TESTING.md` has the full procedure:

| Case | Time |
|---|---|
| Generate notes for a 64-key line | ~6 µs |
| Tumour line (64 keys) | ~0.66 ms |
| Fill a spam shape, 100k notes | ~1.8 ms |
| Fill a spam funnel, 100k notes | ~4.7 ms |
| `render` 100k notes (overlaps + channels) | ~2.5 ms |
| App: load project + first render, 327k notes | ~25 ms |

Reproduce locally:

```bash
cargo bench -p spiderweb-core --bench engine   # core micro-benchmarks
python3 tools/gen_bench_project.py --notes 5000000 -o bench.json
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
| `spiderweb-core` | shapes → notes: paths, bezier, arc, smooth, tumour, envelope, custom, convert, funnel, text, fonts, engine (no UI, no platform code) |
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
