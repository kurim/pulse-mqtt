//! "Start with Windows": a scheduled task, because Windows does not start programs with
//! admin rights from the Run key. Because the task runs elevated, it points at a copy in
//! Program Files, where only admins can change it. Same task and folder as install.ps1.

use std::{
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Output},
};
use windows_sys::Win32::{
    Security::Authentication::Identity::{GetUserNameExW, NameSamCompatible},
    System::{
        Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW},
        Threading::CREATE_NO_WINDOW,
    },
    UI::Shell::IsUserAnAdmin,
};

const TASK: &str = "PulseMQTT-Rust";
/// Task of the .NET app, which uses the same port.
const OLD_TASK: &str = "PulseMQTT";
// Fixed paths on purpose: an elevated process must not trust the environment.
// shortcut: assumes Windows on C:; ask for the known folders if that ever differs.
const FOLDER: &str = r"C:\Program Files\PulseMQTT-Rust";
const SCHTASKS: &str = r"C:\Windows\System32\schtasks.exe";

/// Where the autostart copy of the program lives.
pub fn program() -> PathBuf {
    Path::new(FOLDER).join("pulsemqtt.exe")
}

/// Creating and removing the task needs admin rights.
pub fn possible() -> bool {
    // SAFETY: no arguments.
    unsafe { IsUserAnAdmin() != 0 }
}

pub fn enabled() -> bool {
    task_state(TASK)
}

pub fn old_app_enabled() -> bool {
    task_state(OLD_TASK) || old_run_value(false)
}

/// Removes both ways the .NET app used to start with Windows.
pub fn remove_old_app() {
    run(&[
        "/Delete".as_ref(),
        "/TN".as_ref(),
        OLD_TASK.as_ref(),
        "/F".as_ref(),
    ])
    .ok();
    old_run_value(true);
}

/// The Run value of the .NET app without admin rights: whether it exists, or delete it.
fn old_run_value(delete: bool) -> bool {
    let wide = |text: &str| -> Vec<u16> { text.encode_utf16().chain([0]).collect() };
    let key = wide(r"Software\Microsoft\Windows\CurrentVersion\Run");
    let name = wide(OLD_TASK);
    // SAFETY: NUL-terminated strings; no output buffers are passed.
    let status = unsafe {
        if delete {
            RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr())
        } else {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        }
    };
    status == 0
}

/// Copies the program to Program Files and starts it at every logon.
pub fn enable() -> Result<(), String> {
    let current = std::env::current_exe().map_err(|e| e.to_string())?;
    let folder = Path::new(FOLDER);
    if !current
        .as_os_str()
        .eq_ignore_ascii_case(program().as_os_str())
    {
        // The task does not get a login from the environment: the broker would be open.
        if std::env::var_os("PULSEMQTT_USER").is_some() && matches!(crate::login::saved(), Ok(None))
        {
            return Err("PULSEMQTT_USER: save the login in this window first".into());
        }
        copy_program(&current, folder)?;
    }
    // Next to the program, so that nobody without admin rights can swap the definition.
    let definition = folder.join("task.xml");
    let utf16: Vec<u8> = std::iter::once(0xFEFF)
        .chain(task_xml(&user()?, &program()).encode_utf16())
        .flat_map(u16::to_le_bytes)
        .collect();
    std::fs::write(&definition, utf16).map_err(|e| format!("{}: {e}", definition.display()))?;
    let result = run(&[
        "/Create".as_ref(),
        "/TN".as_ref(),
        TASK.as_ref(),
        "/XML".as_ref(),
        definition.as_os_str(),
        "/F".as_ref(),
    ]);
    std::fs::remove_file(&definition).ok();
    result.map(drop)
}

/// Removes the task. The copy in Program Files stays.
pub fn disable() -> Result<(), String> {
    run(&[
        "/Delete".as_ref(),
        "/TN".as_ref(),
        TASK.as_ref(),
        "/F".as_ref(),
    ])
    .map(drop)
}

