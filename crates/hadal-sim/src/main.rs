//! HADAL simulator — headless pacing CLI.
//!
//! Runs deterministic simulation profiles for balance tuning.
//!
//! # Usage
//!
//! ```text
//! hadal-sim --profile active|idle|offline --hours 1,24,72 [--assert] [--json] [--csv] [--out <path>]
//! ```

#![forbid(unsafe_code)]

use std::fs;
use std::process;

use clap::Parser;

use hadal_sim::{
    SimProfile, assert_bands, check_clock_skew, check_online_offline_equivalence, print_bands,
    print_result, to_csv, to_json,
};

#[derive(Parser, Debug)]
#[command(name = "hadal-sim", version, about = "HADAL headless pacing simulator")]
struct Cli {
    /// Simulation profile.
    #[arg(long, default_value = "active")]
    profile: SimProfile,

    /// Hours to simulate (comma-separated list). `0` exits with empty result.
    #[arg(long, default_value = "1,24,72")]
    hours: String,

    /// Assert pacing bands and exit non-zero on failure.
    #[arg(long)]
    assert: bool,

    /// Output results as JSON.
    #[arg(long)]
    json: bool,

    /// Output results as CSV.
    #[arg(long)]
    csv: bool,

    /// Write output to file (stdout if omitted).
    #[arg(long)]
    out: Option<String>,
}

fn main() {
    let cli = Cli::parse();

    // Parse hours
    let hours: Vec<u64> = if cli.hours == "0" {
        Vec::new()
    } else {
        cli.hours
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|s| {
                s.parse().unwrap_or_else(|_| {
                    eprintln!("invalid hour value: {s}");
                    process::exit(2);
                })
            })
            .collect()
    };

    // --hours 0: exit 0 with empty result
    if hours.is_empty() {
        if cli.assert {
            // No bands to check → all pass
            print_bands(&[]);
        } else if cli.json {
            println!("[]");
        } else if cli.csv {
            println!(
                "profile,hours,elapsed_secs,echo,rate,peak,descents,era_fathoms,hulls_owned,depth_m,distinct_generators_owned,unowned_hulls,unowned_upgrades,anomaly"
            );
        } else {
            println!(
                "HADAL sim v{} — no hours specified",
                env!("CARGO_PKG_VERSION")
            );
        }
        return;
    }

    // Run simulation
    let results = hadal_sim::run(cli.profile, &hours);

    // Assert bands
    let mut all_bands = Vec::new();
    let mut assert_failed = false;

    if cli.assert {
        let bands = assert_bands(&results);
        let mut failed = bands.iter().filter(|b| !b.pass).count();

        // Clock skew bands
        let skew_bands = check_clock_skew();
        failed += skew_bands.iter().filter(|b| !b.pass).count();

        // Online/offline equivalence
        let eq_band = check_online_offline_equivalence();
        if !eq_band.pass {
            failed += 1;
        }

        all_bands.extend(bands);
        all_bands.extend(skew_bands);
        all_bands.push(eq_band);

        print_bands(&all_bands);

        if failed > 0 {
            assert_failed = true;
        }
    }

    // Output results
    let output = if cli.json {
        to_json(&results)
    } else if cli.csv {
        to_csv(&results)
    } else {
        // Print per-result metrics
        let mut buf = String::new();
        for r in &results {
            print_result(r);
            buf.push_str(&format!("\n{} {} h\n", r.profile, r.hours));
        }
        buf
    };

    // Write output
    match &cli.out {
        Some(path) => {
            if let Err(e) = fs::write(path, output) {
                eprintln!("failed to write {}: {}", path, e);
                process::exit(2);
            }
        }
        None => {
            print!("{}", output);
        }
    }

    if assert_failed {
        process::exit(1);
    }
}
