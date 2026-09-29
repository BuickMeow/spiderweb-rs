//! 工程文件 / autosave / MIDI 导出 / 数学表达式（对应 Python `files/`）。
//!
//! 与 Python 原版逐模块对应：
//! - [`compat`] 形状 ⇄ JSON（`engine.clean_shape` 与 `files.project.short_shape` 的移植）；
//! - [`project`] 工程读写、autosave 与其备份的轮换语义（`files.project`）；
//! - [`snap`] the snap choices (`files.snap`);
//! - [`midi`] 标准 MIDI 文件（format 1）写出（`files.midi_out`）；
//! - [`mathexpr`] 数字框里的数学表达式（`files.mathexpr`）；
//! - [`safefile`] 临时文件 + rename 的原子写（`files.safefile`）。
//!
//! 只依赖 [`spiderweb_core`] 的形状数据模型与已移植的清理函数，不依赖尚未移植的
//! `engine` / `funnel`：形状 → 音符由 app 层接线，MIDI 导出接收最终的 (start, end, key,
//! velocity, slot, owner) 音符行。

pub mod compat;
pub mod mathexpr;
pub mod midi;
pub mod project;
pub mod safefile;
pub mod snap;
