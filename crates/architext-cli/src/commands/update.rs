//! `architext update` — self-update the native binary from GitHub releases.
//!
//! This is the off-npm path: download the latest release binary for this
//! platform, verify its SHA-256, and replace the running binary in place. If the
//! running binary is npm-managed (under node_modules), don't fight npm — install
//! a native copy to ~/.local/bin and print the steps to drop the npm install.
//! `--check-updates` reports availability without installing.

use std::io::Read;
use std::path::{Path, PathBuf};

const REPO: &str = "robot-accomplice/architext";

/// Map the compile-time target to the release asset key (matches the names the
/// publish workflow attaches). `None` on an unsupported platform.
fn target_key() -> Option<&'static str> {
    Some(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "darwin-arm64",
        ("macos", "x86_64") => "darwin-x64",
        ("linux", "x86_64") => "linux-x64",
        ("linux", "aarch64") => "linux-arm64",
        ("windows", "x86_64") => "win32-x64",
        _ => return None,
    })
}

fn asset_name(key: &str) -> String {
    if key == "win32-x64" {
        "architext-win32-x64.exe".to_string()
    } else {
        format!("architext-{key}")
    }
}

/// Parse "x.y.z" (tolerating a leading "v") into a comparable tuple; unparsable
/// parts sort as 0, so a dev `0.0.0` always reads as older than a real release.
fn ver_tuple(v: &str) -> (u64, u64, u64) {
    let mut it = v
        .trim()
        .trim_start_matches('v')
        .split('.')
        .map(|s| s.split(|c: char| !c.is_ascii_digit()).next().unwrap_or("").parse().unwrap_or(0));
    (it.next().unwrap_or(0), it.next().unwrap_or(0), it.next().unwrap_or(0))
}

fn http_get_bytes(url: &str) -> Result<Vec<u8>, String> {
    let resp = ureq::get(url)
        .set("User-Agent", "architext-update")
        .call()
        .map_err(|e| format!("request failed: {e}"))?;
    let mut buf = Vec::new();
    resp.into_reader()
        .read_to_end(&mut buf)
        .map_err(|e| format!("read failed: {e}"))?;
    Ok(buf)
}

fn http_get_string(url: &str) -> Result<String, String> {
    String::from_utf8(http_get_bytes(url)?).map_err(|e| format!("invalid UTF-8: {e}"))
}

fn latest_tag() -> Result<String, String> {
    let body = http_get_string(&format!("https://api.github.com/repos/{REPO}/releases/latest"))?;
    let v: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| format!("unexpected GitHub API response: {e}"))?;
    v.get("tag_name")
        .and_then(|t| t.as_str())
        .map(String::from)
        .ok_or_else(|| "no tag_name in the release response".to_string())
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn is_npm_managed(exe: &Path) -> bool {
    exe.components().any(|c| c.as_os_str() == "node_modules")
}

/// `--check-updates`: report whether a newer release exists; install nothing.
pub fn check(current_version: &str) {
    match latest_tag() {
        Ok(tag) => {
            let latest = tag.trim_start_matches('v');
            if ver_tuple(latest) > ver_tuple(current_version) {
                println!("Update available: {current_version} -> {latest}");
                println!("Run `architext update` to install it.");
            } else {
                println!("architext is up to date ({current_version}).");
            }
        }
        Err(e) => {
            eprintln!("Could not check for updates: {e}");
            std::process::exit(1);
        }
    }
}

/// `architext update`: download + verify + install the latest release binary.
pub fn run(current_version: &str) {
    let key = match target_key() {
        Some(k) => k,
        None => {
            eprintln!(
                "Self-update is not supported on {}-{}; build from source instead.",
                std::env::consts::OS,
                std::env::consts::ARCH
            );
            std::process::exit(1);
        }
    };

    let tag = match latest_tag() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Could not resolve the latest release: {e}");
            std::process::exit(1);
        }
    };
    let latest = tag.trim_start_matches('v');
    if ver_tuple(latest) <= ver_tuple(current_version) {
        println!("architext is already up to date ({current_version}).");
        return;
    }

    println!("Updating architext {current_version} -> {latest} ({key})...");
    let asset = asset_name(key);
    let base = format!("https://github.com/{REPO}/releases/download/{tag}");

    let bin = match http_get_bytes(&format!("{base}/{asset}")) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Download failed: {e}");
            std::process::exit(1);
        }
    };

    // Verify the checksum when the release ships SHA256SUMS (every release does
    // from 1.7.2 on); warn rather than fail for older asset-less releases.
    match http_get_string(&format!("{base}/SHA256SUMS")) {
        Ok(sums) => match sums
            .lines()
            .find(|l| l.split_whitespace().last() == Some(asset.as_str()))
            .and_then(|l| l.split_whitespace().next())
        {
            Some(want) => {
                if want != sha256_hex(&bin) {
                    eprintln!("Checksum mismatch for {asset}; aborting.");
                    std::process::exit(1);
                }
                println!("Checksum verified.");
            }
            None => eprintln!("warning: {asset} not listed in SHA256SUMS; skipping verification"),
        },
        Err(_) => eprintln!("warning: no SHA256SUMS on this release; skipping checksum verification"),
    }

    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Cannot locate the running executable: {e}");
            std::process::exit(1);
        }
    };

    if is_npm_managed(&exe) {
        migrate_to_native(&bin, latest);
        return;
    }

    if let Err(e) = replace_in_place(&exe, &bin) {
        eprintln!("Update failed: {e}");
        eprintln!("(if {} is not writable, reinstall via the installer)", exe.display());
        std::process::exit(1);
    }
    println!("Updated architext {current_version} -> {latest}.");
}

