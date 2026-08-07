use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use windows::core::w;

const RUN_KEY: windows::core::PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const VALUE_NAME: windows::core::PCWSTR = w!("Wreath");

pub fn is_enabled() -> bool {
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
            return false;
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
            return false;
        }
        command_is_enabled(&command)
    }
}

fn command_is_enabled(command: &[u16]) -> bool {
    let end = command
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(command.len());
    !String::from_utf16_lossy(&command[..end]).trim().is_empty()
}

pub fn set_enabled(enabled: bool) -> Result<(), String> {
    if enabled {
        let executable = std::env::current_exe().map_err(|error| error.to_string())?;
        let command = format!("\"{}\"", executable.display());
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

#[cfg(test)]
mod tests {
    use super::command_is_enabled;

    #[test]
    fn installer_placeholder_does_not_enable_autostart() {
        assert!(!command_is_enabled(&[0]));
        assert!(!command_is_enabled(&[' ' as u16, 0]));
    }

    #[test]
    fn executable_command_enables_autostart() {
        let command = r#""C:\Program Files\Wreath\wreath-win-ui.exe""#
            .encode_utf16()
            .chain(Some(0))
            .collect::<Vec<_>>();
        assert!(command_is_enabled(&command));
    }
}
