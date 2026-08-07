use windows::Win32::Foundation::HINSTANCE;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{HICON, IDI_APPLICATION, LoadIconW};
use windows::core::PCWSTR;

const WREATH_ICON_RESOURCE_ID: usize = 1;

pub fn load() -> HICON {
    let embedded = unsafe { GetModuleHandleW(PCWSTR::null()) }
        .ok()
        .and_then(|module| unsafe {
            LoadIconW(
                Some(HINSTANCE(module.0)),
                PCWSTR(WREATH_ICON_RESOURCE_ID as *const u16),
            )
            .ok()
        });
    embedded.unwrap_or_else(|| unsafe { LoadIconW(None, IDI_APPLICATION) }.unwrap_or_default())
}
