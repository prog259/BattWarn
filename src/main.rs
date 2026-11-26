mod engine;
mod install;
mod notify;
mod power;
mod ui;
mod watch;

use std::process::ExitCode;

use engine::Level;
use ui::Ui;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug)]
pub struct Config {
    pub threshold: u8,
    pub step: u8,
    pub interval: u64,
    pub daemon: bool,
    pub quiet: bool,
    pub keep_files: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            threshold: 20,
            step: 5,
            interval: 20,
            daemon: false,
            quiet: false,
            keep_files: false,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cmd {
    Status,
    Watch,
    Install,
    Uninstall,
    Test,
    Help,
    Version,
}

fn help() -> String {
    format!(
        "\
battwarn {VERSION} \u{2014} battery watchdog for Windows 11

USAGE
  battwarn                    show battery status
  battwarn watch              watch the battery in this window
  battwarn install            install, start at logon, start watching
  battwarn uninstall          stop watching and remove everything
  battwarn test               show the warning popup right now

OPTIONS
  -t, --threshold <pct>       warn at this percentage      (default 20)
  -s, --step <pct>            warn again every N% below   (default 5)
  -i, --interval <secs>       poll interval                (default 20)
      --daemon                run hidden (used at logon)
  -q, --quiet                 no console output
      --keep-files            with uninstall: leave the exe in place
  -h, --help                  this text
  -V, --version               version

EXAMPLES
  battwarn install -t 25      install and warn a bit earlier
  battwarn watch              try it in a terminal before installing
  battwarn                    what is the battery doing right now?
"
    )
}

fn parse(args: &[String]) -> Result<(Cmd, Config), String> {
    let mut cfg = Config::default();
    let mut cmd: Option<Cmd> = None;
    let mut i = 0;

    let take = |i: &mut usize, flag: &str| -> Result<String, String> {
        *i += 1;
        args.get(*i)
            .cloned()
            .ok_or_else(|| format!("{flag} needs a value"))
    };

    while i < args.len() {
        let arg = args[i].clone();
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) => (f.to_string(), Some(v.to_string())),
            None => (arg.clone(), None),
        };

