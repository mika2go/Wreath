use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, RECT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, DeleteObject, EndPaint, FillRect, PAINTSTRUCT, UpdateWindow,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DispatchMessageW, FindWindowW,
    GetMessageW, MSG, PostQuitMessage, RegisterClassW, SW_RESTORE, SetForegroundWindow, ShowWindow,
    TranslateMessage, WINDOW_EX_STYLE, WM_DESTROY, WM_NCCREATE, WM_PAINT, WNDCLASSW,
    WS_OVERLAPPEDWINDOW,
};
use windows::core::{PCWSTR, w};

const WINDOW_CLASS: windows::core::PCWSTR = w!("WreathApplicationWindow");

pub fn run() -> Result<(), String> {
    let single_instance = unsafe {
        windows::Win32::System::Threading::CreateMutexW(None, false, w!("Local\\WreathApplication"))
    }
    .map_err(|error| error.to_string())?;
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        activate_existing_window();
        let _ = unsafe { CloseHandle(single_instance) };
        return Ok(());
    }

    let class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        lpszClassName: WINDOW_CLASS,
        ..Default::default()
    };
    if unsafe { RegisterClassW(&class) } == 0 {
        let _ = unsafe { CloseHandle(single_instance) };
        return Err(std::io::Error::last_os_error().to_string());
    }

    let window = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            WINDOW_CLASS,
            w!("Wreath"),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1280,
            760,
            None,
            None,
            None,
            None,
        )
    }
    .map_err(|error| error.to_string())?;
    unsafe {
        let _ = ShowWindow(window, SW_RESTORE);
        let _ = UpdateWindow(window);
    }

    let mut message = MSG::default();
    while unsafe { GetMessageW(&mut message, None, 0, 0) }.as_bool() {
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    let _ = unsafe { CloseHandle(single_instance) };
    Ok(())
}

fn activate_existing_window() {
    let Ok(window) = (unsafe { FindWindowW(WINDOW_CLASS, PCWSTR::null()) }) else {
        return;
    };
    if window.is_invalid() {
        return;
    }
    unsafe {
        let _ = ShowWindow(window, SW_RESTORE);
        let _ = SetForegroundWindow(window);
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCCREATE => {
            let create = lparam.0 as *const CREATESTRUCTW;
            if create.is_null() {
                LRESULT(0)
            } else {
                LRESULT(1)
            }
        }
        WM_PAINT => {
            let mut paint = PAINTSTRUCT::default();
            let device = unsafe { BeginPaint(window, &mut paint) };
            let brush =
                unsafe { CreateSolidBrush(windows::Win32::Foundation::COLORREF(0x000f0d0d)) };
            let client = RECT {
                left: paint.rcPaint.left,
                top: paint.rcPaint.top,
                right: paint.rcPaint.right,
                bottom: paint.rcPaint.bottom,
            };
            unsafe {
                FillRect(device, &client, brush);
                let _ = DeleteObject(brush.into());
                let _ = EndPaint(window, &paint);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

pub fn show_error(message: &str) {
    use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};

    let message = wide(message);
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(message.as_ptr()),
            w!("Wreath failed to start"),
            MB_OK | MB_ICONERROR,
        );
    }
}
