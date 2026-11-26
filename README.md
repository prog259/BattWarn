# battwarn

A tiny battery watchdog for **Windows 11**. It sits in the background, and when
your battery drops to **20%** it puts an always-on-top popup in front of you that
you have to click OK on:

```
┌─ battwarn — battery low ────────────────────────┐
│  ⚠                                              │
│  Battery at 18%.                                │
│                                                 │
│  Windows can shut down without warning from      │
│  here on.                                       │
│  Save your work and plug in the charger.        │
│                                                 │
│                       [ OK ]                    │
└─────────────────────────────────────────────────┘
```

If you dismiss it and keep working, it comes back at 15%, 10% and 5%. It never
nags while you are plugged in.

## Install

**Setup exe**

```
battwarn-setup.exe            # double-click, or:
battwarn-setup.exe /S         # silent, no UI
battwarn-setup.exe /S /D=C:\tools\battwarn
```

It installs for the current user only — no administrator prompt — and starts
watching immediately.

**Or just the bare exe**

```
battwarn install
```

That copies the exe to `%LOCALAPPDATA%\Programs\battwarn`, puts it on your user
PATH, registers it to start at logon, and starts the watcher. The single exe is
the whole installer; the setup exe just wraps this.

## Commands

That is the whole interface — five verbs, and no arguments means "tell me
everything".

| Command | What it does |
|---|---|
| `battwarn` | Battery status. No arguments needed. |
| `battwarn watch` | Watch in this window (good for trying it out). |
| `battwarn install` | Install, autostart at logon, start watching. |
| `battwarn uninstall` | Stop watching, remove autostart, PATH and files. |
| `battwarn test` | Show the warning popup right now. |

## Status screen

```
  battwarn  1.0.0 · battery

  charge      62%  [████████████░░░░░░░░]  discharging
  power       battery · 9.8 W draw
  remaining   1h 47m
  capacity    47.2 Wh of 48.9 Wh full
  health      83%  47.2 Wh / 57.0 Wh design · 214 cycles
  ────────────────────────────────────────────────────
  warn at     20%  then every 5% down
  watchdog    running
  installed   C:\Users\you\AppData\Local\Programs\battwarn
```

Rows disappear when the hardware doesn't report them, rather than showing
invented numbers.

## Options

| Flag | Default | Meaning |
|---|---|---|
| `-t, --threshold <pct>` | `20` | Warn at or below this percentage |
| `-s, --step <pct>` | `5` | Warn again every N% below the threshold |
| `-i, --interval <secs>` | `20` | Poll interval (drops to 5s near the threshold) |
| `--daemon` | off | Run with no console window (used at logon) |
| `-q, --quiet` | off | No console output |
| `--keep-files` | off | With `uninstall`: leave the exe in place |

Options stick: `battwarn install -t 25` rewrites the autostart entry, and
`battwarn` reads the setting back so the status screen always shows the truth.

## How it decides

```
on AC or charging        -> never warn, fully re-arm
above the threshold      -> never warn, re-arm
at/below the threshold   -> warn once, then again every `step` percent down
```

Two extra guards:

* **No stacking.** While a popup is still on screen, new levels are held rather
  than dropped, so a fast drop can't silently skip a warning.
* **No duplicates.** The same percentage never warns twice in a row.

The decision logic lives in `src/engine.rs`, has no Windows imports, and is
verified by `tools/engine_check.rs` (run by `build.sh`).

## Where things live

| Thing | Path |
|---|---|
| Program | `%LOCALAPPDATA%\Programs\battwarn\battwarn.exe` |
| Log | `%LOCALAPPDATA%\battwarn\battwarn.log` |
| Autostart | `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` → `battwarn` |
| PATH entry | `HKCU\Environment` → `Path` |
| Uninstall entry | `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\battwarn` |

Nothing is written outside your user profile. No services, no admin, no
telemetry, no network access.

## Uninstall

```
battwarn uninstall
```

or Settings → Apps → **battwarn** → Uninstall, or run
`%LOCALAPPDATA%\Programs\battwarn\uninstall.exe`.

The running watcher is asked to stop first; the exe removes itself via a
detached `cmd` after the process exits.

## Troubleshooting

**"Windows protected your PC"** — the exe is unsigned. Click *More info* →
*Run anyway*. Only a code-signing certificate removes that prompt.

**Nothing pops up when I run `battwarn test`** — check the log:
`%LOCALAPPDATA%\battwarn\battwarn.log`. If the file doesn't exist, the watcher
isn't running; `battwarn` will say so on the `watchdog` line.

**"battwarn is already watching this machine."** — a second copy is running.
That is intentional: two watchers would mean two popups. `battwarn uninstall`
then `battwarn install` restarts it cleanly.

**`battwarn` isn't found in my terminal** — the PATH change only applies to
terminals opened after install. Open a new one.

**The popup didn't appear over a fullscreen game** — modal message boxes are
raised above normal windows and most fullscreen ones, but exclusive-fullscreen
games can still hide them. Play windowed if you need the warning.

## The real problem may be battery wear

"Shuts off instantly at 20%" is usually a worn pack whose voltage collapses
under load — Windows' percentage is guessing. Two things help:

1. Raise the threshold: `battwarn install -t 30`
2. Check the actual wear:

   ```
   powercfg /batteryreport
   ```

   Open the HTML report and compare **DESIGN CAPACITY** with **FULL CHARGE
   CAPACITY**. If full charge is far below design, the pack is done and no
   threshold will fully save you.

## Building from source

```
./build.sh
```

Cross-compiles to `x86_64-pc-windows-msvc` with `cargo-xwin` and packages the
installer with `makensis`. On Windows, plain `cargo build --release` works and
`makensis installer\battwarn.nsi` builds the setup exe.

Only one dependency: the `windows` crate.