        match flag.as_str() {
            "-h" | "--help" | "help" => {
                cmd = Some(Cmd::Help);
            }
            "-V" | "--version" | "version" => {
                cmd = Some(Cmd::Version);
            }
            "-t" | "--threshold" => {
                let v = match inline {
                    Some(v) => v,
                    None => take(&mut i, "--threshold")?,
                };
                let n: u8 = v.parse().map_err(|_| format!("bad threshold: {v}"))?;
                if !(5..=95).contains(&n) {
                    return Err("threshold must be between 5 and 95".into());
                }
                cfg.threshold = n;
            }
            "-s" | "--step" => {
                let v = match inline {
                    Some(v) => v,
                    None => take(&mut i, "--step")?,
                };
                let n: u8 = v.parse().map_err(|_| format!("bad step: {v}"))?;
                if n == 0 {
                    return Err("step must be at least 1".into());
                }
                cfg.step = n;
            }
            "-i" | "--interval" => {
                let v = match inline {
                    Some(v) => v,
                    None => take(&mut i, "--interval")?,
                };
                let n: u64 = v.parse().map_err(|_| format!("bad interval: {v}"))?;
                cfg.interval = n.clamp(2, 3600);
            }
            "--daemon" => cfg.daemon = true,
            "-q" | "--quiet" => cfg.quiet = true,
            "--keep-files" => cfg.keep_files = true,
            other if other.starts_with('-') => {
                return Err(format!("unknown option: {other}"));
            }
            other => {
                let c = match other {
                    "status" => Cmd::Status,
                    "watch" => Cmd::Watch,
                    "install" => Cmd::Install,
                    "uninstall" => Cmd::Uninstall,
                    "test" => Cmd::Test,
                    _ => return Err(format!("unknown command: {other}")),
                };
                if cmd.is_some() {
                    return Err("only one command at a time".into());
                }
                cmd = Some(c);
            }
        }
        i += 1;
    }

    Ok((cmd.unwrap_or(Cmd::Status), cfg))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let (cmd, cfg) = match parse(&args) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("battwarn: {e}");
            eprintln!("try `battwarn --help`");
            return ExitCode::from(2);
        }
    };

    match cmd {
        Cmd::Help => {
            print!("{}", help());
            ExitCode::SUCCESS
        }
        Cmd::Version => {
            println!("battwarn {VERSION}");
            ExitCode::SUCCESS
        }
        Cmd::Status => {
            status(&cfg);
            ExitCode::SUCCESS
        }
        Cmd::Watch => ExitCode::from(watch::run(&cfg)),
        Cmd::Test => {
            test(&cfg);
            ExitCode::SUCCESS
        }
        Cmd::Install => match install::install(&cfg) {
            Ok(()) => {
                if !cfg.quiet && ui::launched_from_explorer() {
                    ui::wait_for_enter();
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("battwarn: {e}");
                ExitCode::from(1)
            }
        },
        Cmd::Uninstall => match install::uninstall(&cfg) {
            Ok(()) => {
                if !cfg.quiet {
                    println!("battwarn uninstalled.");
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("battwarn: {e}");
                ExitCode::from(1)
            }
        },
    }
}

fn test(cfg: &Config) {
    let sample = power::sample();
    let percent = sample.percent.unwrap_or(18);
    let level = if percent <= 5 {
        Level::Final
    } else if percent <= 10 {
        Level::Critical
    } else {
        Level::Low
    };

    if !cfg.quiet {
        println!("battwarn {VERSION} \u{00b7} test popup at {percent}%");
        println!("this is exactly what you will see when the battery gets low.\n");
    }
    notify::show_blocking(percent, level);
    if !cfg.quiet {
        println!("dismissed.");
    }
}

fn status(cfg: &Config) {
    let ui = Ui::init();
    let sample = power::sample();
    let auto = install::autostart_config();
    let installed = install::installed_exe().exists();
    let running = install::is_running();

    let threshold = auto.as_ref().map(|c| c.threshold).unwrap_or(cfg.threshold);
    let step = auto.as_ref().map(|c| c.step).unwrap_or(cfg.step);

    println!();
    println!("  {}  {}", ui.bold("battwarn"), ui.dim(&format!("{VERSION} \u{00b7} battery")));
    println!();

    if !sample.present {
        println!("  {}", ui.dim("no battery detected on this machine."));
        println!();
        if !cfg.quiet && ui::launched_from_explorer() {
            ui::wait_for_enter();
        }
        return;
    }

    let percent = sample.percent.unwrap_or(0);
    let state = if sample.charging {
        ui.green("charging")
    } else if sample.ac_online {
        ui.dim("on AC, not charging")
    } else if sample.critical {
        ui.red("discharging \u{00b7} critical")
    } else {
        ui.yellow("discharging")
    };

    println!(
        "  {}  {:>3}%  {}  {}",
        ui.dim(&format!("{:<10}", "charge")),
        percent,
        ui.bar(percent, 20),
        state
    );

    if let Some(rate) = sample.rate_mw {
        let text = if sample.charging {
            format!("AC \u{00b7} {} charge", ui::watts(rate))
        } else if sample.ac_online {
            format!("AC \u{00b7} {} idle", ui::watts(rate))
        } else {
            format!("battery \u{00b7} {} draw", ui::watts(rate))
        };
        println!("  {}  {}", ui.dim(&format!("{:<10}", "power")), text);
    }

    if sample.charging {
        println!(
            "  {}  {}",
            ui.dim(&format!("{:<10}", "full in")),
            ui::duration(sample.secs_to_full)
        );
    } else {
        println!(
            "  {}  {}",
            ui.dim(&format!("{:<10}", "remaining")),
            ui::duration(sample.secs_left)
        );
    }

    if let (Some(remaining), Some(full)) = (sample.remaining_mwh, sample.full_mwh) {
        println!(
            "  {}  {} of {} full",
            ui.dim(&format!("{:<10}", "capacity")),
            ui::wh(remaining),
            ui::wh(full)
        );
    }

    if let Some(health) = power::health() {
        let pct = health.percent();
        let paint = if pct >= 80 {
            ui.green(&format!("{pct}%"))
        } else if pct >= 60 {
            ui.yellow(&format!("{pct}%"))
        } else {
            ui.red(&format!("{pct}%"))
        };
        let cycles = health
            .cycles
            .map(|c| format!(" \u{00b7} {c} cycles"))
            .unwrap_or_default();
        println!(
            "  {}  {}  {} / {} design{}",
            ui.dim(&format!("{:<10}", "health")),
            paint,
            ui::wh(health.full_mwh),
            ui::wh(health.designed_mwh),
            cycles
        );
    }

    if sample.saver {
        println!("  {}  {}", ui.dim(&format!("{:<10}", "saver")), ui.yellow("on"));
    }

    println!("  {}", ui.rule(52));

    match &auto {
        Some(_) => println!(
            "  {}  {}%  {}",
            ui.dim(&format!("{:<10}", "warn at")),
            threshold,
            ui.dim(&format!("then every {step}% down"))
        ),
        None => println!(
            "  {}  {}",
            ui.dim(&format!("{:<10}", "warn at")),
            ui.dim("not configured")
        ),
    }

    let watchdog = if running {
        ui.green("running")
    } else if installed {
        ui.yellow("installed, not running \u{00b7} run \"battwarn install\"")
    } else {
        ui.dim("not installed \u{00b7} run \"battwarn install\"")
    };
    println!("  {}  {}", ui.dim(&format!("{:<10}", "watchdog")), watchdog);

    if installed {
        println!(
            "  {}  {}",
            ui.dim(&format!("{:<10}", "installed")),
            ui.dim(&install::installed_exe().display().to_string())
        );
    }

    println!();
    if !cfg.quiet && ui::launched_from_explorer() {
        ui::wait_for_enter();
    }
}
