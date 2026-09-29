//! Project files / autosave / MIDI export / math expressions (mirrors Python `files/`).
//!
//! One module per original Python module:
//! - [`compat`] shape ⇄ JSON (port of `engine.clean_shape` and `files.project.short_shape`);
//! - [`project`] project read/write and autosave/backup rotation semantics (`files.project`);
//! - [`snap`] the snap choices (`files.snap`);
//! - [`midi`] standard MIDI file (format 1) writing (`files.midi_out`);
//! - [`mathexpr`] math expressions in numeric fields (`files.mathexpr`);
//! - [`safefile`] atomic write via temp file + rename (`files.safefile`).
//!
//! Depends only on [`spiderweb_core`]'s shape data model and the ported cleanup helpers,
//! not on the unported `engine` / `funnel`: shape → notes is wired up by the app layer,
//! and MIDI export receives the final (start, end, key, velocity, slot, owner) note rows.

pub mod compat;
pub mod mathexpr;
pub mod midi;
pub mod project;
pub mod safefile;
pub mod snap;
