//! xtask — development gate, setup, and local distribution.
//!
//! Commands: `setup`, `gate`, `dist`, `checks`, `privacy`, `gate-status`,
//! `floor`, `mutate`, `--version`, `--help`.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use sha2::{Digest, Sha256};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Format SHA-256 digest as hex string.
fn hex_digest(data: &[u8]) -> String {
    data.iter().map(|b| format!("{:02x}", b)).collect()
}

fn repo_root() -> PathBuf {
    let manifest = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    PathBuf::from(manifest)
        .parent()
        .and_then(Path::parent)
        .expect("xtask should be in crates/xtask/")
        .to_path_buf()
}

fn print_usage() {
    println!("HADAL xtask v{VERSION}");
    println!();
    println!("Usage: cargo xtask <command> [options]");
    println!();
    println!("Commands:");
    println!("  setup              Install git hooks and print next steps");
    println!("  gate               Run all quality gates (fmt, clippy, test, checks, receipt)");
    println!("  gate --release-pacing  Run gate plus release pacing tests");
    println!("  dist               Build release binary and package distribution");
    println!("  dist --out <dir>   Place distribution in <dir>");
    println!("  checks             Run source checks (ignore-scan, path-scan, test-floor, no-net)");
    println!("  privacy            Scan for local paths, private patterns, and commit issues");
    println!("  gate-status        Verify the gate receipt against the current tree");
    println!("  floor <n>|--current  Set or write the test floor");
    println!("  mutate [--base <ref>]  Run cargo-mutants in-diff");
    println!("  --version          Print version");
    println!("  --help             Print this help");
}

// ── Setup ──────────────────────────────────────────────────────────────────

/// Install git hooks by copying .githooks/* to .git/hooks/*.
fn cmd_setup() -> io::Result<()> {
    let root = repo_root();
    let hooks_dir = root.join(".githooks");

    if !root.join(".git").exists() {
        eprintln!("ERROR: .git directory not found. Initialize the repository first.");
        std::process::exit(1);
    }

    let target = root.join(".git").join("hooks");
    fs::create_dir_all(&target)?;

    for entry in fs::read_dir(&hooks_dir)? {
        let entry = entry?;
        let src = entry.path();
        let dst = target.join(entry.file_name());
        let content = fs::read_to_string(&src)?;
        fs::write(&dst, &content)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&dst)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&dst, perms)?;
        }

        println!("Installed: {} -> {}", src.display(), dst.display());
    }

    println!();
    println!("Next steps:");
    println!("  1. Run `cargo xtask gate` to verify the quality pipeline.");
    println!("  2. Commit your work; pre-commit will run gate automatically.");
    println!("  3. Run `cargo xtask dist` to build a release distribution.");
    Ok(())
}

// ── Git helpers ────────────────────────────────────────────────────────────

/// Run a git command and return Ok(stdout) or Err with the stderr.
fn git_output(args: &[&str], cwd: &Path) -> io::Result<String> {
    let output = Command::new("git").args(args).current_dir(cwd).output()?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(io::Error::other(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ))
    }
}

/// Find merge-base for in-diff.
fn find_merge_base(cwd: &Path) -> io::Result<String> {
    // Try origin/main first
    if let Ok(out) = git_output(&["merge-base", "HEAD", "origin/main"], cwd) {
        return Ok(out.trim().to_owned());
    }
    // Fall back to main
    if let Ok(out) = git_output(&["merge-base", "HEAD", "main"], cwd) {
        return Ok(out.trim().to_owned());
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "no merge-base found (first push, no remote)",
    ))
}

/// Get unpushed commits: origin/main..HEAD, fallback to HEAD.
fn get_unpushed_commits(cwd: &Path) -> io::Result<Vec<String>> {
    let rev_range = if git_output(&["rev-parse", "--verify", "origin/main"], cwd).is_ok() {
        "origin/main..HEAD"
    } else {
        "HEAD"
    };
    let out = git_output(&["log", "--format=%H", rev_range], cwd)?;
    Ok(out
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.to_owned())
        .collect())
}

/// Get commit details (subject, body, author email, committer email).
fn get_commit_details(cwd: &Path, hash: &str) -> io::Result<(String, String, String, String)> {
    let subject = git_output(&["log", "-1", "--format=%s", hash], cwd)?;
    let body = git_output(&["log", "-1", "--format=%b", hash], cwd)?;
    let author_email = git_output(&["log", "-1", "--format=%ae", hash], cwd)?;
    let committer_email = git_output(&["log", "-1", "--format=%ce", hash], cwd)?;
    Ok((
        subject.trim().to_owned(),
        body.trim().to_owned(),
        author_email.trim().to_owned(),
        committer_email.trim().to_owned(),
    ))
}

// ── Gate ───────────────────────────────────────────────────────────────────

