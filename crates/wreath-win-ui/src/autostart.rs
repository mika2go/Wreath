use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use windows::core::w;

const RUN_KEY: windows::core::PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const VALUE_NAME: windows::core::PCWSTR = w!("Wreath");

pub fn is_enabled() -> bool {
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            VALUE_NAME,
            RRF_RT_REG_SZ,
            None,
            None,
            None,
        ) == ERROR_SUCCESS
    }
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
