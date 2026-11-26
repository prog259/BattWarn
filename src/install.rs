//! Install / uninstall: files, PATH, autostart, and the background watcher.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, ERROR_SUCCESS, HANDLE,
};
use windows::Win32::System::Console::{FreeConsole, GetConsoleWindow};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_DWORD, REG_EXPAND_SZ, REG_OPEN_CREATE_OPTIONS,
    REG_SAM_FLAGS, REG_SZ, REG_VALUE_TYPE,
};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, ResetEvent, SetEvent, WaitForSingleObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    SendMessageTimeoutW, HWND_BROADCAST, SW_HIDE, ShowWindow, WM_SETTINGCHANGE,
};

use crate::Config;

pub const APP: &str = "battwarn";
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const ENV_KEY: &str = "Environment";
const ARP_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\battwarn";
const MUTEX_NAME: &str = r"Local\battwarn-watch";
const STOP_EVENT: &str = r"Local\battwarn-stop";

pub type R<T> = Result<T, String>;

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn local_app_data() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Users\Public"))
}

pub fn install_dir() -> PathBuf {
    local_app_data().join("Programs").join(APP)
}

pub fn data_dir() -> PathBuf {
    local_app_data().join(APP)
}

pub fn log_path() -> PathBuf {
    data_dir().join("battwarn.log")
}

pub fn installed_exe() -> PathBuf {
    install_dir().join(format!("{APP}.exe"))
}

fn current_exe() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| installed_exe())
}

// ---------------------------------------------------------------------------
// Registry helpers
// ---------------------------------------------------------------------------

fn open_key(root: HKEY, sub: &str, access: REG_SAM_FLAGS) -> Option<HKEY> {
    let sub = wide(sub);
    let mut out = HKEY::default();
    let rc = unsafe {
        RegCreateKeyExW(
            root,
            PCWSTR::from_raw(sub.as_ptr()),
            0,
            PCWSTR::null(),
            REG_OPEN_CREATE_OPTIONS(0),
            access,
            None,
            &mut out,
            None,
        )
    };
    if rc == ERROR_SUCCESS {
        Some(out)
    } else {
        None
    }
}

fn reg_set(key: HKEY, name: &str, kind: REG_VALUE_TYPE, data: &[u8]) -> bool {
    let name = wide(name);
    unsafe {
        RegSetValueExW(
            key,
            PCWSTR::from_raw(name.as_ptr()),
            0,
            kind,
            Some(data),
        ) == ERROR_SUCCESS
    }
}

fn reg_set_str(key: HKEY, name: &str, value: &str, kind: REG_VALUE_TYPE) -> bool {
    let mut bytes: Vec<u8> = Vec::with_capacity(value.len() * 2 + 2);
    for unit in value.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes.extend_from_slice(&0u16.to_le_bytes());
    reg_set(key, name, kind, &bytes)
}

fn reg_set_u32(key: HKEY, name: &str, value: u32) -> bool {
    reg_set(key, name, REG_DWORD, &value.to_le_bytes())
}

fn reg_delete(key: HKEY, name: &str) -> bool {
    let name = wide(name);
    unsafe { RegDeleteValueW(key, PCWSTR::from_raw(name.as_ptr())) == ERROR_SUCCESS }
}

fn reg_get_string(key: HKEY, name: &str) -> Option<(String, REG_VALUE_TYPE)> {
    let name = wide(name);
    let mut kind = REG_SZ;
    let mut size: u32 = 0;
    let rc = unsafe {
        RegQueryValueExW(
            key,
            PCWSTR::from_raw(name.as_ptr()),
            None,
            Some(&mut kind),
            None,
            Some(&mut size),
        )
    };
    if rc != ERROR_SUCCESS || size == 0 {
        return None;
    }
    let mut buf = vec![0u8; size as usize];
    let rc = unsafe {
        RegQueryValueExW(
            key,
            PCWSTR::from_raw(name.as_ptr()),
            None,
            Some(&mut kind),
            Some(buf.as_mut_ptr()),
            Some(&mut size),
        )
    };
    if rc != ERROR_SUCCESS {
        return None;
    }
    buf.truncate(size as usize);
    let units: Vec<u16> = buf
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|&u| u != 0)
        .collect();
    Some((String::from_utf16_lossy(&units), kind))
}

