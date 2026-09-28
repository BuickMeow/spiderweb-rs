//! Windows 剪贴板 FFI（对应原版 `put_on_clipboard` / `get_from_clipboard` 的 ctypes 部分）。
//!
//! 手写 `extern "system"` 声明，不引入额外依赖。非 Windows 平台提供同签名的空实现，
//! 返回 `false` / [`ClipboardGet::NoData`]，保证跨平台编译通过。

/// 剪贴板格式名。
pub const FORMAT: &str = "MidiPortalSequence";

/// [`get_from_clipboard`] 的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardGet {
    /// 剪贴板被别的程序占着（重试 10 次仍打不开，对应原版返回 None）。
    Busy,
    /// 剪贴板上没有这种格式的数据（对应原版返回 b""）。
    NoData,
    /// 数据本身。
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

    /// 注册剪贴板格式，失败返回 None。
    fn register_format() -> Option<Uint> {
        let mut name: Vec<u16> = FORMAT.encode_utf16().collect();
        name.push(0);
        // SAFETY: name 以 NUL 结尾，调用期间一直有效。
        let fmt = unsafe { RegisterClipboardFormatW(name.as_ptr()) };
        (fmt != 0).then_some(fmt)
    }

    /// 原版的重试：10 次、每次等 20ms，别的程序有时会短暂占着剪贴板。
    fn open_clipboard() -> bool {
        for _ in 0..OPEN_RETRIES {
            // SAFETY: NULL 表示当前任务。
            if unsafe { OpenClipboard(std::ptr::null_mut()) } != 0 {
                return true;
            }
            // SAFETY: 参数只是一个毫秒数。
            unsafe { Sleep(RETRY_SLEEP_MS) };
        }
        false
    }

    /// 把 raw 作为 FORMAT 放到剪贴板（替换原有内容）。剪贴板打不开或内存不够返回 false。
    pub fn put_on_clipboard(raw: &[u8]) -> bool {
        let Some(fmt) = register_format() else {
            return false;
        };
        // SAFETY: 只调用系统 API，指针都在使用期间有效。
        unsafe {
            let h = GlobalAlloc(GMEM_MOVEABLE, raw.len());
            if h.is_null() {
                return false; // 原版这里抛 MemoryError
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
                // 失败时内存还在我们手里
                GlobalFree(h);
                CloseClipboard();
                return false;
            }
            // 成功时剪贴板接管 h，不能再释放
            CloseClipboard();
            true
        }
    }

    /// 剪贴板上 FORMAT 的字节；没有数据是 [`ClipboardGet::NoData`]，打不开是
    /// [`ClipboardGet::Busy`]。
    pub fn get_from_clipboard() -> ClipboardGet {
        let Some(fmt) = register_format() else {
            return ClipboardGet::NoData;
        };
        // SAFETY: 只调用系统 API，指针都在使用期间有效。
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
