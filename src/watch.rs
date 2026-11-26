//! The polling loop.

use std::io::Write;

use crate::engine::{Engine, Level, Warn};
use crate::install::{self, InstanceGuard, StopEvent};
use crate::notify::Warner;
use crate::power;
use crate::Config;

fn timestamp() -> String {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
    )
}

fn log(msg: &str) {
    let path = install::log_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > 1_048_576 {
            let _ = std::fs::rename(&path, path.with_extension("log.1"));
        }
    }
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "[{}] {}", timestamp(), msg);
    }
}

fn level_name(level: Level) -> &'static str {
    match level {
        Level::Low => "low",
        Level::Critical => "critical",
        Level::Final => "final",
    }
}

pub fn run(cfg: &Config) -> u8 {
    if cfg.daemon {
        install::detach_console();
    }

    let Some(_guard) = InstanceGuard::acquire() else {
        if !cfg.quiet {
            eprintln!("battwarn is already watching this machine.");
        }
        return 0;
    };

    let Some(stop) = StopEvent::create() else {
        if !cfg.quiet {
            eprintln!("battwarn: cannot create the stop event.");
        }
        return 1;
    };

    let first = power::sample();
    if !first.present {
        if !cfg.quiet {
            eprintln!("battwarn: no battery detected, nothing to watch.");
        }
        return 1;
    }

    if !cfg.quiet {
        println!(
            "battwarn {} \u{00b7} watching \u{00b7} warn at {}% (then every {}% down) \u{00b7} Ctrl+C to stop",
            env!("CARGO_PKG_VERSION"),
            cfg.threshold,
            cfg.step
        );
    }
    log(&format!(
        "watch started: threshold {}%, step {}%, interval {}s, pid {}",
        cfg.threshold,
        cfg.step,
        cfg.interval,
        std::process::id()
    ));

    let mut engine = Engine::new(cfg.threshold, cfg.step);
    let warner = Warner::new();

    loop {
        let sample = power::sample();

        if !sample.present {
            log("battery no longer present; exiting");
            break;
        }

        if let Some(percent) = sample.percent {
            // While a popup is still on screen, do not consume new levels;
            // otherwise a fast drop would silently skip a warning.
            if !warner.is_open() {
                if let Some(Warn { percent, level }) =
                    engine.on_sample(percent, sample.ac_online, sample.charging)
                {
                    warner.show(percent, level);
                    log(&format!(
                        "warned: {}% ({}) ac={} charging={}",
                        percent,
                        level_name(level),
                        sample.ac_online,
                        sample.charging
                    ));
                }
            }
        }

        let near = sample
            .percent
            .map(|p| p <= cfg.threshold.saturating_add(10))
            .unwrap_or(true);
        let wait = if near { cfg.interval.min(5).max(2) } else { cfg.interval };

        if stop.wait(wait) {
            log("watch stopped on request");
            break;
        }
    }

    0
}
