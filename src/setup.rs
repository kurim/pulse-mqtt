//! The app is its own installer: copy to Program Files, entry in Settings > Apps, start
//! menu shortcut, and uninstall. The windows for it are in `gui`. Apps entry, silent
//! uninstall and retries follow the installer of kurim/pulse-request.

use crate::{autostart, config, tray, ui::wide};
use std::{
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
    ptr::{null, null_mut},
    thread,
    time::Duration,
};
use windows_sys::{
    Win32::{
        Foundation::CloseHandle,
        System::{
            Com::{
                CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
                CoUninitialize,
            },
            ProcessStatus::K32EnumProcesses,
            Registry::{
                HKEY, HKEY_LOCAL_MACHINE, REG_DWORD, REG_SZ, RRF_RT_REG_SZ, RegDeleteTreeW,
                RegGetValueW, RegSetKeyValueW,
            },
            Threading::{
                CREATE_NO_WINDOW, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
                PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, QueryFullProcessImageNameW,
                TerminateProcess, WaitForSingleObject,
            },
        },
        UI::{
            Shell::{ShellExecuteW, ShellLink},
            WindowsAndMessaging::SW_SHOWNORMAL,
        },
    },
    core::GUID,
};

/// Entry in Settings > Apps > Installed apps.
const APPS_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\PulseMQTT-Rust";
// Fixed paths on purpose, as in `autostart`.
const SHORTCUT: &str = r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs\PulseMQTT.lnk";

/// Starts this program again with admin rights; Windows asks the user unless it has them.
pub fn elevate(arguments: &[String]) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = exe.to_str().ok_or("the program path is not valid text")?;
    // SAFETY: NUL-terminated strings that outlive the call.
    let result = unsafe {
        ShellExecuteW(
            null_mut(),
            wide("runas").as_ptr(),
            wide(exe).as_ptr(),
            // Our own flags only, none of which contains a space.
            wide(&arguments.join(" ")).as_ptr(),
            null(),
            SW_SHOWNORMAL,
        )
    };
    if result as usize > 32 {
        Ok(())
    } else {
        Err("administrator rights are needed".into())
    }
}

/// Installs or updates, then starts the installed copy, which takes over the settings
/// and registers itself. Needs admin rights.
pub fn install(with_autostart: bool) -> Result<(), String> {
    let current = std::env::current_exe().map_err(|e| e.to_string())?;
    let program = autostart::program();
    let folder = program.parent().ok_or("no program folder")?;
    if !autostart::installed() {
        stop(&program)?;
        // The installed copy would not start next to another one, and so not finish.
        if tray::running() {
            return Err("Another copy of PulseMQTT is running; exit it and try again.".into());
        }
        autostart::copy_program(&current, folder)?;
    }
    if !with_autostart && autostart::enabled() {
        autostart::disable()?;
    }
    let mut next = Command::new(&program);
    next.arg(format!("--after={}", std::process::id()));
    if with_autostart {
        // Switched on by that copy, after it has taken over the settings of the .NET app.
        next.arg("--enable-autostart");
    }
    next.current_dir(folder)
        .spawn()
        .map(drop)
        .map_err(|e| format!("{}: {e}", program.display()))
}

/// Keeps the Apps entry and the start menu shortcut of the installed copy up to date.
pub fn register() -> Result<(), String> {
    let program = autostart::program();
    write_entry(HKEY_LOCAL_MACHINE, APPS_KEY, &program)?;
    shortcut(Path::new(SHORTCUT), &program)
}

/// The version in the Apps entry.
pub fn installed_version() -> Option<String> {
    read_version(HKEY_LOCAL_MACHINE, APPS_KEY)
}

/// Removes everything but the running program, see `delete_self_later`. Settings and login
/// stay unless `purge`. Needs admin rights.
pub fn uninstall(purge: bool) -> Result<(), String> {
    let program = autostart::program();
    let folder = program.parent().ok_or("no program folder")?;
    let removed = autostart::disable();
    if autostart::enabled() {
        removed?;
    }
    stop(&program)?;
    let mut files = vec![
        program.with_extension("old"),
        program.with_extension("new"),
        folder.join("task.xml"),
        PathBuf::from(SHORTCUT),
    ];
    if !autostart::installed() {
        files.push(program.clone());
    }
    if purge {
        for folder in [folder.to_owned(), config::user_dir()?] {
            for name in ["config.json", "config.tmp", "login.dat", "login.tmp"] {
                files.push(folder.join(name));
            }
        }
    }
    remove_files(&files)?;
    // SAFETY: a NUL-terminated key name. A missing key is fine.
    unsafe { RegDeleteTreeW(HKEY_LOCAL_MACHINE, wide(APPS_KEY).as_ptr()) };
    // Only works if nothing is left in it.
    std::fs::remove_dir(folder).ok();
    Ok(())
}