/// Atomically replace the running binary. On Unix, write a temp file next to the
/// target, mark it executable, and rename over the original — the running process
/// keeps its open inode. On Windows the running .exe can't be overwritten, so the
/// old one is renamed aside first.
fn replace_in_place(exe: &Path, bin: &[u8]) -> Result<(), String> {
    let dir = exe.parent().ok_or("executable has no parent directory")?;
    let tmp = dir.join(".architext-update.tmp");
    std::fs::write(&tmp, bin).map_err(|e| format!("write temp binary: {e}"))?;
    set_executable(&tmp)?;
    #[cfg(windows)]
    {
        let old = dir.join(".architext-old");
        let _ = std::fs::remove_file(&old);
        std::fs::rename(exe, &old).map_err(|e| format!("move old binary aside: {e}"))?;
        std::fs::rename(&tmp, exe).map_err(|e| format!("install new binary: {e}"))?;
        let _ = std::fs::remove_file(&old);
    }
    #[cfg(not(windows))]
    {
        std::fs::rename(&tmp, exe).map_err(|e| format!("install new binary: {e}"))?;
    }
    Ok(())
}

#[cfg(unix)]
fn set_executable(p: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mut perm = std::fs::metadata(p).map_err(|e| e.to_string())?.permissions();
    perm.set_mode(0o755);
    std::fs::set_permissions(p, perm).map_err(|e| e.to_string())
}
#[cfg(not(unix))]
fn set_executable(_p: &Path) -> Result<(), String> {
    Ok(())
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn local_bin() -> PathBuf {
    std::env::var_os("ARCHITEXT_INSTALL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".local").join("bin"))
}

/// npm-managed binary: don't overwrite it (npm would revert it and may flag a
/// modified install). Install a native copy to ~/.local/bin and print the
/// off-ramp — this is the comfortable path off npm.
fn migrate_to_native(bin: &[u8], latest: &str) {
    let dir = local_bin();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("Cannot create {}: {e}", dir.display());
        std::process::exit(1);
    }
    let dest = dir.join("architext");
    if let Err(e) = std::fs::write(&dest, bin) {
        eprintln!("Cannot write {}: {e}", dest.display());
        std::process::exit(1);
    }
    let _ = set_executable(&dest);
    println!("This architext was installed via npm, which manages its binary.");
    println!("Installed a native architext {latest} to {}", dest.display());
    println!();
    println!("To finish moving off npm:");
    println!("  1. npm uninstall -g @robotaccomplice/architext");
    println!("  2. ensure {} is on your PATH, ahead of the npm bin", dir.display());
    println!();
    println!("After that, `architext update` keeps it current with no npm involved.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_tuple_ordering() {
        assert!(ver_tuple("1.7.1") > ver_tuple("1.7.0"));
        assert!(ver_tuple("1.8.0") > ver_tuple("1.7.9"));
        assert!(ver_tuple("2.0.0") > ver_tuple("1.99.99"));
        assert_eq!(ver_tuple("v1.7.1"), ver_tuple("1.7.1"));
        // a dev 0.0.0 always reads as older than any real release
        assert!(ver_tuple("1.0.0") > ver_tuple("0.0.0"));
        // tolerate prerelease suffixes on a part
        assert_eq!(ver_tuple("1.7.1-rc1"), (1, 7, 1));
    }

    #[test]
    fn asset_names_per_platform() {
        assert_eq!(asset_name("darwin-arm64"), "architext-darwin-arm64");
        assert_eq!(asset_name("linux-x64"), "architext-linux-x64");
        assert_eq!(asset_name("win32-x64"), "architext-win32-x64.exe");
    }

    #[test]
    fn npm_managed_detection() {
        assert!(is_npm_managed(Path::new(
            "/usr/lib/node_modules/@robotaccomplice/architext-darwin-arm64/architext"
        )));
        assert!(!is_npm_managed(Path::new("/home/u/.local/bin/architext")));
    }

    #[test]
    fn sha256_matches_known_vector() {
        // echo -n "" | sha256sum
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn replace_in_place_swaps_contents() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("architext");
        std::fs::write(&exe, b"OLD").unwrap();
        replace_in_place(&exe, b"NEWBINARY").unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"NEWBINARY");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&exe).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111, "binary should be executable");
        }
        // no leftover temp files
        assert!(!dir.path().join(".architext-update.tmp").exists());
    }
}
