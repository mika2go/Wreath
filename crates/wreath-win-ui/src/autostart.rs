use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, RRF_RT_REG_BINARY, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW,
};
use windows::core::w;

const RUN_KEY: windows::core::PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const APPROVAL_KEY: windows::core::PCWSTR =
    w!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run");
const VALUE_NAME: windows::core::PCWSTR = w!("Wreath");
const TASK_NAME: &str = "Wreath elevated autostart";

pub fn is_enabled() -> bool {
    task_is_registered() || legacy_entry_is_enabled()
}

pub fn set_enabled(enabled: bool) -> Result<(), String> {
    if enabled {
        register_task()?;
    } else {
        unregister_task()?;
    }
    remove_run_entry();
    Ok(())
}

/// Wreath runs elevated, and Windows starts no elevated executable from the
/// `Run` key, so the entry the installer writes becomes the logon task the
/// first time the tray comes up.
pub fn repair() -> Result<bool, String> {
    let Some(command) = run_command() else {
        return Ok(false);
    };
    if !command_is_enabled(&command) || approval_withdrawn() {
        remove_run_entry();
        return Ok(false);
    }
    register_task()?;
    remove_run_entry();
    Ok(true)
}

fn legacy_entry_is_enabled() -> bool {
    run_command().is_some_and(|command| command_is_enabled(&command)) && !approval_withdrawn()
}

fn run_command() -> Option<String> {
    unsafe {
        let mut byte_length = 0_u32;
        if RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            VALUE_NAME,
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut byte_length),
        ) != ERROR_SUCCESS
            || byte_length <= size_of::<u16>() as u32
        {
            return None;
        }
        let mut command = vec![0_u16; byte_length.div_ceil(size_of::<u16>() as u32) as usize];
        if RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            VALUE_NAME,
            RRF_RT_REG_SZ,
            None,
            Some(command.as_mut_ptr().cast()),
            Some(&mut byte_length),
        ) != ERROR_SUCCESS
        {
            return None;
        }
        let end = command
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(command.len());
        Some(String::from_utf16_lossy(&command[..end]))
    }
}

fn approval_withdrawn() -> bool {
    let mut approval = [0_u8; 12];
    let mut byte_length = u32::try_from(approval.len()).unwrap_or(0);
    let read = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            APPROVAL_KEY,
            VALUE_NAME,
            RRF_RT_REG_BINARY,
            None,
            Some(approval.as_mut_ptr().cast()),
            Some(&mut byte_length),
        )
    };
    if read != ERROR_SUCCESS {
        return false;
    }
    approval_is_disabled(&approval[..approval.len().min(byte_length as usize)])
}

fn approval_is_disabled(approval: &[u8]) -> bool {
    approval.first().is_some_and(|state| state & 1 != 0)
}

fn command_is_enabled(command: &str) -> bool {
    !command.trim().is_empty()
}

fn remove_run_entry() {
    let _ = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, VALUE_NAME) };
    let _ = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, APPROVAL_KEY, VALUE_NAME) };
}

fn register_task() -> Result<(), String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let tray = tray_executable(&executable);
    let code = run_schtasks(&create_parameters(&tray, &current_user()))?;
    if code != 0 {
        return Err(format!(
            "Windows refused the logon task for the autostart (schtasks reported {code})"
        ));
    }
    Ok(())
}

fn unregister_task() -> Result<(), String> {
    if !task_is_registered() {
        return Ok(());
    }
    let code = run_schtasks(&delete_parameters())?;
    if code != 0 {
        return Err(format!(
            "Windows kept the logon task for the autostart (schtasks reported {code})"
        ));
    }
    Ok(())
}

fn create_parameters(tray: &std::path::Path, user: &str) -> String {
    format!(
        "/Create /TN \"{TASK_NAME}\" /TR \"\\\"{}\\\"\" /SC ONLOGON /RU \"{user}\" /RL HIGHEST /F",
        tray.display()
    )
}

fn delete_parameters() -> String {
    format!("/Delete /TN \"{TASK_NAME}\" /F")
}

fn current_user() -> String {
    let name = std::env::var("USERNAME").unwrap_or_default();
    match std::env::var("USERDOMAIN") {
        Ok(domain) if !domain.is_empty() && !name.is_empty() => format!("{domain}\\{name}"),
        _ => name,
    }
}

fn task_is_registered() -> bool {
    run_schtasks(&format!("/Query /TN \"{TASK_NAME}\"")).is_ok_and(|code| code == 0)
}

fn run_schtasks(parameters: &str) -> Result<i32, String> {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    Command::new("schtasks.exe")
        .raw_arg(parameters)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|error| format!("cannot run the Windows task scheduler: {error}"))
        .map(|status| status.code().unwrap_or(-1))
}

fn tray_executable(current_executable: &std::path::Path) -> std::path::PathBuf {
    current_executable
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("wreath-tray.exe")
}

#[cfg(test)]
mod tests {
    use super::{
        approval_is_disabled, command_is_enabled, create_parameters, delete_parameters,
        tray_executable,
    };
    use std::path::Path;

    #[test]
    fn the_logon_task_runs_the_tray_with_the_highest_rights() {
        assert_eq!(
            create_parameters(
                Path::new(r"C:\Program Files\Wreath\wreath-tray.exe"),
                r"DESK\mika"
            ),
            concat!(
                r#"/Create /TN "Wreath elevated autostart" "#,
                r#"/TR "\"C:\Program Files\Wreath\wreath-tray.exe\"" "#,
                r#"/SC ONLOGON /RU "DESK\mika" /RL HIGHEST /F"#
            )
        );
        assert_eq!(
            delete_parameters(),
            r#"/Delete /TN "Wreath elevated autostart" /F"#
        );
    }

    #[test]
    fn a_startup_entry_switched_off_in_windows_counts_as_disabled() {
        assert!(approval_is_disabled(&[
            0x03, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
        ]));
        assert!(!approval_is_disabled(&[
            0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
        ]));
        assert!(!approval_is_disabled(&[]));
    }

    #[test]
    fn installer_placeholder_does_not_enable_autostart() {
        assert!(!command_is_enabled(""));
        assert!(!command_is_enabled(" "));
    }

    #[test]
    fn executable_command_enables_autostart() {
        assert!(command_is_enabled(
            r#""C:\Program Files\Wreath\wreath-tray.exe""#
        ));
    }

    #[test]
    fn autostart_always_targets_the_tray_sibling() {
        assert_eq!(
            tray_executable(Path::new(r"C:\Program Files\Wreath\wreath-win-ui.exe")),
            Path::new(r"C:\Program Files\Wreath\wreath-tray.exe")
        );
        assert_eq!(
            tray_executable(Path::new(r"C:\Program Files\Wreath\wreath-tray.exe")),
            Path::new(r"C:\Program Files\Wreath\wreath-tray.exe")
        );
    }
}
