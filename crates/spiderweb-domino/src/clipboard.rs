//! Windows clipboard FFI (corresponds to the ctypes part of the original `put_on_clipboard` /
//! `get_from_clipboard`).
//!
//! Hand-written `extern "system"` declarations, no extra dependencies. Non-Windows platforms
//! get empty implementations with the same signatures, returning `false` /
//! [`ClipboardGet::NoData`], so the code still compiles cross-platform.

/// Clipboard format name.
pub const FORMAT: &str = "MidiPortalSequence";

/// Result of [`get_from_clipboard`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardGet {
    /// The clipboard is held by another program (still unopenable after 10 retries, matching the original returning None).
    Busy,
    /// The clipboard has no data in this format (matching the original returning b"").
    NoData,
    /// The data itself.
    Data(Vec<u8>),
}

#[cfg(not(windows))]
pub fn put_on_clipboard(_raw: &[u8]) -> bool {
    false
}

#[cfg(not(windows))]
pub fn get_from_clipboard() -> ClipboardGet {
    ClipboardGet::NoData
}

#[cfg(windows)]
pub use win::{get_from_clipboard, put_on_clipboard};

#[cfg(windows)]
mod win {
    use super::{ClipboardGet, FORMAT};
    use std::ffi::c_void;

    type Bool = i32;
    type Uint = u32;
    type Handle = *mut c_void;

    const GMEM_MOVEABLE: Uint = 0x0002;
    const OPEN_RETRIES: u32 = 10;
    const RETRY_SLEEP_MS: u32 = 20;

    #[allow(non_snake_case)]
    #[link(name = "user32")]
    unsafe extern "system" {
        fn RegisterClipboardFormatW(name: *const u16) -> Uint;
        fn OpenClipboard(new_owner: Handle) -> Bool;
        fn EmptyClipboard() -> Bool;
        fn SetClipboardData(format: Uint, mem: Handle) -> Handle;
        fn GetClipboardData(format: Uint) -> Handle;
        fn CloseClipboard() -> Bool;
    }

    #[allow(non_snake_case)]
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalAlloc(flags: Uint, bytes: usize) -> Handle;
        fn GlobalLock(mem: Handle) -> *mut c_void;
        fn GlobalUnlock(mem: Handle) -> Bool;
        fn GlobalSize(mem: Handle) -> usize;
        fn GlobalFree(mem: Handle) -> Handle;
        fn Sleep(ms: u32);
    }

    /// Register the clipboard format; returns None on failure.
    fn register_format() -> Option<Uint> {
        let mut name: Vec<u16> = FORMAT.encode_utf16().collect();
        name.push(0);
        // SAFETY: name is NUL-terminated and stays valid for the duration of the call.
        let fmt = unsafe { RegisterClipboardFormatW(name.as_ptr()) };
        (fmt != 0).then_some(fmt)
    }

    /// The original's retry: 10 attempts, 20ms apart; other programs sometimes hold the clipboard briefly.
    fn open_clipboard() -> bool {
        for _ in 0..OPEN_RETRIES {
            // SAFETY: NULL means the current task.
            if unsafe { OpenClipboard(std::ptr::null_mut()) } != 0 {
                return true;
            }
            // SAFETY: the argument is just a millisecond count.
            unsafe { Sleep(RETRY_SLEEP_MS) };
        }
        false
    }

    /// Put raw on the clipboard as FORMAT (replacing the previous content). Returns false when the clipboard cannot be opened or memory is short.
    pub fn put_on_clipboard(raw: &[u8]) -> bool {
        let Some(fmt) = register_format() else {
            return false;
        };
        // SAFETY: only system APIs are called; the pointers stay valid for the duration.
        unsafe {
            let h = GlobalAlloc(GMEM_MOVEABLE, raw.len());
            if h.is_null() {
                return false; // the original raises MemoryError here
            }
            let p = GlobalLock(h);
            if p.is_null() {
                GlobalFree(h);
                return false;
            }
            std::ptr::copy_nonoverlapping(raw.as_ptr(), p.cast::<u8>(), raw.len());
            GlobalUnlock(h);
            if !open_clipboard() {
                GlobalFree(h);
                return false;
            }
            EmptyClipboard();
            if SetClipboardData(fmt, h).is_null() {
                // on failure the memory is still ours
                GlobalFree(h);
                CloseClipboard();
                return false;
            }
            // on success the clipboard takes over h, so it must not be freed
            CloseClipboard();
            true
        }
    }

    /// The bytes of FORMAT on the clipboard; no data is [`ClipboardGet::NoData`], an
    /// unopenable clipboard is [`ClipboardGet::Busy`].
    pub fn get_from_clipboard() -> ClipboardGet {
        let Some(fmt) = register_format() else {
            return ClipboardGet::NoData;
        };
        // SAFETY: only system APIs are called; the pointers stay valid for the duration.
        unsafe {
            if !open_clipboard() {
                return ClipboardGet::Busy;
            }
            let h = GetClipboardData(fmt);
            if h.is_null() {
                CloseClipboard();
                return ClipboardGet::NoData;
            }
            let p = GlobalLock(h);
            if p.is_null() {
                CloseClipboard();
                return ClipboardGet::NoData;
            }
            let size = GlobalSize(h);
            let mut out = vec![0u8; size];
            std::ptr::copy_nonoverlapping(p.cast::<u8>(), out.as_mut_ptr(), size);
            GlobalUnlock(h);
            CloseClipboard();
            ClipboardGet::Data(out)
        }
    }
}
