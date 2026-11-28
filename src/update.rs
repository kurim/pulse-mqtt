//! Self-update from GitHub releases. The app usually runs elevated, so a new version is
//! only installed if it carries a valid minisign signature for exactly that version.

use minisign_verify::{PublicKey, Signature};
use std::{
    os::windows::process::CommandExt,
    path::Path,
    process::Command,
    sync::{Mutex, TryLockError},
};
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

const RELEASES: &str = "https://api.github.com/repos/kurim/pulse-mqtt/releases?per_page=30";
const DOWNLOADS: &str = "https://github.com/kurim/pulse-mqtt/releases/download/";
/// Names the old .NET app does not recognise, so it never installs these files.
const PROGRAM: &str = "pulsemqtt.exe";
const SIGNATURE: &str = "pulsemqtt.exe.minisig";
/// The public half of the release signing key; empty until one has been created.
const PUBLIC_KEY: &str = include_str!("../update.pub");

/// One check or installation at a time.
static BUSY: Mutex<()> = Mutex::new(());

#[derive(Debug, PartialEq)]
pub struct Release {
    pub version: String,
    program: String,
    signature: String,
}

/// Updates are off without a key to verify them.
pub fn configured() -> bool {
    PublicKey::decode(PUBLIC_KEY).is_ok()
}

/// The newest release that is newer than this program.
pub fn check() -> Result<Option<Release>, String> {
    let json = download(RELEASES, 5_000_000)?;
    newest(&json, env!("CARGO_PKG_VERSION"))
}

/// Replaces the running program. The caller restarts it afterwards.
pub fn install(release: &Release) -> Result<(), String> {
    let _busy = match BUSY.try_lock() {
        Ok(guard) => guard,
        Err(TryLockError::Poisoned(e)) => e.into_inner(),
        Err(TryLockError::WouldBlock) => return Err("an update is already running".into()),
    };
    let program = download(&release.program, 100_000_000)?;
    let signature = download(&release.signature, 10_000)?;
    verify(PUBLIC_KEY, &program, &signature, &release.version)?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    replace(&exe, &program)
}

/// Removes the previous program left behind by an update. It may still be running.
pub fn clean_up() {
    if let Ok(exe) = std::env::current_exe() {
        std::fs::remove_file(exe.with_extension("old")).ok();
    }
}

fn download(url: &str, limit: u64) -> Result<Vec<u8>, String> {
    // curl ships with Windows. Full path: this process may be elevated.
    let output = Command::new(r"C:\Windows\System32\curl.exe")
        .args(["--fail", "--silent", "--show-error", "--location"])
        .args(["--proto", "=https", "--max-time", "300"])
        .args(["--max-filesize", &limit.to_string()])
        .args(["--user-agent", "PulseMQTT", url])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    if output.stdout.len() as u64 > limit {
        return Err("download is too large".into());
    }
    Ok(output.stdout)
}

/// "v1.2.3" or "1.2.3"; anything else, including pre-release suffixes, is ignored.
fn version(text: &str) -> Option<[u32; 3]> {
    let mut parts = text.strip_prefix('v').unwrap_or(text).split('.');
    let mut next = || parts.next()?.parse().ok();
    let result = [next()?, next()?, next()?];
    parts.next().is_none().then_some(result)
}

fn newest(json: &[u8], current: &str) -> Result<Option<Release>, String> {
    let releases: serde_json::Value = serde_json::from_slice(json).map_err(|e| e.to_string())?;
    let current = version(current).ok_or("own version is invalid")?;
    let releases = releases.as_array().ok_or("unexpected answer from GitHub")?;
    Ok(releases
        .iter()
        // Pre-releases count: this app is published as such while the .NET app is "latest".
        .filter(|r| r["draft"] != true)
        .filter_map(|r| {
            let number = version(r["tag_name"].as_str()?)?;
            let asset = |name: &str| {
                r["assets"].as_array()?.iter().find(|a| a["name"] == name)?["browser_download_url"]
                    .as_str()
                    .filter(|url| url.starts_with(DOWNLOADS))
                    .map(str::to_owned)
            };
            let release = Release {
                version: format!("{}.{}.{}", number[0], number[1], number[2]),
                program: asset(PROGRAM)?,
                signature: asset(SIGNATURE)?,
            };
            (number > current).then_some((number, release))
        })
        .max_by_key(|(number, _)| *number)
        .map(|(_, release)| release))
}

