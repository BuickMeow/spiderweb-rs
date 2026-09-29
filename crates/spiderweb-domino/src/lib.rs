//! Domino 剪贴板格式（MidiPortalSequence）编解码，忠实移植自 Python 版 `files/domino_clip.py`。
//!
//! 剪贴板格式为 `MAGIC` + 解压后长度（u32）+ zlib 数据；解压后是 `[tag u16][length u32][data]`
//! 的嵌套项：每个复制出去的通道（slot）一个 1003 轨项，里面是轨道设置、音符、复制范围的
//! 长度和更多设置。音符 = 2001 项，含 1001 起点 tick(u32)、2001 key(u8)、2002 力度(u8)、
//! 2003 时值(u32)。
//!
//! - [`clip_data`]：音符行 -> 剪贴板字节（复制到 Domino 用）。
//! - [`read_notes`]：剪贴板字节 -> 音符行 + PPQ（从 Domino 粘贴用），控制器等事件跳过。
//! - [`put_on_clipboard`] / [`get_from_clipboard`]：Windows 剪贴板 FFI；其它平台返回
//!   false / [`ClipboardGet::NoData`]。

mod clipboard;
mod codec;

pub use clipboard::{ClipboardGet, FORMAT, get_from_clipboard, put_on_clipboard};
pub use codec::{Error, MAGIC, Note6, clip_data, item, read_notes, read_notes_max_key};