/// Enumerate the working tree the same way `privacy` does: tracked, staged, and
/// untracked-but-not-ignored, deduped and sorted. Excludes build/output dirs and
/// non-files. `.gitignore`d paths are already excluded by `--exclude-standard`.
fn working_tree_files(root: &Path) -> io::Result<Vec<String>> {
    let mut candidates: Vec<String> = Vec::new();
    for args in [
        ["ls-files", "--cached"].as_slice(),
        ["diff", "--cached", "--name-only", "--diff-filter=AM"].as_slice(),
        ["ls-files", "--others", "--exclude-standard"].as_slice(),
    ] {
        let out = git_output(args, root)?;
        for line in out.lines() {
            let path = line.trim();
            if !path.is_empty() {
                candidates.push(path.to_owned());
            }
        }
    }
    candidates.sort();
    candidates.dedup();
    candidates.retain(|p| {
        let full = root.join(p);
        full.is_file() && !is_excluded_path(p)
    });
    Ok(candidates)
}

/// Build/output directories whose contents are never part of the gate digest.
fn is_excluded_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    normalized.starts_with("target/")
        || normalized.starts_with("dist/")
        || normalized.starts_with(".git/")
        || normalized.contains("/node_modules/")
        || normalized.starts_with("node_modules/")
}

/// Digest of the working tree's file *contents* (not just git metadata), so a
/// stale receipt cannot survive edits to untracked files or new untracked files.
/// Returns `(digest, files_hashed)`.
fn tree_digest(root: &Path) -> io::Result<(String, usize)> {
    let head = git_output(&["rev-parse", "HEAD"], root)
        .map(|s| s.trim().to_owned())
        .unwrap_or_else(|_| "none".to_owned());
    let files = working_tree_files(root)?;

    let mut lines = String::new();
    for path in &files {
        let bytes = fs::read(root.join(path)).unwrap_or_default();
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        lines.push_str(path);
        lines.push('\0');
        lines.push_str(&hex_digest(&hasher.finalize()));
        lines.push('\n');
    }

    let canonical = format!("head={head}\nfiles={}\n{lines}", files.len());
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    Ok((hex_digest(&hasher.finalize()), files.len()))
}

/// Write gate receipt to target/gate-receipt.
fn write_receipt(test_count: usize, floor: usize, cwd: &Path) -> io::Result<()> {
    fs::create_dir_all(cwd.join("target"))?;
    let receipt_path = cwd.join("target").join("gate-receipt");

    let (digest, files) = tree_digest(cwd)?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time before epoch");
    let at = chrono_rfc3339(now.as_secs());

    let content = format!(
        "version=2\nat={at}\ndigest={digest}\nfiles={files}\ntests={test_count}\nfloor={floor}\n",
    );
    fs::write(&receipt_path, content)?;
    Ok(())
}

