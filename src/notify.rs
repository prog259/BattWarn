//! The popup itself: a modal, always-on-top message box with a single OK.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Diagnostics::Debug::MessageBeep;
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, MB_ICONWARNING, MB_OK, MB_SETFOREGROUND, MB_SYSTEMMODAL, MB_TOPMOST,
};

use crate::engine::Level;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn title(level: Level) -> &'static str {
    match level {
        Level::Low => "battwarn \u{2014} battery low",
        Level::Critical => "battwarn \u{2014} battery critical",
        Level::Final => "battwarn \u{2014} final warning",
    }
}

fn body(percent: u8, level: Level) -> String {
    match level {
        Level::Low => format!(
            "Battery at {percent}%.\n\n\
             Windows can shut down without warning from here on.\n\
             Save your work and plug in the charger."
        ),
        Level::Critical => format!(
            "Battery at {percent}%.\n\n\
             Shutdown is imminent. Plug in the charger now,\n\
             then save your work."
        ),
        Level::Final => format!(
            "Battery at {percent}%.\n\n\
             This machine is about to die.\n\
             PLUG IN THE CHARGER NOW."
        ),
    }
}

/// Blocks until the user clicks OK. Returns the message box result.
pub fn show_blocking(percent: u8, level: Level) -> i32 {
    let text = wide(&body(percent, level));
    let caption = wide(title(level));

    unsafe {
        let _ = MessageBeep(MB_ICONWARNING);
        MessageBoxW(
            HWND(std::ptr::null_mut()),
            PCWSTR::from_raw(text.as_ptr()),
            PCWSTR::from_raw(caption.as_ptr()),
            MB_OK | MB_ICONWARNING | MB_TOPMOST | MB_SETFOREGROUND | MB_SYSTEMMODAL,
        )
        .0
    }
}

/// Non-blocking popup that never stacks on top of itself.
#[derive(Clone, Default)]
pub struct Warner {
    open: Arc<AtomicBool>,
}

impl Warner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns false when a popup is already on screen (the caller should try
    /// again after it has been dismissed).
    pub fn show(&self, percent: u8, level: Level) -> bool {
        if self.open.swap(true, Ordering::SeqCst) {
            return false;
        }
        let flag = self.open.clone();
        std::thread::spawn(move || {
            show_blocking(percent, level);
            flag.store(false, Ordering::SeqCst);
        });
        true
    }

    pub fn is_open(&self) -> bool {
        self.open.load(Ordering::SeqCst)
    }
}
