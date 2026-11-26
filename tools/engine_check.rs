// Standalone harness for the warning state machine. Runs on any platform:
//
//     rustc -O tools/engine_check.rs -o /tmp/engine_check && /tmp/engine_check

#[allow(dead_code)]
mod engine {
    include!("../src/engine.rs");
}

use engine::{Engine, Level, Warn};

fn discharge(threshold: u8, step: u8) -> Vec<(u8, Level)> {
    let mut e = Engine::new(threshold, step);
    let mut fired = Vec::new();
    for percent in (0..=100).rev() {
        if let Some(Warn { percent, level }) = e.on_sample(percent, false, false) {
            fired.push((percent, level));
        }
    }
    fired
}

fn main() {
    let mut failures = 0;

    let mut check = |name: &str, ok: bool, detail: String| {
        if ok {
            println!("  ok    {name}");
        } else {
            println!("  FAIL  {name}  -> {detail}");
            failures += 1;
        }
    };

    let fired = discharge(20, 5);
    let levels: Vec<u8> = fired.iter().map(|(p, _)| *p).collect();
    check(
        "20/5 discharges warn at 20,15,10,5",
        levels == vec![20, 15, 10, 5],
        format!("{levels:?}"),
    );
    check(
        "levels escalate low -> critical -> final",
        fired.iter().map(|(_, l)| *l).collect::<Vec<_>>()
            == vec![Level::Low, Level::Low, Level::Critical, Level::Final],
        format!("{fired:?}"),
    );

    let fired = discharge(30, 10);
    let levels: Vec<u8> = fired.iter().map(|(p, _)| *p).collect();
    check(
        "30/10 discharges warn at 30,20,10",
        levels == vec![30, 20, 10],
        format!("{levels:?}"),
    );

    // Charging must silence everything.
    let mut e = Engine::new(20, 5);
    let mut quiet = true;
    for percent in (0..=25).rev() {
        if e.on_sample(percent, true, true).is_some() {
            quiet = false;
        }
    }
    check("charging never warns", quiet, "warned while charging".into());

    // Plugging in re-arms, so unplugging at the same level warns again.
    let mut e = Engine::new(20, 5);
    let a = e.on_sample(12, false, false).is_some();
    let b = e.on_sample(12, true, false).is_some();
    let c = e.on_sample(12, false, false).is_some();
    check(
        "ac re-arms the warning",
        a && !b && c,
        format!("first={a} on_ac={b} again={c}"),
    );

    // A sudden drop past several levels only warns once, at the real value.
    let mut e = Engine::new(20, 5);
    let w = e.on_sample(4, false, false);
    check(
        "sudden drop warns at the observed percentage",
        w == Some(Warn { percent: 4, level: Level::Final }),
        format!("{w:?}"),
    );

    // Never warn twice for the same percentage.
    let mut e = Engine::new(20, 5);
    let first = e.on_sample(17, false, false).is_some();
    let second = e.on_sample(17, false, false).is_some();
    check("no duplicate warnings", first && !second, format!("{first}/{second}"));

    println!();
    if failures == 0 {
        println!("all engine checks passed");
    } else {
        println!("{failures} check(s) failed");
        std::process::exit(1);
    }
}