/// Simple RFC 3339 timestamp from seconds since epoch (no chrono dependency).
fn chrono_rfc3339(secs: u64) -> String {
    // Approximate: days since epoch, then hours/mins/secs
    let days = secs / 86400;
    let rem = secs % 86400;
    let hours = rem / 3600;
    let mins = (rem % 3600) / 60;
    let seconds = rem % 60;

    // Simple epoch calculation: 1970-01-01
    let mut year = 1970u32;
    let mut remaining_days = days;
    loop {
        let leap =
            (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
        let year_days = if leap { 366 } else { 365 };
        if remaining_days < year_days {
            break;
        }
        remaining_days -= year_days;
        year += 1;
    }

    let leap = (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
    let mut mdays = [31u64, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if leap {
        mdays[1] = 29;
    }
    let mut month = 1u32;
    for (i, &d) in mdays.iter().enumerate() {
        if remaining_days < d {
            month = i as u32 + 1;
            break;
        }
        remaining_days -= d;
    }
    let day = remaining_days + 1;

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, mins, seconds
    )
}

/// Run all quality gates sequentially, aborting on first failure.
fn run_gate(release_pacing: bool) -> io::Result<()> {
    let root = repo_root();

    // Delete any existing receipt before a new run
    let receipt_path = root.join("target").join("gate-receipt");
    let _ = fs::remove_file(&receipt_path);

    // Step 1: fmt
    print!("  cargo fmt --all --check... ");
    io::stdout().flush()?;
    let status = Command::new("cargo")
        .args(["fmt", "--all", "--check"])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;
    if !status.success() {
        eprintln!("\nGATE FAILED: cargo fmt --all --check");
        std::process::exit(1);
    }
    println!("OK");

    // Step 2: clippy
    print!("  cargo clippy --workspace --all-targets --all-features -- -D warnings... ");
    io::stdout().flush()?;
    let status = Command::new("cargo")
        .args([
            "clippy",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--",
            "-D",
            "warnings",
        ])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;
    if !status.success() {
        eprintln!(
            "\nGATE FAILED: cargo clippy --workspace --all-targets --all-features -- -D warnings"
        );
        std::process::exit(1);
    }
    println!("OK");

    // Step 3: tests
    print!("  cargo test --workspace --all-features... ");
    io::stdout().flush()?;
    let status = Command::new("cargo")
        .args(["test", "--workspace", "--all-features"])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;
    if !status.success() {
        eprintln!("\nGATE FAILED: cargo test --workspace --all-features");
        std::process::exit(1);
    }
    println!("OK");

    // Step 4: checks (ignore scan, path scan, test floor, no-net)
    run_checks(&root)?;

    // Step 5: release pacing (optional)
    if release_pacing {
        let pacing_steps = [
            (
                "cargo test -p hadal-core --release --test pacing -- --nocapture",
                vec![
                    "test",
                    "-p",
                    "hadal-core",
                    "--release",
                    "--test",
                    "pacing",
                    "--",
                    "--nocapture",
                ],
            ),
            (
                "cargo run -p hadal-sim --release -- --profile active --hours 1,24,72 --assert",
                vec![
                    "run",
                    "-p",
                    "hadal-sim",
                    "--release",
                    "--",
                    "--profile",
                    "active",
                    "--hours",
                    "1,24,72",
                    "--assert",
                ],
            ),
        ];

        for (desc, args) in pacing_steps {
            print!("  {desc}... ");
            io::stdout().flush()?;
            let status = Command::new("cargo")
                .args(&args)
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .status()?;
            if !status.success() {
                eprintln!("\nGATE FAILED: {desc}");
                std::process::exit(1);
            }
            println!("OK");
        }
    }

    // Step 6: write receipt
    let test_count = count_tests(&root)?;
    let floor = read_floor(&root);
    write_receipt(test_count, floor, &root)?;

    println!("\nAll gates passed.");
    Ok(())
}

// ── Checks ─────────────────────────────────────────────────────────────────

/// Run all source checks.
fn run_checks(root: &Path) -> io::Result<()> {
    let mut ignore_hits: Vec<(String, usize, String)> = Vec::new();
    let mut path_hits: Vec<(String, usize, String)> = Vec::new();
    let mut net_hits: Vec<(String, usize, String)> = Vec::new();

    // a) Ignore scan: walk crate source files
    for entry in glob_crate_sources(root)? {
        if is_xtask_harness(&entry) {
            continue;
        }
        let content = fs::read_to_string(&entry)?;
        for (line_num, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.contains("#[ignore]") && !trimmed.contains("gate-allow: ignore") {
                ignore_hits.push((
                    entry.to_string_lossy().into_owned(),
                    line_num + 1,
                    trimmed.to_owned(),
                ));
            }
        }
    }

    // b) Path-literal scan: same files
    let path_patterns = [
        r"C:\Users\",
        r"C:/Users/",
        "/Users/",
        "/home/",
        "192.168.",
        "10.",
        "172.",
    ];
    for entry in glob_crate_sources(root)? {
        if is_xtask_harness(&entry) {
            continue;
        }
        let content = fs::read_to_string(&entry)?;
        for (line_num, line) in content.lines().enumerate() {
            if line.contains("path-literal:") {
                continue;
            }
            for pattern in &path_patterns {
                if pattern_line_hit(line, pattern) {
                    path_hits.push((
                        entry.to_string_lossy().into_owned(),
                        line_num + 1,
                        truncate(line, 120),
                    ));
                    break;
                }
            }
        }
    }

    // c) No-network source check
    let net_patterns = ["std::net", "TcpStream", "reqwest", "ureq", "fetch("];
    for entry in glob_crate_sources(root)? {
        if is_xtask_harness(&entry) {
            continue;
        }
        let content = fs::read_to_string(&entry)?;
        for (line_num, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("#[cfg(test)]") {
                continue; // skip cfg(test) lines
            }
            for pattern in &net_patterns {
                if line.contains(pattern) {
                    net_hits.push((
                        entry.to_string_lossy().into_owned(),
                        line_num + 1,
                        truncate(line, 120),
                    ));
                    break;
                }
            }
        }
    }

    // d) Test floor
    let actual = count_tests(root)?;
    let floor = read_floor(root);

    // Print summary
    println!("  checks:");
    if ignore_hits.is_empty() {
        println!("    ignore-scan: clean");
    } else {
        println!("    ignore-scan: FAILED");
        for (file, line, text) in &ignore_hits {
            println!("      {file}:{line}: {text}");
        }
    }

    if path_hits.is_empty() {
        println!("    path-scan: clean");
    } else {
        println!("    path-scan: FAILED");
        for (file, line, text) in &path_hits {
            println!("      {file}:{line}: {text}");
        }
    }

    if net_hits.is_empty() {
        println!("    no-network: clean");
    } else {
        println!("    no-network: FAILED");
        for (file, line, text) in &net_hits {
            println!("      {file}:{line}: {text}");
        }
    }

    println!("    test-floor: {actual} (floor: {floor})");
    if actual < floor {
        println!("      FAIL: actual < floor");
    } else if actual > floor {
        println!(
            "      WARNING: test floor {floor} is stale; raise with: cargo xtask floor {actual}"
        );
    }

    // Fail on any hit
    if !ignore_hits.is_empty() || !path_hits.is_empty() || !net_hits.is_empty() || actual < floor {
        eprintln!("\nchecks FAILED");
        std::process::exit(1);
    }

    Ok(())
}

/// Walk all source files in crates/*/src/**/*.rs and crates/*/tests/**/*.rs.
fn glob_crate_sources(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    walk_crate_sources(root, &mut files)?;
    files.sort();
    Ok(files)
}

fn walk_crate_sources(dir: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk_crate_sources(&path, files)?;
        } else if path.extension().is_some_and(|e| e == "rs") {
            files.push(path);
        }
    }
    Ok(())
}

