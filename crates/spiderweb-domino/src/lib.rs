//! Domino clipboard format (MidiPortalSequence) encode/decode, a faithful port of Python
//! `files/domino_clip.py`.
//!
//! The clipboard format is `MAGIC` + decompressed length (u32) + zlib data; decompressed it
//! is nested `[tag u16][length u32][data]` items: one 1003 track item per copied channel
//! (slot), holding track settings, notes, the length of the copied range and more settings.
//! A note is a 2001 item containing 1001 start tick (u32), 2001 key (u8), 2002 velocity (u8)
//! and 2003 duration (u32).
//!
//! - [`clip_data`]: note rows -> clipboard bytes (for copying into Domino).
//! - [`read_notes`]: clipboard bytes -> note rows + PPQ (for pasting from Domino); controller
//!   and other events are skipped.
//! - [`put_on_clipboard`] / [`get_from_clipboard`]: Windows clipboard FFI; other platforms
//!   return false / [`ClipboardGet::NoData`].

mod clipboard;
mod codec;

pub use clipboard::{ClipboardGet, FORMAT, get_from_clipboard, put_on_clipboard};
pub use codec::{
    DominoStart, Error, MAGIC, Note6, clip_data, item, read_notes, read_notes_max_key,
};
