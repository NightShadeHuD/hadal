//! Release-mode pacing acceptance test (plan §9.2).
//!
//! This test drives the headless simulation policies from `hadal-sim` and
//! asserts every pacing band.  It is deliberately loud: a regression in any
//! band fails the test with the measured value and the required range.
//!
//! Run with:
//! ```text
//! cargo test -p hadal-core --release --test pacing -- --nocapture
//! ```

use hadal_sim::{
    Band, SimProfile, SimResult, assert_bands, check_clock_skew, check_online_offline_equivalence,
    run,
};

fn collect(reports: &mut Vec<String>, bands: &[Band]) {
    for b in bands {
        let status = if b.pass { "PASS" } else { "FAIL" };
        reports.push(format!(
            "  [{status}] {:<42} {}  (required {})",
            b.name, b.measured, b.required
        ));
    }
}

fn assert_profile(reports: &mut Vec<String>, profile: SimProfile, hours: &[u64]) -> Vec<Band> {
    let results: Vec<SimResult> = run(profile, hours);
    let bands = assert_bands(&results);
    collect(reports, &bands);
    bands
}

#[test]
fn pacing_all_bands_met() {
    let mut reports: Vec<String> = Vec::new();
    let mut failed: Vec<String> = Vec::new();

    let mut check = |bands: Vec<Band>| {
        for b in &bands {
            if !b.pass {
                failed.push(format!("{}: measured {}", b.name, b.measured));
            }
        }
        bands
    };

    // Active: 1 h / 24 h / 72 h bands.
    let active = assert_profile(&mut reports, SimProfile::Active, &[1, 24, 72]);
    check(active);

    // Idle: first-descent availability.
    let idle = assert_profile(&mut reports, SimProfile::Idle, &[1]);
    check(idle);

    // Offline: 8 h fresh cap + 72 h midgame.
    let offline = assert_profile(&mut reports, SimProfile::Offline, &[8, 72]);
    check(offline);

    // Clock skew and online/offline equivalence are profile-agnostic.
    let skew = check_clock_skew();
    collect(&mut reports, &skew);
    check(skew);

    let equiv = check_online_offline_equivalence();
    collect(&mut reports, std::slice::from_ref(&equiv));
    check(vec![equiv]);

    println!("\n==== PACING BANDS ====");
    for line in &reports {
        println!("{line}");
    }
    println!("======================\n");

    if !failed.is_empty() {
        panic!(
            "{} pacing band(s) failed:\n{}",
            failed.len(),
            failed.join("\n")
        );
    }
}