/// The xtask harness necessarily contains the scanner's own pattern tables,
/// so it is exempt from the source scans (it is a dev tool, not game code).
fn is_xtask_harness(path: &Path) -> bool {
    path.components()
        .any(|c| c.as_os_str().eq_ignore_ascii_case("xtask"))
}

/// A path pattern only counts as a leak when it is followed by a name-like
/// character; this keeps decimal literals such as `10.` or `10.1` from
/// tripping the scan while still catching `10.0.0.1`.
fn pattern_line_hit(line: &str, pattern: &str) -> bool {
    if !line.contains(pattern) {
        return false;
    }
    if pattern == "10." || pattern == "172." || pattern == "192.168." {
        return simple_ip_match(line, pattern);
    }
    // Home-dir patterns must be followed by a plausible user name. Single-char
    // segments such as `/home/u` or `C:/Users/u` are the conventional anonymous
    // placeholders used in tests, not a real account leaking into the repo.
    if pattern.contains("home/") || pattern.contains("Users/") || pattern.contains("Users\\") {
        let lower = line.to_lowercase();
        let needle = pattern.to_lowercase();
        return lower.match_indices(&needle).any(|(i, m)| {
            let after = &lower[i + m.len()..];
            let name_len = after
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == '.')
                .count();
            name_len >= 2
        });
    }
    true
}

/// Count tests: unit+integration + doctests.
fn count_tests(root: &Path) -> io::Result<usize> {
    let mut count = 0;

    // Unit + integration tests
    let output = Command::new("cargo")
        .args(["test", "--workspace", "--all-features", "--", "--list"])
        .current_dir(root)
        .output()?;
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if line.trim().ends_with(": test") {
                count += 1;
            }
        }
    }

    // Doctests
    let output = Command::new("cargo")
        .args([
            "test",
            "--workspace",
            "--all-features",
            "--doc",
            "--",
            "--list",
        ])
        .current_dir(root)
        .output()?;
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if line.trim().ends_with(": test") {
                count += 1;
            }
        }
    }

    Ok(count)
}

fn read_floor(root: &Path) -> usize {
    let path = root.join("tools").join("test-floor.txt");
    if !path.exists() {
        return 0;
    }
    let content = fs::read_to_string(&path).unwrap_or_default();
    content.trim().parse().unwrap_or(0)
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_owned()
    } else {
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}

// ── Privacy helpers ────────────────────────────────────────────────────────

/// Check if a line contains a case-insensitive substring.
fn contains_ci(line: &str, needle: &str) -> bool {
    line.to_lowercase().contains(&needle.to_lowercase())
}

/// Simple pattern matching for privacy patterns (no regex dependency).
/// Supports literals, `\b` word boundaries, `[...]` classes with optional `?`
/// quantifiers, and `\b`-free optional groups of the form `[ -]?` used by the
/// owner's private list (e.g. `mail[ -]?order[ -]?monsters`).
fn matches_pattern(line: &str, pattern: &str) -> bool {
    // Whole-word form: \bword\b
    if pattern.starts_with("\\b") && pattern.ends_with("\\b") && pattern.len() > 4 {
        let inner = &pattern[2..pattern.len() - 2];
        let lower = line.to_lowercase();
        let words: Vec<&str> = lower
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .collect();
        return words.iter().any(|w| *w == inner.to_lowercase());
    }

    // Piecewise matcher: literal runs plus `[...]` (optionally `?`) groups.
    // Every literal run must appear in order; a class run must be satisfied.
    let lower = line.to_lowercase();
    let mut rest: &str = &lower;
    let lowered_pat = pattern.to_lowercase();
    let mut pat: &str = &lowered_pat;
    while !pat.is_empty() {
        if let Some(open) = pat.find('[') {
            let literal = &pat[..open];
            if !literal.is_empty() {
                match rest.find(literal) {
                    Some(i) => rest = &rest[i + literal.len()..],
                    None => return false,
                }
            }
            let Some(close_rel) = pat[open..].find(']') else {
                // Unterminated class: treat the remainder as a literal.
                return rest.contains(&pat[open..]);
            };
            let close = open + close_rel;
            let class = &pat[open + 1..close];
            let mut after = &pat[close + 1..];
            let optional = after.starts_with('?');
            if optional {
                after = &after[1..];
            }
            if optional {
                // Consume one class char if present, then continue; if absent,
                // also continue (the group is optional).
                if let Some(c) = rest.chars().next()
                    && class_contains(class, c)
                {
                    rest = &rest[c.len_utf8()..];
                }
            } else {
                let Some(c) = rest.chars().next() else {
                    return false;
                };
                if !class_contains(class, c) {
                    return false;
                }
                rest = &rest[c.len_utf8()..];
            }
            pat = after;
        } else {
            return rest.contains(pat);
        }
    }
    true
}

