use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_BINARY, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW,
    RegSetKeyValueW,
};
use windows::core::w;

const RUN_KEY: windows::core::PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
/// Task Manager's startup tab and the Settings startup page disable an entry
/// here instead of deleting it, and an entry disabled this way never runs no
/// matter how correct the `Run` value looks.
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

/// Windows does nothing at all with a startup entry whose executable is not
/// where it says, and reports nothing either: the switch in Wreath still reads
/// as on while nothing starts at the next logon. An installation that moved is
/// exactly that case, so the entry is pointed back at the tray next to this
/// executable. Only an entry that already exists is rewritten, and the approval
/// Windows keeps separately is left untouched, so this never turns on an
/// autostart that was not asked for.
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

/// Windows stores the state in the first byte and sets its low bit to disable.
fn approval_is_disabled(approval: &[u8]) -> bool {
    approval.first().is_some_and(|state| state & 1 != 0)
}

fn command_is_enabled(command: &str) -> bool {
    !command.trim().is_empty()
}

fn autostart_command(tray: &std::path::Path) -> String {
    format!("\"{}\"", tray.display())
}

/// Windows paths differ only in case, and the value Wreath wrote itself is the
/// one that compares equal, so a repaired entry is not rewritten again.
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
        // Without this the entry stays switched off and turning it on in Wreath
        // looks like it worked while nothing starts at the next logon. Windows
        // treats a missing value as approved.
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
        tray_executable,
    };
    use std::path::Path;

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

    /// An entry left behind by an installation somewhere else starts nothing,
    /// while Wreath keeps reporting autostart as enabled.
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