/// A running program cannot delete itself: a hidden command does it after this one has
/// ended. Call it right before exiting.
pub fn delete_self_later() {
    if !autostart::installed() {
        return;
    }
    let program = autostart::program();
    let Some(folder) = program.parent() else {
        return;
    };
    Command::new(r"C:\Windows\System32\cmd.exe")
        .raw_arg(format!(
            r#"/d /c C:\Windows\System32\ping.exe -n 4 127.0.0.1 >nul & del /f /q "{}" & rmdir "{}""#,
            program.display(),
            folder.display()
        ))
        .current_dir(r"C:\Windows")
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .ok();
}

/// Deletes a file if it exists. Windows or a virus scanner may hold it for a moment after
/// the program has ended.
fn remove(file: &Path) -> std::io::Result<()> {
    let mut result = std::fs::remove_file(file);
    for _ in 0..40 {
        match &result {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                thread::sleep(Duration::from_millis(250));
                result = std::fs::remove_file(file);
            }
            _ => return Ok(()),
        }
    }
    result
}

/// Deletes the files that exist.
fn remove_files(files: &[PathBuf]) -> Result<(), String> {
    let failed: Vec<String> = files
        .iter()
        .filter_map(|file| {
            let e = remove(file).err()?;
            Some(format!("{}: {e}", file.display()))
        })
        .collect();
    if failed.is_empty() {
        Ok(())
    } else {
        Err(failed.join("\n"))
    }
}

/// The other processes that run `program`.
fn processes(program: &Path) -> Vec<u32> {
    let mut ids = [0u32; 4096];
    let mut bytes = 0;
    // SAFETY: the buffer has the size passed in.
    if unsafe { K32EnumProcesses(ids.as_mut_ptr(), size_of_val(&ids) as u32, &mut bytes) } == 0 {
        return Vec::new();
    }
    ids.into_iter()
        .take(bytes as usize / size_of::<u32>())
        .filter(|&id| id != std::process::id() && runs(id, program))
        .collect()
}

fn runs(id: u32, program: &Path) -> bool {
    let mut path = [0u16; 1024];
    let mut length = path.len() as u32;
    // SAFETY: the handle is checked and closed; the buffer has the length passed in.
    let ok = unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, id);
        if process.is_null() {
            return false;
        }
        let ok = QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut length);
        CloseHandle(process);
        ok
    };
    ok != 0
        && String::from_utf16_lossy(&path[..length as usize])
            .eq_ignore_ascii_case(&program.to_string_lossy())
}

/// Ends the other processes of `program`, so that it can be replaced or removed.
fn stop(program: &Path) -> Result<(), String> {
    for id in processes(program) {
        // SAFETY: the handle is checked and closed.
        unsafe {
            let process = OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, 0, id);
            if process.is_null() {
                continue; // it has ended meanwhile, or the check below reports it
            }
            // Exit code 0, so that the task does not restart it.
            TerminateProcess(process, 0);
            WaitForSingleObject(process, 10_000);
            CloseHandle(process);
        }
    }
    if processes(program).is_empty() {
        Ok(())
    } else {
        Err("PulseMQTT is still running; exit it and try again".into())
    }
}

fn write_entry(root: HKEY, key: &str, program: &Path) -> Result<(), String> {
    let path = program.to_string_lossy();
    let folder = program.parent().ok_or("no program folder")?;
    let set = |name: &str, kind, data: *const u8, bytes: usize| {
        // SAFETY: NUL-terminated names and `bytes` readable bytes at `data`.
        let status = unsafe {
            RegSetKeyValueW(
                root,
                wide(key).as_ptr(),
                wide(name).as_ptr(),
                kind,
                data.cast(),
                bytes as u32,
            )
        };
        if status == 0 {
            Ok(())
        } else {
            let error = std::io::Error::from_raw_os_error(status as i32);
            Err(format!("registry: {error}"))
        }
    };
    for (name, value) in [
        ("DisplayName", "PulseMQTT".to_owned()),
        ("DisplayVersion", env!("CARGO_PKG_VERSION").to_owned()),
        ("Publisher", "kurim".to_owned()),
        ("InstallLocation", folder.to_string_lossy().into_owned()),
        ("DisplayIcon", path.to_string()),
        ("UninstallString", format!("\"{path}\" --uninstall")),
        (
            "QuietUninstallString",
            format!("\"{path}\" --uninstall --silent"),
        ),
        (
            "URLInfoAbout",
            "https://github.com/kurim/pulse-mqtt".to_owned(),
        ),
    ] {
        let value = wide(&value);
        set(name, REG_SZ, value.as_ptr().cast(), value.len() * 2)?;
    }
    // In KiB.
    let size = std::fs::metadata(program).map_or(0, |m| m.len() / 1024) as u32;
    for (name, value) in [("NoModify", 1), ("NoRepair", 1), ("EstimatedSize", size)] {
        let value = value.to_le_bytes();
        set(name, REG_DWORD, value.as_ptr(), value.len())?;
    }
    Ok(())
}