/// Does `class` (the interior of `[...]`) contain `c`? Handles `a-z` ranges.
fn class_contains(class: &str, c: char) -> bool {
    let chars: Vec<char> = class.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if i + 2 < chars.len() && chars[i + 1] == '-' {
            if c >= chars[i] && c <= chars[i + 2] {
                return true;
            }
            i += 3;
        } else {
            if chars[i] == c {
                return true;
            }
            i += 1;
        }
    }
    false
}

/// Simple IP address pattern match for 10.x.x.x and 172.x.x.x
fn simple_ip_match(line: &str, prefix: &str) -> bool {
    let lower = line.to_lowercase();
    if let Some(pos) = lower.find(prefix) {
        let rest = &lower[pos + prefix.len()..];
        let parts: Vec<&str> = rest.split('.').collect();
        if parts.len() >= 4 {
            // Check if first 2-3 chars after prefix are digits
            let num_part = parts[0];
            if !num_part.is_empty() && num_part.chars().all(|c| c.is_ascii_digit()) {
                return true;
            }
        }
    }
    false
}

/// Find email addresses in a line using simple parsing.
fn find_emails(line: &str) -> Vec<String> {
    let mut emails = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        if chars[i] == '@' && i > 0 {
            // Look backwards for the local part
            let mut j = i;
            while j > 0
                && (chars[j - 1].is_alphanumeric()
                    || chars[j - 1] == '.'
                    || chars[j - 1] == '_'
                    || chars[j - 1] == '%'
                    || chars[j - 1] == '+'
                    || chars[j - 1] == '-')
            {
                j -= 1;
            }
            // Look forwards for the domain part
            let mut k = i + 1;
            while k < len && (chars[k].is_alphanumeric() || chars[k] == '.' || chars[k] == '-') {
                k += 1;
            }
            if k > i + 1 {
                let email: String = chars[j..k].iter().collect();
                // Validate: at least one dot in domain, domain has 2+ chars after last dot
                if let Some(dot_pos) = email.rfind('.') {
                    let domain_part = &email[dot_pos + 1..];
                    if domain_part.len() >= 2
                        && domain_part.chars().all(|c| c.is_ascii_alphabetic())
                    {
                        emails.push(email);
                    }
                }
            }
        }
        i += 1;
    }

    emails
}

/// Check if an email domain is allowed.
fn is_allowed_email(email: &str) -> bool {
    if let Some(at_pos) = email.find('@') {
        let domain = &email[at_pos + 1..];
        // Allow @users.noreply.github.com and subdomains
        if domain.ends_with(".noreply.github.com") || domain == "users.noreply.github.com" {
            return true;
        }
        // Allow exact noreply addresses
        if domain == "noreply@anthropic.com" || domain == "noreply@github.com" {
            return true;
        }
        if domain == "anthropic.com" || domain == "github.com" {
            return true;
        }
    }
    false
}

// ── Privacy ────────────────────────────────────────────────────────────────

