//! Battery state, straight from the Windows power APIs.

use windows::core::PCWSTR;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_MODE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::Power::{
    CallNtPowerInformation, GetSystemPowerStatus, SYSTEM_BATTERY_STATE, SYSTEM_POWER_STATUS,
};
use windows::Win32::System::Power::SystemBatteryState;

const GENERIC_READ_WRITE: u32 = 0x8000_0000 | 0x4000_0000;

const BATTERY_FLAG_CHARGING: u8 = 8;
const BATTERY_FLAG_CRITICAL: u8 = 4;
const BATTERY_FLAG_NO_BATTERY: u8 = 128;
const UNKNOWN: u8 = 255;

#[derive(Clone, Copy, Debug, Default)]
pub struct Sample {
    pub present: bool,
    pub percent: Option<u8>,
    pub ac_online: bool,
    pub charging: bool,
    pub critical: bool,
    pub saver: bool,
    /// Seconds until empty, when Windows is willing to guess.
    pub secs_left: Option<u32>,
    /// Seconds until full, when charging.
    pub secs_to_full: Option<u32>,
    /// Current draw/charge rate in milliwatts.
    pub rate_mw: Option<u32>,
    /// Remaining charge in milliwatt-hours.
    pub remaining_mwh: Option<u32>,
    /// Current full-charge capacity in milliwatt-hours.
    pub full_mwh: Option<u32>,
}

#[derive(Clone, Copy, Debug)]
pub struct Health {
    pub designed_mwh: u32,
    pub full_mwh: u32,
    pub cycles: Option<u32>,
}

impl Health {
    pub fn percent(&self) -> u8 {
        if self.designed_mwh == 0 {
            return 0;
        }
        let pct = (self.full_mwh as u64 * 100) / self.designed_mwh as u64;
        pct.min(255) as u8
    }
}

fn sane_u32(v: u32) -> Option<u32> {
    if v == 0 || v == u32::MAX {
        None
    } else {
        Some(v)
    }
}