fn copy_program(source: &Path, folder: &Path) -> Result<(), String> {
    let fail = |path: &Path, e: std::io::Error| format!("{}: {e}", path.display());
    std::fs::create_dir_all(folder).map_err(|e| fail(folder, e))?;
    let target = folder.join("pulsemqtt.exe");
    let program = std::fs::read(source).map_err(|e| fail(source, e))?;
    if target.exists() {
        // It may be running.
        crate::update::replace(&target, &program)
    } else {
        std::fs::write(&target, program).map_err(|e| fail(&target, e))
    }
}
fn user() -> Result<String, String> {
    let mut name = [0u16; 512];
    let mut length = name.len() as u32;
    // SAFETY: the buffer has the length passed in.
    if !unsafe { GetUserNameExW(NameSamCompatible, name.as_mut_ptr(), &mut length) } {
        return Err(format!("user name: {}", std::io::Error::last_os_error()));
    }
    Ok(String::from_utf16_lossy(&name[..length as usize]))
}

fn task_xml(user: &str, program: &Path) -> String {
    let escape = |text: &str| {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    let user = escape(user);
    let folder = escape(&program.parent().unwrap_or(Path::new(".")).to_string_lossy());
    let program = escape(&program.to_string_lossy());
    // The delay gives the GPU driver time to come up.
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <Triggers>
    <LogonTrigger><UserId>{user}</UserId><Delay>PT15S</Delay></LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <RestartOnFailure><Interval>PT1M</Interval><Count>3</Count></RestartOnFailure>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{program}</Command>
      <Arguments>--background</Arguments>
      <WorkingDirectory>{folder}</WorkingDirectory>
    </Exec>
  </Actions>
</Task>
"#
    )
}

fn run(arguments: &[&std::ffi::OsStr]) -> Result<Output, String> {
    let output = Command::new(SCHTASKS)
        .args(arguments)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("schtasks: {e}"))?;
    if output.status.success() {
        Ok(output)
    } else {
        let message = String::from_utf8_lossy(&output.stderr);
        Err(format!("schtasks: {}", message.trim()))
    }
}

/// Whether the task exists and is enabled.
fn task_state(name: &str) -> bool {
    run(&[
        "/Query".as_ref(),
        "/TN".as_ref(),
        name.as_ref(),
        "/XML".as_ref(),
    ])
    .is_ok_and(|output| task_enabled(&String::from_utf8_lossy(&output.stdout)))
}

fn task_enabled(xml: &str) -> bool {
    // Triggers have their own <Enabled>; only the one in <Settings> counts. Default: true.
    let settings = xml
        .split_once("<Settings>")
        .and_then(|(_, rest)| rest.split_once("</Settings>"))
        .map_or("", |(settings, _)| settings);
    !settings.contains("<Enabled>false</Enabled>")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_state_comes_from_the_settings_block() {
        let xml = |setting| {
            format!("<Triggers><Enabled>false</Enabled></Triggers><Settings>{setting}</Settings>")
        };
        assert!(task_enabled(&xml("")));
        assert!(!task_enabled(&xml("<Enabled>false</Enabled>")));
    }

    #[test]
    fn task_runs_the_copy_elevated_and_escapes_names() {
        let xml = task_xml(r"PC\a&b", &program());
        assert!(xml.contains(r"<UserId>PC\a&amp;b</UserId>"));
        assert!(xml.contains("<RunLevel>HighestAvailable</RunLevel>"));
        assert!(xml.contains(r"<Command>C:\Program Files\PulseMQTT-Rust\pulsemqtt.exe</Command>"));
        assert!(user().unwrap().contains('\\'));
    }

    #[test]
    fn copy_installs_and_replaces_the_program() {
        let root = std::env::temp_dir().join(format!("pulsemqtt-copy-{}", std::process::id()));
        let (source, to) = (root.join("source.exe"), root.join("to"));
        std::fs::create_dir_all(&root).unwrap();
        for version in [b"v1", b"v2"] {
            std::fs::write(&source, version).unwrap();
            copy_program(&source, &to).unwrap();
            assert_eq!(std::fs::read(to.join("pulsemqtt.exe")).unwrap(), version);
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