/// Scan for local paths, private patterns, and commit issues.
fn cmd_privacy() -> io::Result<()> {
    let root = repo_root();
    let mut hits: Vec<String> = Vec::new();

    // Local private patterns
    let local_patterns_path = root.join(".git").join("hadal-private-patterns");
    let local_patterns: Vec<String> = if local_patterns_path.exists() {
        let content = fs::read_to_string(&local_patterns_path)?;
        content
            .lines()
            .map(|l| l.trim().to_owned())
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect()
    } else {
        println!("privacy: local private-pattern list absent (generic patterns only)");
        Vec::new()
    };

    // Enumerate the working tree: tracked, staged, and untracked-but-not-ignored.
    // Before the first commit `git ls-files` alone is empty, which would make
    // the scan vacuously clean; the union keeps it meaningful on a fresh repo.
    let candidates = working_tree_files(&root)?;
    let mut file_count = 0;

    for file_path in &candidates {
        let full_path = root.join(file_path);
        if !full_path.is_file() {
            continue;
        }
        if is_xtask_harness(&full_path) {
            continue; // scanner's own pattern tables/doc examples
        }
        file_count += 1;

        let content = match fs::read_to_string(&full_path) {
            Ok(c) => c,
            Err(_) => continue, // non-UTF-8 / binary: unreadable as text
        };

        for (line_num, line) in content.lines().enumerate() {
            // Check generic patterns
            // Windows paths: X:\Users\ or X:/Users/
            // Generic path patterns (single code path to avoid duplicate blocks)
            let generic_kind: Option<&str> = if (contains_ci(line, "\\users\\")
                || contains_ci(line, "/users/")
                || line.contains("/home/"))
                && (pattern_line_hit(line, "C:\\Users\\")
                    || pattern_line_hit(line, "C:/Users/")
                    || pattern_line_hit(line, "/Users/")
                    || pattern_line_hit(line, "/home/"))
            {
                // Exclude Cargo.lock checksums
                if line.contains("checksum") {
                    None
                } else {
                    Some("home-path")
                }
            } else if line.contains("192.168.")
                || simple_ip_match(line, "10.")
                || simple_ip_match(line, "172.")
            {
                Some("private-ip")
            } else {
                None
            };

            if let Some(kind) = generic_kind {
                hits.push(format!(
                    "{file_path}:{}: generic[{kind}]: {}",
                    line_num + 1,
                    truncate(line, 120)
                ));
            }

            // Check local private patterns
            for pattern in &local_patterns {
                if matches_pattern(line, pattern) {
                    hits.push(format!(
                        "{file_path}:{}: private[{}]: {}",
                        line_num + 1,
                        pattern,
                        truncate(line, 120)
                    ));
                }
            }

            // Check email addresses
            for email in find_emails(line) {
                if !is_allowed_email(&email) {
                    hits.push(format!("{file_path}:{}: email: {email}", line_num + 1));
                }
            }
        }
    }

    // Check unpushed commits
    let commits = get_unpushed_commits(&root).unwrap_or_default();
    let mut commit_count = 0;

    for hash in &commits {
        commit_count += 1;
        match get_commit_details(&root, hash) {
            Ok((subject, body, author_email, committer_email)) => {
                // Check email
                if !is_allowed_email(&author_email) {
                    hits.push(format!(
                        "commit {hash}: author email {author_email} not allowed"
                    ));
                }
                if !is_allowed_email(&committer_email) {
                    hits.push(format!(
                        "commit {hash}: committer email {committer_email} not allowed"
                    ));
                }

                // Check subject/body against patterns
                let text_to_check = format!("{subject}\n{body}");
                for pattern in &local_patterns {
                    if matches_pattern(&text_to_check, pattern) {
                        hits.push(format!(
                            "commit {hash}: private pattern matched in commit message"
                        ));
                    }
                }
                // Check generic patterns in commit messages
                if ((contains_ci(&text_to_check, "\\users\\")
                    || contains_ci(&text_to_check, "/users/")
                    || text_to_check.contains("/home/"))
                    && (pattern_line_hit(&text_to_check, "C:\\Users\\")
                        || pattern_line_hit(&text_to_check, "C:/Users/")
                        || pattern_line_hit(&text_to_check, "/Users/")
                        || pattern_line_hit(&text_to_check, "/home/")))
                    || text_to_check.contains("192.168.")
                    || simple_ip_match(&text_to_check, "10.")
                    || simple_ip_match(&text_to_check, "172.")
                {
                    hits.push(format!(
                        "commit {hash}: generic pattern matched in commit message"
                    ));
                }
            }
            Err(_) => {
                hits.push(format!("commit {hash}: could not read details"));
            }
        }
    }

    if hits.is_empty() {
        println!("privacy: clean ({file_count} files, {commit_count} commits)");
        Ok(())
    } else {
        for hit in &hits {
            println!("privacy: {hit}");
        }
        eprintln!("\nprivacy FAILED");
        std::process::exit(1);
    }
}

// ── Gate Status ────────────────────────────────────────────────────────────

