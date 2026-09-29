# AGENTS.md — spiderweb-rs

Guidance for AI coding agents and contributors working on this repository.
Read this together with [README.md](README.md) and [TESTING.md](TESTING.md).

## Project

- A native Rust port of [Spiderweb](https://github.com/UnPrioritized/Spiderweb)
  (Python/Tkinter, MIT, © 2026 Kanade Tachibana).
- **Keep tracking upstream**: when a new upstream version ships, port its
  changes too (the current target is 1.2.0). The Python sources are reference
  material, never a runtime or build dependency.
- The long-term plan is to move the UI-free engine (`spiderweb-core`) into the
  author's main editor (Yinhe). Keep `spiderweb-core` free of UI/platform code.
- The `tools/` Python scripts are **development-only** (differential-test
  oracle, project/help data generation). They never run in CI or at runtime.

## Language

- **All code comments, doc comments, test names, markdown docs and commit
  subjects must be English.** Existing Chinese comments are being migrated
  incrementally; new code must not add new Chinese comments.
- Localisation: user-visible strings live in `crates/spiderweb-app/locales/en.yml`
  and go through `rust_i18n::t!(...)`. English text must match the original
  Spiderweb wording verbatim; do not invent new strings.
- Commit messages are **English**: `type(scope): summary` (imperative, e.g.
  `feat(core): port the 1.2.0 snap system`), followed by a blank line and
  `- bullet` details. Keep them short. History before this rule is Chinese;
  do not rewrite it.

## Commands

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release -p spiderweb-app

# Core micro-benchmarks (prints libtest bencher lines for CI)
cargo bench -p spiderweb-core --bench engine

# Regenerate differential vectors from the Python reference (dev only)
SPIDERWEB_SRC=/path/to/upstream/scripts python3 tools/gen_paths_vectors.py
```

Performance HUD: `SPIDERWEB_PERF=1 cargo run --release -p spiderweb-app`.
Bench project: `cargo xtask bench-project --notes 1000000 -o bench.json`.
i18n catalog check: `cargo xtask i18n-sync --upstream /path/to/upstream/scripts`.

## Code rules

1. **No `unwrap()` / `expect()` in production code.** Tests may use them.
2. Every behaviour change needs a test. Differential vectors (`tests/vectors/*.json`)
   are the source of truth for engine behaviour; regenerate them against the
   upstream version you are porting, never hand-edit.
3. Keep file sizes reasonable. If a module grows past ~1000 lines, split it
   along responsibility lines instead of appending.
4. Do not reintroduce `midly` or other heavy MIDI dependencies: SMF writing is a
   hand-written port of the original `midi_out.py` and must stay byte-identical.
5. Keep the three format contracts tested and compatible: project JSON, MIDI
   export, Domino clipboard.
6. `spiderweb-core` must not depend on egui, eframe, wgpu or platform APIs.
7. The wgpu note renderer: instance data is 16 bytes per note; pan/zoom must
   only touch uniforms (never re-upload instances); never add cull/LOD without
   benchmark data justifying it.
8. Attribution: keep the original MIT notice in `LICENSE`; do not remove it.

## Architecture map

- `crates/spiderweb-core/src/` — one file per upstream `notes/*.py` module
  (`paths.rs` ↔ `paths.py`, …). `engine.rs` assembles shapes → notes.
- `crates/spiderweb-domino/` ↔ upstream `files/domino_clip.py`.
- `crates/spiderweb-io/` ↔ upstream `files/project.py`, `midi_out.py`,
  `mathexpr.py`, `safefile.py`.
- `crates/spiderweb-app/src/` — UI. `roll*.rs` are the piano roll (view, input,
  drawing, tools), `panels.rs` the side panel, `drawer*.rs` the shape drawer,
  `note_gpu.rs` + `note_gpu.wgsl` the wgpu renderer, `app.rs` the state.

## Verification before handing work back

- Run fmt / clippy / test from the Commands section.
- Update `README.md` numbers or `TESTING.md` when behaviour or coverage changes.
- Report any pre-existing warnings or failures, even if unrelated.
