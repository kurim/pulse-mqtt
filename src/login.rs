//! MQTT login: `login.dat` next to `config.json`, holding "user\npassword" encrypted with
//! DPAPI for the current Windows user. Written by the settings window or when taking over the .NET app.

use std::{io, path::PathBuf, ptr};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPT_STRING_BASE64, CryptProtectData, CryptStringToBinaryW,
        CryptUnprotectData,
    },
};

fn path() -> Result<PathBuf, String> {
    Ok(crate::config::data_dir()?.join("login.dat"))
}
/// Whether a login is saved, readable or not.
pub fn exists() -> bool {
    path().is_ok_and(|path| path.exists())
}

/// `Ok(None)` if no login is saved. An unreadable file is an error, so that a saved
/// login can never silently turn into anonymous access.
pub fn saved() -> Result<Option<(String, String)>, String> {
    let path = path()?;
    let fail = |e: String| format!("{}: {e}", path.display());
    match std::fs::read(&path) {
        Ok(data) => crypt(&data, false, &[])
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
            crate::config::write_replacing(&path, &crypt(plain.as_bytes(), true, &[])?)
        }
        None => match std::fs::remove_file(&path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => {
                Err(format!("{}: {e}", path.display()))
            }
            _ => Ok(()),
        },
    }
}

/// Decrypts a base64 text that .NET protected for the current user with `entropy`.
pub fn decrypt_base64(text: &str, entropy: &[u8]) -> Result<String, String> {
    let wide: Vec<u16> = text.encode_utf16().collect();
    let length = wide.len().try_into().map_err(|_| "text too long")?;
    let mut data = vec![0u8; wide.len()];
    let mut size = data.len().try_into().map_err(|_| "text too long")?;
    // SAFETY: both buffers have the lengths passed in.
    let ok = unsafe {
        CryptStringToBinaryW(
            wide.as_ptr(),
            length,
            CRYPT_STRING_BASE64,
            data.as_mut_ptr(),
            &mut size,
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    if ok == 0 || size == 0 {
        return Err("invalid base64".into());
    }
    data.truncate(size as usize);
    String::from_utf8(crypt(&data, false, entropy)?).map_err(|e| e.to_string())
}

/// The counterpart of `decrypt_base64`, to test it.
#[cfg(test)]
pub fn encrypt_base64(text: &str, entropy: &[u8]) -> Result<String, String> {
    use windows_sys::Win32::Security::Cryptography::{CRYPT_STRING_NOCRLF, CryptBinaryToStringW};
    let data = crypt(text.as_bytes(), true, entropy)?;
    let mut buffer = vec![0u16; data.len() * 2 + 8];
    let mut length = buffer.len() as u32;
    // SAFETY: both buffers have the lengths passed in.
    let ok = unsafe {
        CryptBinaryToStringW(
            data.as_ptr(),
            data.len() as u32,
            CRYPT_STRING_BASE64 | CRYPT_STRING_NOCRLF,
            buffer.as_mut_ptr(),
            &mut length,
        )
    };
    (ok != 0)
        .then(|| String::from_utf16_lossy(&buffer[..length as usize]))
        .ok_or_else(|| "base64".into())
}

fn crypt(data: &[u8], encrypt: bool, entropy: &[u8]) -> Result<Vec<u8>, String> {
    let extra = CRYPT_INTEGER_BLOB {
        cbData: entropy.len() as u32,
        pbData: entropy.as_ptr().cast_mut(), // only read by the calls
    };
    let extra = if entropy.is_empty() {
        ptr::null()
    } else {
        &raw const extra
    };
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
                extra,
                ptr::null(),
                ptr::null(),
                0,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                ptr::null_mut(),
                extra,
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
        let secret = crypt(b"user\npassword", true, &[]).unwrap();
        assert_ne!(secret, b"user\npassword");
        assert_eq!(crypt(&secret, false, &[]).unwrap(), b"user\npassword");
        assert!(crypt(&secret, false, b"other").is_err());
        assert!(crypt(b"user\npassword", false, &[]).is_err());
    }
}