/// Verify the gate receipt against the current tree.
fn cmd_gate_status() -> io::Result<()> {
    let root = repo_root();
    let receipt_path = root.join("target").join("gate-receipt");

    if !receipt_path.exists() {
        eprintln!("gate-status: no receipt (run `cargo xtask gate` first)");
        std::process::exit(2);
    }

    let content = fs::read_to_string(&receipt_path)?;

    // Parse receipt
    let mut stored_digest = "";
    let mut stored_at = "";
    let mut stored_tests = "";
    let mut stored_files = "";
    for line in content.lines() {
        if let Some(v) = line.strip_prefix("digest=") {
            stored_digest = v;
        } else if let Some(v) = line.strip_prefix("at=") {
            stored_at = v;
        } else if let Some(v) = line.strip_prefix("tests=") {
            stored_tests = v;
        } else if let Some(v) = line.strip_prefix("files=") {
            stored_files = v;
        }
    }

    // Recompute digest over working-tree contents (same function `gate` used).
    let (current_digest, files) = tree_digest(&root)?;

    if stored_digest != current_digest {
        eprintln!("gate-status: stale receipt: tree changed since last gate");
        std::process::exit(1);
    }

    println!("gate green on this tree");
    println!("  at: {stored_at}");
    println!("  tests: {stored_tests}");
    println!("  files: {stored_files} (recomputed {files})");
    Ok(())
}

// ── Floor ──────────────────────────────────────────────────────────────────

/// Set or write the test floor.
fn cmd_floor(args: &[String]) -> io::Result<()> {
    let root = repo_root();
    let floor_path = root.join("tools").join("test-floor.txt");

    if args.iter().any(|a| a == "--current") {
        let count = count_tests(&root)?;
        fs::create_dir_all(floor_path.parent().unwrap())?;
        fs::write(&floor_path, format!("{count}\n"))?;
        println!("floor: wrote {count}");
        return Ok(());
    }

    if args.is_empty() {
        eprintln!("Usage: cargo xtask floor <n>");
        std::process::exit(2);
    }

    let new_floor: usize = args[0].parse().unwrap_or_else(|e| {
        eprintln!("Usage: cargo xtask floor <n> (integer): {e}");
        std::process::exit(2);
    });

    let current = read_floor(&root);
    let max_floor = std::cmp::max(current, new_floor);

    fs::create_dir_all(floor_path.parent().unwrap())?;
    fs::write(&floor_path, format!("{max_floor}\n"))?;
    println!("floor: {current} -> {max_floor}");
    Ok(())
}

// ── Mutate ─────────────────────────────────────────────────────────────────

/// Run cargo-mutants in-diff.
fn cmd_mutate(_args: &[String]) -> io::Result<()> {
    let root = repo_root();

    // Find base ref
    let base = if let Ok(b) = find_merge_base(&root) {
        b
    } else {
        println!("mutate: no base ref (first push) — in-diff skipped; run the weekly full job");
        return Ok(());
    };

    // Write diff
    fs::create_dir_all(root.join("target"))?;
    let diff_path = root.join("target").join("mutants.diff");
    let diff_args = ["diff", &base, "--", ".", ":(exclude)Cargo.lock"];
    let diff_output = Command::new("git")
        .args(diff_args)
        .current_dir(&root)
        .output()?;
    fs::write(&diff_path, &diff_output.stdout)?;

    // Check if cargo-mutants is installed
    let version_check = Command::new("cargo")
        .args(["mutants", "--version"])
        .current_dir(&root)
        .output()?;
    if !version_check.status.success() {
        println!(
            "mutate: cargo-mutants not installed — SKIPPED (owner has not approved the install); install with: cargo install cargo-mutants --locked"
        );
        return Ok(());
    }

    // Measure baseline wall time
    let baseline_start = Instant::now();
    let _baseline = Command::new("cargo")
        .args(["test", "-p", "hadal-core", "--lib"])
        .current_dir(&root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .status()?;
    let baseline_secs = baseline_start.elapsed().as_secs_f64();

    let timeout: u64 = (4.0 * baseline_secs).ceil().clamp(20.0, 60.0) as u64;
    let build_timeout = (3.0 * baseline_secs).ceil() as u64;

    println!(
        "mutate: baseline={:.1}s, timeout={}s, build-timeout={}s",
        baseline_secs, timeout, build_timeout
    );

    let status = Command::new("cargo")
        .args([
            "mutants",
            "-p",
            "hadal-core",
            "--in-diff",
            diff_path.to_string_lossy().as_ref(),
            "--timeout",
            &timeout.to_string(),
            "--build-timeout",
            &build_timeout.to_string(),
        ])
        .current_dir(&root)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;

    if !status.success() {
        eprintln!("\nmutate FAILED");
        std::process::exit(1);
    }

    println!("mutate passed.");
    Ok(())
}

// ── Dist ───────────────────────────────────────────────────────────────────

/// The host target triple in Cargo's canonical form (e.g.
/// `x86_64-pc-windows-msvc`), which is what release archives are named after
/// (plan 5.10). Derived from rustc so it can never drift from the real target.
fn host_target_triple() -> String {
    if let Ok(out) = Command::new("rustc").arg("-vV").output()
        && out.status.success()
    {
        let text = String::from_utf8_lossy(&out.stdout);
        if let Some(line) = text.lines().find(|l| l.starts_with("host: ")) {
            let host = line.trim_start_matches("host: ").trim();
            if !host.is_empty() {
                return host.to_owned();
            }
        }
    }
    // Fallback: compose from cfg/consts where the mapping is unambiguous.
    format!(
        "{}-{}-{}",
        env::consts::ARCH,
        if cfg!(target_os = "windows") {
            "pc-windows-msvc"
        } else if cfg!(target_os = "macos") {
            "apple-darwin"
        } else {
            "unknown-linux-gnu"
        },
        ""
    )
    .trim_end_matches('-')
    .to_owned()
}

/// Build release and package distribution.
fn cmd_dist(out_dir: Option<&str>) -> io::Result<()> {
    let root = repo_root();
    let out_dir = out_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("dist"));

    // Build release binary
    let build_status = Command::new("cargo")
        .args(["build", "--release", "-p", "hadal"])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;
    if !build_status.success() {
        eprintln!("GATE FAILED: cargo build --release -p hadal");
        std::process::exit(1);
    }

    // Determine binary name
    let bin_name = if cfg!(target_os = "windows") {
        "hadal.exe"
    } else {
        "hadal"
    };

    let version = VERSION;
    let target_triple = host_target_triple();

    // Read the release binary
    let binary_path = root.join("target").join("release").join(bin_name);

    if !binary_path.exists() {
        eprintln!("ERROR: binary not found at {}", binary_path.display());
        std::process::exit(1);
    }

    let binary_data = fs::read(&binary_path)?;

    // Create distribution directory
    let dist_dir = out_dir.join(format!("hadal-{version}-{target_triple}"));
    fs::create_dir_all(&dist_dir)?;

    // Copy binary
    let dist_bin = dist_dir.join(bin_name);
    fs::copy(&binary_path, &dist_bin)?;

    // Copy LICENSE files
    for license in ["LICENSE-MIT", "LICENSE-APACHE"] {
        let src = root.join(license);
        if src.exists() {
            let dst = dist_dir.join(license);
            fs::copy(&src, dst)?;
        }
    }

    // Copy README if present
    let readme = root.join("README.md");
    if readme.exists() {
        fs::copy(&readme, dist_dir.join("README.md"))?;
    }

    // Compute SHA256
    let mut hasher = Sha256::new();
    hasher.update(&binary_data);
    let hash_hex = hasher
        .finalize()
        .as_slice()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>();

    // Write SHA256SUMS
    let sums_content = format!("{hash_hex}  {bin_name}\n");
    fs::write(dist_dir.join("SHA256SUMS"), sums_content)?;

    println!("Distribution ready: {}", dist_dir.display());
    Ok(())
}

