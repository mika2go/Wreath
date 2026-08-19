#[derive(Debug, Clone, PartialEq)]
pub struct DisplayTarget {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub refresh_rate: f64,
    pub primary: bool,
}

pub fn choose_display_index(displays: &[DisplayTarget], configured: Option<&str>) -> Option<usize> {
    if let Some(configured) = configured {
        return displays
            .iter()
            .position(|display| display.name.eq_ignore_ascii_case(configured));
    }
    displays
        .iter()
        .position(|display| display.primary)
        .or_else(|| (!displays.is_empty()).then_some(0))
}

#[cfg(target_os = "windows")]
pub struct NativeDisplay {
    pub target: DisplayTarget,
    pub handle: windows::Win32::Graphics::Gdi::HMONITOR,
}

#[cfg(target_os = "windows")]
pub fn select_display(configured: Option<&str>) -> Result<NativeDisplay, crate::video::VideoError> {
    let displays = enumerate_native_displays()?;
    let targets = displays
        .iter()
        .map(|display| display.target.clone())
        .collect::<Vec<_>>();
    let index = choose_display_index(&targets, configured).ok_or_else(|| {
        crate::video::VideoError::Initialization(match configured {
            Some(name) => format!("configured Windows display was not found: {name}"),
            None => "Windows reported no capture displays".into(),
        })
    })?;
    Ok(displays
        .into_iter()
        .nth(index)
        .expect("selected display exists"))
}

#[cfg(target_os = "windows")]
pub fn monitor_details(
    handle: windows::Win32::Graphics::Gdi::HMONITOR,
) -> Option<(String, u32, u32)> {
    use std::mem::size_of;

    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITORINFO, MONITORINFOEXW};

    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    let read = unsafe {
        GetMonitorInfoW(
            handle,
            (&mut info as *mut MONITORINFOEXW).cast::<MONITORINFO>(),
        )
    };
    if !read.as_bool() {
        return None;
    }
    let end = info
        .szDevice
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(info.szDevice.len());
    let rectangle = info.monitorInfo.rcMonitor;
    Some((
        String::from_utf16_lossy(&info.szDevice[..end]),
        u32::try_from(rectangle.right.saturating_sub(rectangle.left)).unwrap_or_default(),
        u32::try_from(rectangle.bottom.saturating_sub(rectangle.top)).unwrap_or_default(),
    ))
}

#[cfg(target_os = "windows")]
pub fn displays() -> Result<Vec<DisplayTarget>, crate::video::VideoError> {
    enumerate_native_displays()
        .map(|displays| displays.into_iter().map(|display| display.target).collect())
}

#[cfg(target_os = "windows")]
fn enumerate_native_displays() -> Result<Vec<NativeDisplay>, crate::video::VideoError> {
    use std::mem::size_of;

    use windows::Win32::Foundation::{LPARAM, TRUE};
    use windows::Win32::Graphics::Gdi::{
        DEVMODEW, ENUM_CURRENT_SETTINGS, EnumDisplayMonitors, EnumDisplaySettingsW,
        GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
    };
    use windows::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;
    use windows::core::{BOOL, PCWSTR};

    unsafe extern "system" fn collect_monitor(
        monitor: HMONITOR,
        _device_context: HDC,
        _rectangle: *mut windows::Win32::Foundation::RECT,
        state: LPARAM,
    ) -> BOOL {
        let handles = unsafe { &mut *(state.0 as *mut Vec<HMONITOR>) };
        handles.push(monitor);
        TRUE
    }

    let mut handles = Vec::new();
    let enumeration = unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(collect_monitor),
            LPARAM((&mut handles as *mut Vec<HMONITOR>) as isize),
        )
    };
    if !enumeration.as_bool() {
        return Err(crate::video::VideoError::Initialization(
            std::io::Error::last_os_error().to_string(),
        ));
    }

    let mut displays = Vec::with_capacity(handles.len());
    for handle in handles {
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
        let info_result = unsafe {
            GetMonitorInfoW(
                handle,
                (&mut info as *mut MONITORINFOEXW).cast::<MONITORINFO>(),
            )
        };
        if !info_result.as_bool() {
            continue;
        }
        let name_length = info
            .szDevice
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(info.szDevice.len());
        let name = String::from_utf16_lossy(&info.szDevice[..name_length]);
        let rectangle = info.monitorInfo.rcMonitor;
        let mut mode = DEVMODEW {
            dmSize: size_of::<DEVMODEW>() as u16,
            ..Default::default()
        };
        let has_mode = unsafe {
            EnumDisplaySettingsW(
                PCWSTR(info.szDevice.as_ptr()),
                ENUM_CURRENT_SETTINGS,
                &mut mode,
            )
        }
        .as_bool();
        displays.push(NativeDisplay {
            target: DisplayTarget {
                name,
                width: u32::try_from(rectangle.right.saturating_sub(rectangle.left))
                    .unwrap_or_default(),
                height: u32::try_from(rectangle.bottom.saturating_sub(rectangle.top))
                    .unwrap_or_default(),
                refresh_rate: if has_mode && mode.dmDisplayFrequency > 1 {
                    f64::from(mode.dmDisplayFrequency)
                } else {
                    60.0
                },
                primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
            },
            handle,
        });
    }
    Ok(displays)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn display(name: &str, primary: bool) -> DisplayTarget {
        DisplayTarget {
            name: name.into(),
            width: 1920,
            height: 1080,
            refresh_rate: 60.0,
            primary,
        }
    }

    #[test]
    fn default_selection_prefers_the_primary_display() {
        let displays = vec![
            display(r"\\.\DISPLAY1", false),
            display(r"\\.\DISPLAY2", true),
        ];
        assert_eq!(choose_display_index(&displays, None), Some(1));
    }

    #[test]
    fn configured_display_is_strict_and_case_insensitive() {
        let displays = vec![display(r"\\.\DISPLAY1", true)];
        assert_eq!(
            choose_display_index(&displays, Some(r"\\.\display1")),
            Some(0)
        );
        assert_eq!(choose_display_index(&displays, Some("missing")), None);
    }
}