fn read_version(root: HKEY, key: &str) -> Option<String> {
    let mut value = [0u16; 64];
    let mut bytes = size_of_val(&value) as u32;
    // SAFETY: the buffer has the size passed in.
    let status = unsafe {
        RegGetValueW(
            root,
            wide(key).as_ptr(),
            wide("DisplayVersion").as_ptr(),
            RRF_RT_REG_SZ,
            null_mut(),
            value.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    // The size includes the terminating NUL.
    (status == 0)
        .then(|| String::from_utf16_lossy(&value[..(bytes as usize / 2).saturating_sub(1)]))
}

/// Writes a shortcut to `program`. windows-sys has no COM interfaces, so the two methods
/// needed of IShellLinkW and IPersistFile are called by their place in the table.
fn shortcut(link: &Path, program: &Path) -> Result<(), String> {
    type Object = *mut *const usize;
    const SHELL_LINK: GUID = GUID::from_u128(0x000214F9_0000_0000_C000_000000000046);
    const PERSIST_FILE: GUID = GUID::from_u128(0x0000010B_0000_0000_C000_000000000046);
    const QUERY_INTERFACE: usize = 0;
    const RELEASE: usize = 2;
    const SAVE: usize = 6;
    const SET_PATH: usize = 20;
    /// The method at `index`; `F` has to be its signature.
    unsafe fn method<F: Copy>(object: Object, index: usize) -> F {
        // SAFETY: a live COM object starts with a pointer to its method table.
        unsafe { std::mem::transmute_copy(&*(*object).add(index)) }
    }
    type Query = unsafe extern "system" fn(Object, *const GUID, *mut Object) -> i32;
    type Release = unsafe extern "system" fn(Object) -> u32;
    type SetPath = unsafe extern "system" fn(Object, *const u16) -> i32;
    type Save = unsafe extern "system" fn(Object, *const u16, i32) -> i32;

    let target = wide(&program.to_string_lossy());
    let file = wide(&link.to_string_lossy());
    // SAFETY: the GUIDs and indexes match the documented interfaces, every object is
    // checked before use and released, and the strings are NUL-terminated.
    let result = unsafe {
        let initialised = CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) >= 0;
        let mut shell: Object = null_mut();
        let mut result = CoCreateInstance(
            &ShellLink,
            null_mut(),
            CLSCTX_INPROC_SERVER,
            &SHELL_LINK,
            (&raw mut shell).cast(),
        );
        if result >= 0 {
            result = method::<SetPath>(shell, SET_PATH)(shell, target.as_ptr());
            let mut persist: Object = null_mut();
            if result >= 0 {
                result =
                    method::<Query>(shell, QUERY_INTERFACE)(shell, &PERSIST_FILE, &mut persist);
            }
            if result >= 0 {
                result = method::<Save>(persist, SAVE)(persist, file.as_ptr(), 1);
                method::<Release>(persist, RELEASE)(persist);
            }
            method::<Release>(shell, RELEASE)(shell);
        }
        if initialised {
            CoUninitialize();
        }
        result
    };
    if result >= 0 {
        Ok(())
    } else {
        Err(format!("shortcut: error {result:#010x}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::System::Registry::HKEY_CURRENT_USER;

    #[test]
    fn apps_entry_is_written_read_and_removable() {
        let key = format!(r"Software\PulseMQTT-test-{}", std::process::id());
        assert_eq!(read_version(HKEY_CURRENT_USER, &key), None);
        write_entry(HKEY_CURRENT_USER, &key, &autostart::program()).unwrap();
        assert_eq!(
            read_version(HKEY_CURRENT_USER, &key).as_deref(),
            Some(env!("CARGO_PKG_VERSION"))
        );
        // SAFETY: a NUL-terminated key name.
        unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, wide(&key).as_ptr()) };
        assert_eq!(read_version(HKEY_CURRENT_USER, &key), None);
    }

    #[test]
    fn shortcut_points_to_the_program() {
        let link = std::env::temp_dir().join(format!("pulsemqtt-{}.lnk", std::process::id()));
        shortcut(&link, &std::env::current_exe().unwrap()).unwrap();
        let content = std::fs::read(&link).unwrap();
        // Header size of a shell link, and the file name of the target.
        assert_eq!(content[0], 0x4C);
        let name = std::env::current_exe().unwrap();
        let name = name.file_name().unwrap().to_string_lossy().into_owned();
        let found = |needle: &[u8]| content.windows(needle.len()).any(|w| w == needle);
        let unicode: Vec<u8> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert!(found(name.as_bytes()) || found(&unicode));
        std::fs::remove_file(link).unwrap();
    }

    #[test]
    fn finds_and_stops_only_processes_of_the_program() {
        let copy = std::env::temp_dir().join(format!("pulsemqtt-stop-{}.exe", std::process::id()));
        std::fs::copy(r"C:\Windows\System32\ping.exe", &copy).unwrap();
        let mut child = Command::new(&copy)
            .args(["-n", "30", "127.0.0.1"])
            .creation_flags(CREATE_NO_WINDOW)
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        assert_eq!(processes(&copy), [child.id()]);
        // The own process is never listed.
        assert!(processes(&std::env::current_exe().unwrap()).is_empty());
        stop(&copy).unwrap();
        assert!(child.wait().unwrap().success());
        remove_files(&[copy.clone(), copy]).unwrap();
    }
}