// ── Main ───────────────────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        std::process::exit(2);
    }

    let command = args[1].as_str();

    match command {
        "--version" | "-V" | "version" => {
            println!("HADAL xtask v{VERSION}");
        }
        "--help" | "-h" | "help" => {
            print_usage();
        }
        "setup" => {
            if let Err(e) = cmd_setup() {
                eprintln!("ERROR: {e}");
                std::process::exit(1);
            }
        }
        "gate" => {
            let release_pacing = args.len() >= 3 && args[2] == "--release-pacing";
            if let Err(e) = run_gate(release_pacing) {
                eprintln!("ERROR: {e}");
                std::process::exit(1);
            }
        }
        "dist" => {
            let mut out_dir: Option<&str> = None;
            let mut i = 2;
            while i < args.len() {
                if args[i] == "--out" && i + 1 < args.len() {
                    out_dir = Some(&args[i + 1]);
                    i += 2;
                } else {
                    eprintln!("Unknown dist option: {}", args[i]);
                    std::process::exit(2);
                }
            }
            if let Err(e) = cmd_dist(out_dir) {
                eprintln!("ERROR: {e}");
                std::process::exit(1);
            }
        }
        "checks" => {
            if let Err(e) = run_checks(&repo_root()) {
                eprintln!("ERROR: {e}");
                std::process::exit(1);
            }
        }
        "privacy" => {
            if let Err(e) = cmd_privacy() {
                eprintln!("ERROR: {e}");
                std::process::exit(1);
            }
        }
        "gate-status" => {
            if let Err(e) = cmd_gate_status() {
                eprintln!("ERROR: {e}");
                std::process::exit(1);
            }
        }
        "floor" => {
            if let Err(e) = cmd_floor(&args[2..]) {
                eprintln!("ERROR: {e}");
                std::process::exit(1);
            }
        }
        "mutate" => {
            if let Err(e) = cmd_mutate(&args[2..]) {
                eprintln!("ERROR: {e}");
                std::process::exit(1);
            }
        }
        _ => {
            eprintln!("Unknown command: {command}");
            print_usage();
            std::process::exit(2);
        }
    }
}
