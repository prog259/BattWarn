//! Tiny terminal toolkit: colour, a battery bar, and human durations.
//!
//! Everything degrades gracefully: if the output is redirected, or the console
//! refuses UTF-8 / VT sequences, we fall back to plain ASCII without escapes.

use windows::Win32::System::Console::{
    GetConsoleMode, GetStdHandle, SetConsoleCP, SetConsoleMode, SetConsoleOutputCP,
    ENABLE_VIRTUAL_TERMINAL_PROCESSING, STD_OUTPUT_HANDLE,
};

pub struct Ui {
    pub unicode: bool,
    pub color: bool,
}

impl Ui {
    pub fn init() -> Ui {
        let color = enable_vt();
        let unicode = enable_utf8();
        Ui { unicode, color }
    }

    fn paint(&self, code: &str, s: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{s}\x1b[0m")
        } else {
            s.to_string()
        }
    }

    pub fn dim(&self, s: &str) -> String {
        self.paint("2", s)
    }

    pub fn bold(&self, s: &str) -> String {
        self.paint("1", s)
    }

    pub fn green(&self, s: &str) -> String {
        self.paint("32", s)
    }

    pub fn yellow(&self, s: &str) -> String {
        self.paint("33", s)
    }

    pub fn red(&self, s: &str) -> String {
        self.paint("31", s)
    }

    pub fn rule(&self, width: usize) -> String {
        let ch = if self.unicode { "\u{2500}" } else { "-" };
        self.dim(&ch.repeat(width))
    }

    /// 20-cell charge bar, coloured by urgency.
    pub fn bar(&self, percent: u8, width: usize) -> String {
        let filled = ((percent as usize * width) + 50) / 100;
        let (full, empty) = if self.unicode {
            ("\u{2588}", "\u{2591}")
        } else {
            ("#", ".")
        };
        let body = format!("{}{}", full.repeat(filled), empty.repeat(width - filled));
        let painted = if percent <= 10 {
            self.red(&body)
        } else if percent <= 25 {
            self.yellow(&body)
        } else {
            self.green(&body)
        };
        format!("[{painted}]")
    }
}

fn enable_vt() -> bool {
    unsafe {
        let Ok(handle) = GetStdHandle(STD_OUTPUT_HANDLE) else {
            return false;
        };
        let mut mode = windows::Win32::System::Console::CONSOLE_MODE(0);
        if GetConsoleMode(handle, &mut mode).is_err() {
            return false;
        }
        let wanted = windows::Win32::System::Console::CONSOLE_MODE(
            mode.0 | ENABLE_VIRTUAL_TERMINAL_PROCESSING.0,
        );
        SetConsoleMode(handle, wanted).is_ok()
    }
}

fn enable_utf8() -> bool {
    unsafe { SetConsoleOutputCP(65001).is_ok() && SetConsoleCP(65001).is_ok() }
}

/// "1h 47m", "12m", "45s", or an em dash when unknown.
pub fn duration(secs: Option<u32>) -> String {
    let Some(secs) = secs else {
        return "\u{2014}".into();
    };
    if secs == 0 {
        return "\u{2014}".into();
    }
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    if h > 0 {
        format!("{h}h {m:02}m")
    } else if m > 0 {
        format!("{m}m")
    } else {
        format!("{secs}s")
    }
}

/// Milliwatt-hours as watt-hours: 47200 -> "47.2 Wh".
pub fn wh(mwh: u32) -> String {
    format!("{:.1} Wh", mwh as f64 / 1000.0)
}

/// Milliwatts as watts: 9800 -> "9.8 W".
pub fn watts(mw: u32) -> String {
    format!("{:.1} W", mw as f64 / 1000.0)
}

/// True when this process is the only thing attached to its console, which is
/// what happens when the user double-clicks the exe. Used to keep the window
/// open instead of flashing and vanishing.
pub fn launched_from_explorer() -> bool {
    use windows::Win32::System::Console::GetConsoleProcessList;
    let mut list = [0u32; 2];
    let count = unsafe { GetConsoleProcessList(&mut list) };
    count == 1
}

pub fn wait_for_enter() {
    use std::io::Write;
    print!("\n{}", "press Enter to close ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);
}
