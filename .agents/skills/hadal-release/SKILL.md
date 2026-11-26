# HADAL Release Skill

**Purpose:** Build, package and ship release artifacts with checksums and installer smoke
tests, and create the release tag only after the gate passes.

**When to use:** Preparing a `v*` release, building the target matrix, generating checksums,
running installer smoke tests, auditing dependencies.

## Workflow

1. **Run the full gate (release mode):**
   ```
   cargo xtask gate
   ```
   All checks must pass, and `cargo xtask gate-status` must report a fresh receipt for the
   tree. The receipt is anchored to a content digest of the working tree.

2. **Build the target matrix (release):**
   ```
   cargo build --release -p hadal --target x86_64-unknown-linux-musl
   cargo build --release -p hadal --target x86_64-pc-windows-msvc
   cargo build --release -p hadal --target x86_64-apple-darwin
   cargo build --release -p hadal --target aarch64-apple-darwin
   ```
   For a local host-only package, `cargo xtask dist [out-dir]` builds the host triple and
   writes an archive.

3. **Smoke each artifact:**
   ```
   hadal --version
   hadal --demo
   hadal --ascii
   hadal-sim --hours 0
   ```

4. **Generate checksums:**
   ```
   sha256sum hadal-* > SHA256SUMS
   ```

5. **Audit dependencies:**
   ```
   cargo deny check
   ```

6. **Verify installers:**
   - Linux: `scripts/install.sh` — POSIX, SHA-256 verification, `~/.local/bin` default,
     `HADAL_DOWNLOAD_BASE` locates the archive.
   - Windows: `scripts/install.ps1` — `Get-FileHash`, `%LOCALAPPDATA%\hadal\bin`.

7. **Tag after the gate passes:**
   ```
   git tag -a v0.1.0 -m "HADAL v0.1.0 — M1 vertical slice"
   ```
   Never force-push; push the tag only when a remote exists.

## Target matrix

| Target | Notes |
|---|---|
| `x86_64-unknown-linux-musl` | Static binary; covers Ubuntu 24+, Debian, Arch, Fedora, Gentoo |
| `x86_64-pc-windows-msvc` | Static CRT |
| `x86_64-apple-darwin` | macOS Intel |
| `aarch64-apple-darwin` | macOS Apple Silicon |

Measured stripped Windows release binary: **1.71 MB** (budget < 20 MB).

## Checklist

- [ ] `cargo xtask gate` passes and `gate-status` is fresh
- [ ] All four targets build in release
- [ ] `--version`, `--demo`, `--ascii`, `hadal-sim --hours 0` smoke each artifact
- [ ] `SHA256SUMS` generated
- [ ] `cargo deny check` clean
- [ ] Installers verified
- [ ] Tag created only after the gate
- [ ] Archive contains binary, README, licenses, `SHA256SUMS`

## Notes

- No signing certificates are available; artifacts are unsigned. The README gives macOS
  quarantine (`xattr -d com.apple.quarantine hadal`) and Windows SmartScreen guidance. No
  notarization is claimed.
- macOS/Linux runtime stays unverified until CI runs.
- Version `0.1.0` for M1.

## Commands

```
cargo xtask gate
cargo xtask gate-status
cargo xtask dist
cargo build --release -p hadal --target x86_64-unknown-linux-musl
cargo build --release -p hadal --target x86_64-pc-windows-msvc
cargo build --release -p hadal --target x86_64-apple-darwin
cargo build --release -p hadal --target aarch64-apple-darwin
cargo deny check
sha256sum hadal-* > SHA256SUMS
```