pub fn sample() -> Sample {
    let mut out = Sample::default();

    let mut sps = unsafe { std::mem::zeroed::<SYSTEM_POWER_STATUS>() };
    let have_sps = unsafe { GetSystemPowerStatus(&mut sps) }.is_ok();

    if have_sps {
        out.ac_online = sps.ACLineStatus == 1;
        out.percent = if sps.BatteryLifePercent == UNKNOWN {
            None
        } else {
            Some(sps.BatteryLifePercent.min(100))
        };
        out.charging = sps.BatteryFlag & BATTERY_FLAG_CHARGING != 0;
        out.critical = sps.BatteryFlag & BATTERY_FLAG_CRITICAL != 0;
        out.saver = sps.SystemStatusFlag == 1;
        out.present = sps.BatteryFlag & BATTERY_FLAG_NO_BATTERY == 0
            && sps.BatteryFlag != UNKNOWN
            && out.percent.is_some();

        if !out.charging {
            out.secs_left = sane_u32(sps.BatteryLifeTime);
        } else {
            out.secs_to_full = sane_u32(sps.BatteryFullLifeTime);
        }
    }

    let mut state = unsafe { std::mem::zeroed::<SYSTEM_BATTERY_STATE>() };
    let status = unsafe {
        CallNtPowerInformation(
            SystemBatteryState,
            None,
            0,
            Some(&mut state as *mut _ as *mut core::ffi::c_void),
            std::mem::size_of::<SYSTEM_BATTERY_STATE>() as u32,
        )
    };

    if status.0 == 0 {
        out.present = state.BatteryPresent.as_bool();
        if !out.charging {
            out.charging = state.Charging.as_bool();
        }
        if !out.ac_online {
            out.ac_online = state.AcOnLine.as_bool();
        }
        out.rate_mw = sane_u32(state.Rate);
        out.remaining_mwh = sane_u32(state.RemainingCapacity);
        out.full_mwh = sane_u32(state.MaxCapacity);
        if out.percent.is_none() && state.MaxCapacity > 0 && state.RemainingCapacity > 0 {
            let pct = (state.RemainingCapacity as u64 * 100) / state.MaxCapacity as u64;
            out.percent = Some(pct.min(100) as u8);
        }
        if out.secs_left.is_none() && !out.charging {
            out.secs_left = sane_u32(state.EstimatedTime);
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Design capacity / cycle count.
//
// There is no friendly API for these; they only exist on the battery device
// itself. This is best-effort: if anything fails we simply return None and the
// status screen omits the row.
// ---------------------------------------------------------------------------

const FILE_DEVICE_BATTERY: u32 = 0x29;

const fn ctl_code(device: u32, function: u32, method: u32, access: u32) -> u32 {
    (device << 16) | (access << 14) | (function << 2) | method
}

const IOCTL_BATTERY_QUERY_TAG: u32 = ctl_code(FILE_DEVICE_BATTERY, 0x10, 0, 1);
const IOCTL_BATTERY_QUERY_INFORMATION: u32 = ctl_code(FILE_DEVICE_BATTERY, 0x11, 0, 1);

const BATTERY_INFORMATION_LEVEL: u32 = 0; // BatteryInformation

#[repr(C)]
struct BatteryQueryInformation {
    battery_tag: u32,
    information_level: u32,
    at_rate: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct BatteryInformation {
    capabilities: u32,
    technology: u8,
    reserved: [u8; 3],
    chemistry: [u8; 4],
    designed_capacity: u32,
    full_charged_capacity: u32,
    default_alert1: u32,
    default_alert2: u32,
    critical_bias: u32,
    cycle_count: u32,
}

struct Device(HANDLE);

impl Drop for Device {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

fn open_battery() -> Option<Device> {
    for n in 1..=4u32 {
        let mut name: Vec<u16> = format!("\\\\.\\BAT{n:04}")
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let handle = unsafe {
            CreateFileW(
                PCWSTR::from_raw(name.as_ptr()),
                GENERIC_READ_WRITE,
                FILE_SHARE_MODE(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0),
                None,
                OPEN_EXISTING,
                FILE_FLAGS_AND_ATTRIBUTES(FILE_ATTRIBUTE_NORMAL.0),
                None,
            )
        };
        name.clear();
        if let Ok(h) = handle {
            if !h.is_invalid() {
                return Some(Device(h));
            }
        }
    }
    None
}

pub fn health() -> Option<Health> {
    let dev = open_battery()?;

    // Ask the driver for the current battery tag.
    let timeout_ms: u32 = 5_000;
    let mut tag: u32 = 0;
    let mut returned: u32 = 0;
    let ok = unsafe {
        DeviceIoControl(
            dev.0,
            IOCTL_BATTERY_QUERY_TAG,
            Some(&timeout_ms as *const u32 as *const core::ffi::c_void),
            std::mem::size_of::<u32>() as u32,
            Some(&mut tag as *mut u32 as *mut core::ffi::c_void),
            std::mem::size_of::<u32>() as u32,
            Some(&mut returned),
            None,
        )
    };
    if ok.is_err() || tag == 0 {
        return None;
    }

    let query = BatteryQueryInformation {
        battery_tag: tag,
        information_level: BATTERY_INFORMATION_LEVEL,
        at_rate: 0,
    };
    let mut info = BatteryInformation::default();
    let ok = unsafe {
        DeviceIoControl(
            dev.0,
            IOCTL_BATTERY_QUERY_INFORMATION,
            Some(&query as *const _ as *const core::ffi::c_void),
            std::mem::size_of::<BatteryQueryInformation>() as u32,
            Some(&mut info as *mut _ as *mut core::ffi::c_void),
            std::mem::size_of::<BatteryInformation>() as u32,
            Some(&mut returned),
            None,
        )
    };
    if ok.is_err() || info.designed_capacity == 0 {
        return None;
    }

    Some(Health {
        designed_mwh: info.designed_capacity,
        full_mwh: info.full_charged_capacity,
        cycles: sane_u32(info.cycle_count),
    })
}