/// The signed comment names the version, so an old program cannot be offered as a new one.
fn verify(key: &str, program: &[u8], signature: &[u8], version: &str) -> Result<(), String> {
    let fail = |e: minisign_verify::Error| format!("signature check failed: {e}");
    let key = PublicKey::decode(key).map_err(fail)?;
    let signature = std::str::from_utf8(signature).map_err(|e| e.to_string())?;
    let signature = Signature::decode(signature).map_err(fail)?;
    key.verify(program, &signature, false).map_err(fail)?;
    if signature.trusted_comment() != format!("version:{version}") {
        return Err("signature belongs to another version".into());
    }
    Ok(())
}

/// Windows allows renaming a running program, but not overwriting it.
pub(crate) fn replace(exe: &Path, program: &[u8]) -> Result<(), String> {
    let (new, old) = (exe.with_extension("new"), exe.with_extension("old"));
    let fail = |e: std::io::Error| format!("{}: {e}", exe.display());
    // Next to the program, so that only those who may change it can touch the new file.
    std::fs::write(&new, program).map_err(fail)?;
    std::fs::remove_file(&old).ok();
    std::fs::rename(exe, &old).map_err(fail)?;
    std::fs::rename(&new, exe).map_err(|e| {
        std::fs::rename(&old, exe).ok();
        fail(e)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use minisign::KeyPair;

    #[test]
    fn picks_the_newest_complete_release() {
        let release = |tag: &str, draft: bool, base: &str| {
            serde_json::json!({
                "tag_name": tag, "draft": draft, "prerelease": true,
                "assets": [
                    {"name": PROGRAM, "browser_download_url": format!("{base}{tag}/{PROGRAM}")},
                    {"name": SIGNATURE, "browser_download_url": format!("{base}{tag}/{SIGNATURE}")},
                ]
            })
        };
        let json = serde_json::json!([
            release("v1.0.1", false, DOWNLOADS),
            release("v1.2.0", false, DOWNLOADS),
            release("v9.0.0", true, DOWNLOADS),
            release("v8.0.0", false, "https://example.invalid/"),
            release("v7.0.0-beta", false, DOWNLOADS),
            {"tag_name": "v6.0.0", "draft": false, "assets": []},
        ])
        .to_string();
        let found = newest(json.as_bytes(), "1.0.0").unwrap().unwrap();
        assert_eq!(found.version, "1.2.0");
        assert_eq!(newest(json.as_bytes(), "1.2.0").unwrap(), None);
        assert!(newest(b"{}", "1.0.0").is_err());
    }

    #[test]
    fn accepts_only_the_signed_program_and_version() {
        let keys = KeyPair::generate_unencrypted_keypair().unwrap();
        let key = keys.pk.to_box().unwrap().into_string();
        let signature = minisign::sign(
            Some(&keys.pk),
            &keys.sk,
            &b"program"[..],
            Some("version:1.2.0"),
            None,
        )
        .unwrap()
        .into_string();
        let check = |key: &str, program: &[u8], version| {
            verify(key, program, signature.as_bytes(), version)
        };
        assert!(check(&key, b"program", "1.2.0").is_ok());
        assert!(check(&key, b"tampered", "1.2.0").is_err());
        assert!(check(&key, b"program", "1.3.0").is_err());
        let other = KeyPair::generate_unencrypted_keypair().unwrap();
        let other = other.pk.to_box().unwrap().into_string();
        assert!(check(&other, b"program", "1.2.0").is_err());
    }

    #[test]
    fn replaces_a_file_and_keeps_the_old_one() {
        let exe = std::env::temp_dir().join(format!("pulsemqtt-test-{}.exe", std::process::id()));
        std::fs::write(&exe, b"old").unwrap();
        replace(&exe, b"new").unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"new");
        assert_eq!(std::fs::read(exe.with_extension("old")).unwrap(), b"old");
        std::fs::remove_file(exe.with_extension("old")).unwrap();
        std::fs::remove_file(exe).unwrap();
    }
}
