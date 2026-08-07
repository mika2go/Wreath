use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    BS_DEFPUSHBUTTON, CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow,
    DispatchMessageW, ES_AUTOHSCROLL, GWLP_USERDATA, GetMessageW, GetWindowLongPtrW, GetWindowRect,
    GetWindowTextLengthW, GetWindowTextW, IsDialogMessageW, MSG, RegisterClassW, SW_SHOW,
    SetWindowLongPtrW, ShowWindow, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE,
    WM_COMMAND, WM_NCCREATE, WNDCLASSW, WS_BORDER, WS_CAPTION, WS_CHILD, WS_SYSMENU, WS_TABSTOP,
    WS_VISIBLE,
};
use windows::core::{PCWSTR, w};

const DIALOG_CLASS: windows::core::PCWSTR = w!("WreathInputDialog");
const ID_OK: usize = 1;
const ID_CANCEL: usize = 2;

struct DialogState {
    edit: Option<HWND>,
    done: bool,
    value: Option<String>,
}

pub fn prompt(
    owner: HWND,
    title: &str,
    label: &str,
    initial: &str,
) -> Result<Option<String>, String> {
    register_class()?;
    let mut owner_rect = RECT::default();
    unsafe { GetWindowRect(owner, &mut owner_rect) }.map_err(|error| error.to_string())?;
    let width = 440;
    let height = 178;
    let x = owner_rect.left + (owner_rect.right - owner_rect.left - width) / 2;
    let y = owner_rect.top + (owner_rect.bottom - owner_rect.top - height) / 2;
    let mut state = Box::new(DialogState {
        edit: None,
        done: false,
        value: None,
    });
    let title = wide(title);
    let window = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            DIALOG_CLASS,
            PCWSTR(title.as_ptr()),
            WS_CAPTION | WS_SYSMENU,
            x,
            y,
            width,
            height,
            Some(owner),
            None,
            None,
            Some((&mut *state as *mut DialogState).cast()),
        )
    }
    .map_err(|error| error.to_string())?;
    let label = wide(label);
    let initial = wide(initial);
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("STATIC"),
            PCWSTR(label.as_ptr()),
            WS_CHILD | WS_VISIBLE,
            18,
            16,
            392,
            22,
            Some(window),
            None,
            None,
            None,
        )
        .map_err(|error| error.to_string())?;
        let edit = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("EDIT"),
            PCWSTR(initial.as_ptr()),
            WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
            18,
            44,
            392,
            30,
            Some(window),
            None,
            None,
            None,
        )
        .map_err(|error| error.to_string())?;
        state.edit = Some(edit);
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("BUTTON"),
            w!("OK"),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32),
            236,
            94,
            82,
            30,
            Some(window),
            Some(windows::Win32::UI::WindowsAndMessaging::HMENU(
                ID_OK as *mut _,
            )),
            None,
            None,
        )
        .map_err(|error| error.to_string())?;
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("BUTTON"),
            w!("Cancel"),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            328,
            94,
            82,
            30,
            Some(window),
            Some(windows::Win32::UI::WindowsAndMessaging::HMENU(
                ID_CANCEL as *mut _,
            )),
            None,
            None,
        )
        .map_err(|error| error.to_string())?;
        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetFocus(Some(edit));
    }

    let mut message = MSG::default();
    while !state.done && unsafe { GetMessageW(&mut message, None, 0, 0) }.as_bool() {
        if !unsafe { IsDialogMessageW(window, &message) }.as_bool() {
            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
    if !state.done {
        let _ = unsafe { DestroyWindow(window) };
    }
    Ok(state.value.take())
}

fn register_class() -> Result<(), String> {
    static REGISTERED: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
    REGISTERED
        .get_or_init(|| {
            let class = WNDCLASSW {
                lpfnWndProc: Some(dialog_proc),
                lpszClassName: DIALOG_CLASS,
                ..Default::default()
            };
            if unsafe { RegisterClassW(&class) } == 0 {
                Err(std::io::Error::last_os_error().to_string())
            } else {
                Ok(())
            }
        })
        .clone()
}

unsafe extern "system" fn dialog_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = lparam.0 as *const CREATESTRUCTW;
        if create.is_null() {
            return LRESULT(0);
        }
        unsafe { SetWindowLongPtrW(window, GWLP_USERDATA, (*create).lpCreateParams as isize) };
        return LRESULT(1);
    }
    match message {
        WM_COMMAND => {
            let command = wparam.0 & 0xffff;
            if command == ID_OK {
                if let Some(state) = state_mut(window) {
                    state.value = state.edit.and_then(read_text);
                    state.done = true;
                }
                let _ = unsafe { DestroyWindow(window) };
                LRESULT(0)
            } else if command == ID_CANCEL {
                close(window);
                LRESULT(0)
            } else {
                unsafe { DefWindowProcW(window, message, wparam, lparam) }
            }
        }
        WM_CLOSE => {
            close(window);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

fn close(window: HWND) {
    if let Some(state) = state_mut(window) {
        state.done = true;
        state.value = None;
    }
    let _ = unsafe { DestroyWindow(window) };
}

fn read_text(window: HWND) -> Option<String> {
    let length = unsafe { GetWindowTextLengthW(window) };
    let mut buffer = vec![0_u16; length.max(0) as usize + 1];
    let copied = unsafe { GetWindowTextW(window, &mut buffer) };
    (copied >= 0).then(|| String::from_utf16_lossy(&buffer[..copied as usize]))
}

fn state_mut(window: HWND) -> Option<&'static mut DialogState> {
    let pointer = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) } as *mut DialogState;
    unsafe { pointer.as_mut() }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
