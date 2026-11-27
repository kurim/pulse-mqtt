//! MQTT login: `login.dat` next to the executable, holding "user\npassword" encrypted
//! with DPAPI for the current Windows user. Written by install.ps1 or the settings window.

use std::{io, path::PathBuf, ptr};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::Cryptography::{CRYPT_INTEGER_BLOB, CryptProtectData, CryptUnprotectData},
};

fn path() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    Ok(exe.with_file_name("login.dat"))
}

/// `Ok(None)` if no login is saved. An unreadable file is an error, so that a saved
/// login can never silently turn into anonymous access.
pub fn saved() -> Result<Option<(String, String)>, String> {
    let path = path()?;
    let fail = |e: String| format!("{}: {e}", path.display());
    match std::fs::read(&path) {
        Ok(data) => crypt(&data, false)
            .and_then(|d| parse(&d))
            .map(Some)
            .map_err(fail),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(fail(e.to_string())),
    }
}

/// Saves the login, or removes it for `None`.
pub fn save(login: Option<(&str, &str)>) -> Result<(), String> {
    let path = path()?;
    match login {
        Some((user, password)) => {
            let plain = format!("{user}\n{password}");
            parse(plain.as_bytes())?;
            crate::config::write_replacing(&path, &crypt(plain.as_bytes(), true)?)
        }
        None => match std::fs::remove_file(&path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => {
                Err(format!("{}: {e}", path.display()))
            }
            _ => Ok(()),
        },
    }
}

fn crypt(data: &[u8], encrypt: bool) -> Result<Vec<u8>, String> {
    let input = CRYPT_INTEGER_BLOB {
        cbData: data.len().try_into().map_err(|_| "file too large")?,
        pbData: data.as_ptr().cast_mut(), // only read by the calls
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    };
    // SAFETY: both blobs are valid; the output buffer is copied and then freed once.
    unsafe {
        let ok = if encrypt {
            CryptProtectData(
                &input,
                ptr::null(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                0,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                ptr::null_mut(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                0,
                &mut output,
            )
        };
        if ok == 0 {
            return Err(format!("DPAPI: {}", io::Error::last_os_error()));
        }
        let result = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        LocalFree(output.pbData.cast());
        Ok(result)
    }
}

fn parse(plain: &[u8]) -> Result<(String, String), String> {
    std::str::from_utf8(plain)
        .ok()
        .and_then(|text| text.split_once('\n'))
        .filter(|(user, password)| !user.is_empty() && !password.is_empty())
        .map(|(user, password)| (user.to_owned(), password.to_owned()))
        .ok_or_else(|| "invalid login".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_complete_logins() {
        let login = parse("user\npäss\nword".as_bytes()).unwrap();
        assert_eq!(login, ("user".into(), "päss\nword".into()));
        for bad in ["", "user", "user\n", "\npassword"] {
            assert!(parse(bad.as_bytes()).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn encryption_round_trips() {
        let secret = crypt(b"user\npassword", true).unwrap();
        assert_ne!(secret, b"user\npassword");
        assert_eq!(crypt(&secret, false).unwrap(), b"user\npassword");
        assert!(crypt(b"user\npassword", false).is_err());
    }
}
