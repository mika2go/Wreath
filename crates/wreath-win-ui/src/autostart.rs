use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_BINARY, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW,
    RegSetKeyValueW,
};
use windows::core::w;

const RUN_KEY: windows::core::PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const APPROVAL_KEY: windows::core::PCWSTR =
    w!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run");
const VALUE_NAME: windows::core::PCWSTR = w!("Wreath");

pub fn is_enabled() -> bool {
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

pub fn repair() -> Result<bool, String> {
    let Some(command) = run_command().filter(|command| command_is_enabled(command)) else {
        return Ok(false);
    };
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let expected = autostart_command(&tray_executable(&executable));
    if commands_match(&command, &expected) {
        return Ok(false);
    }
    write_run_command(&expected)?;
    Ok(true)
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

fn autostart_command(tray: &std::path::Path) -> String {
    format!("\"{}\"", tray.display())
}

fn commands_match(current: &str, expected: &str) -> bool {
    current.trim().eq_ignore_ascii_case(expected.trim())
}

fn write_run_command(command: &str) -> Result<(), String> {
    let wide = command.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let byte_length = u32::try_from(wide.len().saturating_mul(size_of::<u16>()))
        .map_err(|_| "autostart command is too long".to_owned())?;
    let result = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            VALUE_NAME,
            REG_SZ.0,
            Some(wide.as_ptr().cast()),
            byte_length,
        )
    };
    result
        .ok()
        .map_err(|error| format!("cannot enable Wreath autostart: {error}"))
}

pub fn set_enabled(enabled: bool) -> Result<(), String> {
    if enabled {
        let executable = std::env::current_exe().map_err(|error| error.to_string())?;
        write_run_command(&autostart_command(&tray_executable(&executable)))?;
        let _ = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, APPROVAL_KEY, VALUE_NAME) };
        Ok(())
    } else {
        let result = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, VALUE_NAME) };
        if result == ERROR_SUCCESS || result == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            Err(format!(
                "cannot disable Wreath autostart: {}",
                windows::core::Error::from_hresult(result.to_hresult())
            ))
        }
    }
}

const TASK_NAME: &str = "Wreath elevated autostart";

pub fn elevated_is_enabled() -> bool {
    query_task().is_some_and(|code| code == 0)
}

pub fn set_elevated(enabled: bool) -> Result<(), String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let tray = tray_executable(&executable);
    let parameters = if enabled {
        create_parameters(&tray, &current_user())
    } else {
        delete_parameters()
    };
    let code = run_elevated("schtasks.exe", &parameters)?;
    if code != 0 {
        return Err(format!(
            "Windows refused the scheduled task for the elevated autostart (schtasks reported {code})"
        ));
    }
    if enabled {
        let _ = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, VALUE_NAME) };
    } else {
        write_run_command(&autostart_command(&tray))?;
        let _ = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, APPROVAL_KEY, VALUE_NAME) };
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

fn query_task() -> Option<i32> {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    Command::new("schtasks.exe")
        .args(["/Query", "/TN", TASK_NAME])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .ok()
        .map(|status| status.code().unwrap_or(-1))
}

fn run_elevated(file: &str, parameters: &str) -> Result<i32, String> {
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{GetExitCodeProcess, INFINITE, WaitForSingleObject};
    use windows::Win32::UI::Shell::{
        SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
    };
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
    use windows::core::PCWSTR;

    let file = wide(file);
    let parameters = wide(parameters);
    let verb = wide("runas");
    let mut information = SHELLEXECUTEINFOW {
        cbSize: u32::try_from(size_of::<SHELLEXECUTEINFOW>()).unwrap_or_default(),
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(parameters.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    unsafe { ShellExecuteExW(&mut information) }.map_err(|error| {
        format!("the administrator prompt was declined or could not be shown: {error}")
    })?;
    if information.hProcess.is_invalid() {
        return Err("Windows started no elevated process".to_owned());
    }
    let waited = unsafe { WaitForSingleObject(information.hProcess, INFINITE) };
    let mut code = 0_u32;
    let read = unsafe { GetExitCodeProcess(information.hProcess, &mut code) };
    let _ = unsafe { CloseHandle(information.hProcess) };
    if waited != WAIT_OBJECT_0 || read.is_err() {
        return Err("the elevated task command did not report a result".to_owned());
    }
    Ok(code as i32)
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
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
        approval_is_disabled, autostart_command, command_is_enabled, commands_match,
        create_parameters, delete_parameters, tray_executable,
    };
    use std::path::Path;

    #[test]
    fn the_elevated_task_runs_the_tray_at_logon_with_the_highest_rights() {
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
    fn a_startup_entry_from_another_installation_is_rewritten() {
        let current = autostart_command(&tray_executable(Path::new(
            r"C:\Users\Mika\AppData\Local\Wreath\wreath-tray.exe",
        )));

        assert!(commands_match(
            r#" "c:\users\mika\appdata\local\wreath\wreath-tray.exe" "#,
            &current
        ));
        assert!(!commands_match(
            r#""C:\Program Files\Wreath\wreath-tray.exe""#,
            &current
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