// ---------------------------------------------------------------------------
// Autostart
// ---------------------------------------------------------------------------

fn autostart_args(cfg: &Config) -> String {
    format!(
        "watch --daemon --quiet --threshold {} --step {} --interval {}",
        cfg.threshold, cfg.step, cfg.interval
    )
}

fn autostart_value(cfg: &Config) -> String {
    format!("\"{}\" {}", installed_exe().display(), autostart_args(cfg))
}

pub fn write_autostart(cfg: &Config) -> R<()> {
    let key = open_key(HKEY_CURRENT_USER, RUN_KEY, KEY_WRITE)
        .ok_or_else(|| "cannot open the Run registry key".to_string())?;
    let ok = reg_set_str(key, APP, &autostart_value(cfg), REG_SZ);
    unsafe { let _ = RegCloseKey(key); }
    if ok {
        Ok(())
    } else {
        Err("cannot write the autostart entry".into())
    }
}

pub fn remove_autostart() {
    if let Some(key) = open_key(HKEY_CURRENT_USER, RUN_KEY, KEY_WRITE) {
        reg_delete(key, APP);
        unsafe { let _ = RegCloseKey(key); }
    }
}

/// Reads the threshold/step/interval back out of the autostart command line.
pub fn autostart_config() -> Option<Config> {
    let key = open_key(HKEY_CURRENT_USER, RUN_KEY, KEY_READ)?;
    let value = reg_get_string(key, APP).map(|(s, _)| s);
    unsafe { let _ = RegCloseKey(key); }
    let value = value?;

    let mut cfg = Config::default();
    let tokens: Vec<&str> = value.split_whitespace().collect();
    let mut i = 0;
    while i < tokens.len() {
        let (flag, inline) = match tokens[i].split_once('=') {
            Some((f, v)) => (f, Some(v)),
            None => (tokens[i], None),
        };
        let next = || -> Option<&str> {
            if let Some(v) = inline {
                return Some(v);
            }
            tokens.get(i + 1).copied()
        };
        match flag {
            "--threshold" | "-t" => {
                if let Some(v) = next().and_then(|v| v.parse().ok()) {
                    cfg.threshold = v;
                    if inline.is_none() { i += 1; }
                }
            }
            "--step" | "-s" => {
                if let Some(v) = next().and_then(|v| v.parse().ok()) {
                    cfg.step = v;
                    if inline.is_none() { i += 1; }
                }
            }
            "--interval" | "-i" => {
                if let Some(v) = next().and_then(|v| v.parse().ok()) {
                    cfg.interval = v;
                    if inline.is_none() { i += 1; }
                }
            }
            _ => {}
        }
        i += 1;
    }
    Some(cfg)
}

// ---------------------------------------------------------------------------
// PATH
// ---------------------------------------------------------------------------

fn read_user_path() -> (String, REG_VALUE_TYPE) {
    if let Some(key) = open_key(HKEY_CURRENT_USER, ENV_KEY, KEY_READ) {
        let got = reg_get_string(key, "Path");
        unsafe { let _ = RegCloseKey(key); }
        if let Some(v) = got {
            return v;
        }
    }
    (String::new(), REG_EXPAND_SZ)
}

fn write_user_path(value: &str, kind: REG_VALUE_TYPE) {
    if let Some(key) = open_key(HKEY_CURRENT_USER, ENV_KEY, KEY_WRITE) {
        reg_set_str(key, "Path", value, kind);
        unsafe { let _ = RegCloseKey(key); }
    }
    broadcast_environment_change();
}

fn broadcast_environment_change() {
    let mut name = wide("Environment");
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            windows::Win32::Foundation::WPARAM(0),
            windows::Win32::Foundation::LPARAM(name.as_mut_ptr() as isize),
            windows::Win32::UI::WindowsAndMessaging::SMTO_ABORTIFHUNG,
            2000,
            None,
        );
    }
}

fn path_has(dir: &Path, path: &str) -> bool {
    let needle = dir.to_string_lossy().to_lowercase();
    let needle = needle.trim_end_matches('\\');
    path.split(';')
        .map(|p| p.trim().trim_end_matches('\\').to_lowercase())
        .any(|p| p == needle)
}

pub fn add_to_path(dir: &Path) {
    let (path, kind) = read_user_path();
    if path_has(dir, &path) {
        return;
    }
    let dir = dir.to_string_lossy();
    let new = if path.trim().is_empty() {
        dir.to_string()
    } else {
        format!("{};{}", path.trim_end_matches(';'), dir)
    };
    write_user_path(&new, kind);
}

pub fn remove_from_path(dir: &Path) {
    let (path, kind) = read_user_path();
    if !path_has(dir, &path) {
        return;
    }
    let needle = dir.to_string_lossy().to_lowercase();
    let needle = needle.trim_end_matches('\\');
    let kept: Vec<&str> = path
        .split(';')
        .filter(|p| !p.trim().is_empty())
        .filter(|p| p.trim().trim_end_matches('\\').to_lowercase() != needle)
        .collect();
    write_user_path(&kept.join(";"), kind);
}

// ---------------------------------------------------------------------------
// Watcher lifecycle
// ---------------------------------------------------------------------------

pub fn is_running() -> bool {
    let name = wide(MUTEX_NAME);
    let handle = unsafe { CreateMutexW(None, false, PCWSTR::from_raw(name.as_ptr())) };
    match handle {
        Ok(h) => {
            let exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
            unsafe { let _ = CloseHandle(h); }
            exists
        }
        Err(_) => false,
    }
}

pub fn stop_watcher() {
    let name = wide(STOP_EVENT);
    if let Ok(handle) = unsafe { CreateEventW(None, false, false, PCWSTR::from_raw(name.as_ptr())) } {
        unsafe {
            let _ = SetEvent(handle);
            let _ = CloseHandle(handle);
        }
    }
    // Give it a moment to unwind.
    for _ in 0..30 {
        if !is_running() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    // An auto-reset event that nobody consumed is still signalled. Clear it,
    // otherwise the next watcher would see it and quit on its first tick.
    let name = wide(STOP_EVENT);
    if let Ok(handle) = unsafe { CreateEventW(None, false, false, PCWSTR::from_raw(name.as_ptr())) } {
        unsafe {
            let _ = ResetEvent(handle);
            let _ = CloseHandle(handle);
        }
    }
}

/// Held for the lifetime of the watcher; also the single-instance lock.
pub struct InstanceGuard(HANDLE);

impl InstanceGuard {
    pub fn acquire() -> Option<InstanceGuard> {
        let name = wide(MUTEX_NAME);
        let handle = unsafe { CreateMutexW(None, true, PCWSTR::from_raw(name.as_ptr())) }.ok()?;
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe { let _ = CloseHandle(handle); }
            return None;
        }
        Some(InstanceGuard(handle))
    }
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        unsafe { let _ = CloseHandle(self.0); }
    }
}

pub struct StopEvent(HANDLE);

impl StopEvent {
    pub fn create() -> Option<StopEvent> {
        let name = wide(STOP_EVENT);
        let handle = unsafe { CreateEventW(None, false, false, PCWSTR::from_raw(name.as_ptr())) }.ok()?;
        Some(StopEvent(handle))
    }

    /// Sleeps for up to `secs`, returning true if asked to stop.
    pub fn wait(&self, secs: u64) -> bool {
        let ms = (secs.min(3600) * 1000) as u32;
        unsafe { WaitForSingleObject(self.0, ms).0 == 0 }
    }
}

impl Drop for StopEvent {
    fn drop(&mut self) {
        unsafe { let _ = CloseHandle(self.0); }
    }
}

pub fn spawn_daemon(cfg: &Config) -> R<()> {
    let exe = installed_exe();
    let exe = if exe.exists() { exe } else { current_exe() };
    let mut cmd = Command::new(&exe);
    cmd.args([
        "watch",
        "--daemon",
        "--quiet",
        "--threshold",
        &cfg.threshold.to_string(),
        "--step",
        &cfg.step.to_string(),
        "--interval",
        &cfg.interval.to_string(),
    ]);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(0x0800_0000 | 0x0000_0008); // CREATE_NO_WINDOW | DETACHED_PROCESS
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("cannot start the watcher: {e}"))
}

pub fn detach_console() {
    unsafe {
        let hwnd = GetConsoleWindow();
        if !hwnd.0.is_null() {
            let _ = ShowWindow(hwnd, SW_HIDE);
            let _ = FreeConsole();
        }
    }
}

// ---------------------------------------------------------------------------
// Add/Remove Programs entry
// ---------------------------------------------------------------------------

fn write_arp_entry() {
    let Some(key) = open_key(HKEY_CURRENT_USER, ARP_KEY, KEY_WRITE) else {
        return;
    };
    let exe = installed_exe();
    let exe_str = exe.display().to_string();
    reg_set_str(key, "DisplayName", APP, REG_SZ);
    reg_set_str(key, "DisplayVersion", env!("CARGO_PKG_VERSION"), REG_SZ);
    reg_set_str(key, "Publisher", APP, REG_SZ);
    reg_set_str(key, "InstallLocation", &install_dir().display().to_string(), REG_SZ);
    reg_set_str(key, "DisplayIcon", &exe_str, REG_SZ);
    reg_set_str(key, "UninstallString", &format!("\"{exe_str}\" uninstall"), REG_SZ);
    reg_set_str(key, "QuietUninstallString", &format!("\"{exe_str}\" uninstall --quiet"), REG_SZ);
    reg_set_u32(key, "NoModify", 1);
    reg_set_u32(key, "NoRepair", 1);
    if let Ok(meta) = std::fs::metadata(&exe) {
        reg_set_u32(key, "EstimatedSize", (meta.len() / 1024) as u32);
    }
    unsafe { let _ = RegCloseKey(key); }
}

fn remove_arp_entry() {
    use windows::Win32::System::Registry::RegDeleteKeyW;
    let sub = wide(ARP_KEY);
    unsafe {
        let _ = RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR::from_raw(sub.as_ptr()));
    }
}

// ---------------------------------------------------------------------------
// Public verbs
// ---------------------------------------------------------------------------

pub fn install(cfg: &Config) -> R<()> {
    let target = installed_exe();
    let dir = install_dir();
    let source = current_exe();

    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;

    let same_file = source
        .canonicalize()
        .ok()
        .zip(target.canonicalize().ok())
        .map(|(a, b)| a == b)
        .unwrap_or(false);

    if !same_file {
        std::fs::copy(&source, &target)
            .map_err(|e| format!("cannot copy to {}: {e}", target.display()))?;
    }

    // A running watcher holds the old settings; restart it with the new ones.
    stop_watcher();

    write_autostart(cfg)?;
    add_to_path(&dir);
    write_arp_entry();
    std::fs::create_dir_all(data_dir()).ok();

    spawn_daemon(cfg)?;

    if cfg.quiet {
        return Ok(());
    }

    println!();
    println!("  installed  {}", target.display());
    println!("  autostart  at logon");
    println!(
        "  warn at    {}%  {}",
        cfg.threshold,
        crate::ui::Ui::init().dim(&format!("(then every {}% down)", cfg.step))
    );
    println!("  watching   started");
    println!();
    println!("  run \"battwarn\" anytime for status.");
    if !same_file {
        println!();
        println!(
            "  {}",
            crate::ui::Ui::init()
                .dim("open a new terminal for the battwarn command to be on PATH")
        );
    }
    Ok(())
}

pub fn uninstall(cfg: &Config) -> R<()> {
    stop_watcher();
    remove_autostart();
    remove_from_path(&install_dir());
    remove_arp_entry();

    let dir = install_dir();
    let running_from_install_dir = current_exe()
        .parent()
        .map(|p| p == dir.as_path())
        .unwrap_or(false);

    if cfg.keep_files {
        return Ok(());
    }

    if running_from_install_dir {
        // We cannot delete the exe we are running from. Ask cmd.exe to do it
        // once we are gone.
        let script = format!(
            "ping -n 3 127.0.0.1 >nul & rmdir /s /q \"{}\"",
            dir.display()
        );
        let _ = Command::new("cmd.exe")
            .args(["/C", &script])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x0800_0000)
            .spawn();
    } else {
        let _ = std::fs::remove_dir_all(&dir);
    }

    let _ = std::fs::remove_dir_all(data_dir());
    Ok(())
}
