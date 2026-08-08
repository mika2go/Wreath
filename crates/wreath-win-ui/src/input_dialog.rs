use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateFontW, CreateSolidBrush, DEFAULT_CHARSET,
    DT_CENTER, DT_SINGLELINE, DT_VCENTER, DeleteObject, DrawTextW, FillRect, HBRUSH, HDC, HFONT,
    HGDIOBJ, OUT_TT_PRECIS, SelectObject, SetBkColor, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows::Win32::UI::Controls::{DRAWITEMSTRUCT, EM_SETSEL, ODS_SELECTED};
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow};
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::{
    BS_OWNERDRAW, CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    ES_AUTOHSCROLL, GWLP_USERDATA, GetMessageW, GetWindowLongPtrW, GetWindowRect,
    GetWindowTextLengthW, GetWindowTextW, IDC_ARROW, IsDialogMessageW, LoadCursorW, MSG,
    PostQuitMessage, RegisterClassW, SW_SHOW, SendMessageW, SetForegroundWindow, SetWindowLongPtrW,
    ShowWindow, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_COMMAND,
    WM_CTLCOLOREDIT, WM_CTLCOLORSTATIC, WM_DRAWITEM, WM_NCCREATE, WM_NCDESTROY, WM_SETFONT,
    WNDCLASSW, WS_CAPTION, WS_CHILD, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
};
use windows::core::{PCWSTR, w};

const DIALOG_CLASS: windows::core::PCWSTR = w!("WreathInputDialog");
const ID_OK: usize = 1;
const ID_CANCEL: usize = 2;

const STAGE: u32 = 0x101012;
const SURFACE: u32 = 0x17171a;
const SURFACE_HOVER: u32 = 0x202024;
const PRIMARY: u32 = 0xf4f5f9;
const SECONDARY: u32 = 0x777e8e;

const DIALOG_WIDTH: i32 = 420;
const DIALOG_HEIGHT: i32 = 168;
const MARGIN: i32 = 20;
const BUTTON_WIDTH: i32 = 88;
const BUTTON_HEIGHT: i32 = 34;

struct DialogState {
    edit: Option<HWND>,
    font: HFONT,
    stage: HBRUSH,
    surface: HBRUSH,
    done: bool,
    value: Option<String>,
}

pub fn prompt(
    owner: HWND,
    title: &str,
    label: &str,
    initial: &str,
    confirm: &str,
) -> Result<Option<String>, String> {
    register_class()?;
    let dpi = unsafe { GetDpiForWindow(owner) }.max(96);
    let scale = |value: i32| (value as f32 * dpi as f32 / 96.0).round() as i32;
    let style = WS_CAPTION | WS_SYSMENU;

    let mut frame = RECT {
        left: 0,
        top: 0,
        right: scale(DIALOG_WIDTH),
        bottom: scale(DIALOG_HEIGHT),
    };
    let _ = unsafe {
        AdjustWindowRectExForDpi(&mut frame, style, false, WINDOW_EX_STYLE::default(), dpi)
    };
    let width = frame.right - frame.left;
    let height = frame.bottom - frame.top;
    let mut owner_rect = RECT::default();
    unsafe { GetWindowRect(owner, &mut owner_rect) }.map_err(|error| error.to_string())?;
    let x = owner_rect.left + (owner_rect.right - owner_rect.left - width) / 2;
    let y = owner_rect.top + (owner_rect.bottom - owner_rect.top - height) / 2;

    let font = unsafe {
        CreateFontW(
            -scale(15),
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_TT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            0,
            w!("Segoe UI"),
        )
    };
    let mut state = Box::new(DialogState {
        edit: None,
        font,
        stage: unsafe { CreateSolidBrush(colorref(STAGE)) },
        surface: unsafe { CreateSolidBrush(colorref(SURFACE)) },
        done: false,
        value: None,
    });

    let title = wide(title);
    let window = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            DIALOG_CLASS,
            PCWSTR(title.as_ptr()),
            style,
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
    let confirm = wide(confirm);
    let buttons_top = scale(DIALOG_HEIGHT - MARGIN - BUTTON_HEIGHT);
    let cancel_left = scale(DIALOG_WIDTH - MARGIN - BUTTON_WIDTH);
    let ok_left = cancel_left - scale(BUTTON_WIDTH + 8);
    let created = (|| -> windows::core::Result<HWND> {
        unsafe {
            let static_text = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                PCWSTR(label.as_ptr()),
                WS_CHILD | WS_VISIBLE,
                scale(MARGIN),
                scale(18),
                scale(DIALOG_WIDTH - 2 * MARGIN),
                scale(20),
                Some(window),
                None,
                None,
                None,
            )?;
            let edit = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("EDIT"),
                PCWSTR(initial.as_ptr()),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
                scale(MARGIN),
                scale(46),
                scale(DIALOG_WIDTH - 2 * MARGIN),
                scale(32),
                Some(window),
                None,
                None,
                None,
            )?;
            let ok = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("BUTTON"),
                PCWSTR(confirm.as_ptr()),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32),
                ok_left,
                buttons_top,
                scale(BUTTON_WIDTH),
                scale(BUTTON_HEIGHT),
                Some(window),
                Some(windows::Win32::UI::WindowsAndMessaging::HMENU(
                    ID_OK as *mut _,
                )),
                None,
                None,
            )?;
            let cancel = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("BUTTON"),
                w!("Cancel"),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32),
                cancel_left,
                buttons_top,
                scale(BUTTON_WIDTH),
                scale(BUTTON_HEIGHT),
                Some(window),
                Some(windows::Win32::UI::WindowsAndMessaging::HMENU(
                    ID_CANCEL as *mut _,
                )),
                None,
                None,
            )?;
            for control in [static_text, edit, ok, cancel] {
                SendMessageW(
                    control,
                    WM_SETFONT,
                    Some(WPARAM(font.0 as usize)),
                    Some(LPARAM(1)),
                );
            }
            state.edit = Some(edit);
            SendMessageW(edit, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
            Ok(edit)
        }
    })();
    let edit = match created {
        Ok(edit) => edit,
        Err(error) => {
            let _ = unsafe { DestroyWindow(window) };
            release(&state);
            return Err(error.to_string());
        }
    };

    let _ = unsafe { EnableWindow(owner, false) };
    unsafe {
        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetFocus(Some(edit));
    }

    let mut message = MSG::default();
    loop {
        if state.done {
            break;
        }
        let result = unsafe { GetMessageW(&mut message, None, 0, 0) };
        if result.0 == 0 {
            unsafe { PostQuitMessage(message.wParam.0 as i32) };
            break;
        }
        if result.0 == -1 {
            break;
        }
        if !unsafe { IsDialogMessageW(window, &message) }.as_bool() {
            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }

    let _ = unsafe { EnableWindow(owner, true) };
    let _ = unsafe { SetForegroundWindow(owner) };
    if !state.done {
        let _ = unsafe { DestroyWindow(window) };
    }
    let value = state.value.take();
    release(&state);
    Ok(value)
}

fn release(state: &DialogState) {
    unsafe {
        let _ = DeleteObject(HGDIOBJ::from(state.font));
        let _ = DeleteObject(HGDIOBJ::from(state.stage));
        let _ = DeleteObject(HGDIOBJ::from(state.surface));
    }
}

fn register_class() -> Result<(), String> {
    static REGISTERED: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
    REGISTERED
        .get_or_init(|| {
            let class = WNDCLASSW {
                lpfnWndProc: Some(dialog_proc),
                lpszClassName: DIALOG_CLASS,
                hbrBackground: unsafe { CreateSolidBrush(colorref(STAGE)) },
                hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default(),
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
        WM_CTLCOLORSTATIC => {
            let device = HDC(wparam.0 as *mut _);
            unsafe {
                SetTextColor(device, colorref(SECONDARY));
                SetBkColor(device, colorref(STAGE));
            }
            match state_mut(window) {
                Some(state) => LRESULT(state.stage.0 as isize),
                None => unsafe { DefWindowProcW(window, message, wparam, lparam) },
            }
        }
        WM_CTLCOLOREDIT => {
            let device = HDC(wparam.0 as *mut _);
            unsafe {
                SetTextColor(device, colorref(PRIMARY));
                SetBkColor(device, colorref(SURFACE));
            }
            match state_mut(window) {
                Some(state) => LRESULT(state.surface.0 as isize),
                None => unsafe { DefWindowProcW(window, message, wparam, lparam) },
            }
        }
        WM_DRAWITEM => {
            let item = lparam.0 as *const DRAWITEMSTRUCT;
            if item.is_null() {
                return LRESULT(0);
            }
            let font = state_mut(window).map(|state| state.font);
            unsafe { draw_button(&*item, font) };
            LRESULT(1)
        }
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
        WM_NCDESTROY => {
            unsafe { SetWindowLongPtrW(window, GWLP_USERDATA, 0) };
            unsafe { DefWindowProcW(window, message, wparam, lparam) }
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

unsafe fn draw_button(item: &DRAWITEMSTRUCT, font: Option<HFONT>) {
    let pressed = item.itemState.0 & ODS_SELECTED.0 != 0;
    let primary = item.CtlID as usize == ID_OK;
    let (fill, text) = match (primary, pressed) {
        (true, false) => (PRIMARY, STAGE),
        (true, true) => (SECONDARY, STAGE),
        (false, false) => (SURFACE_HOVER, PRIMARY),
        (false, true) => (SURFACE, PRIMARY),
    };
    let brush = unsafe { CreateSolidBrush(colorref(fill)) };
    let mut bounds = item.rcItem;
    unsafe {
        FillRect(item.hDC, &bounds, brush);
        let _ = DeleteObject(brush.into());
        if let Some(font) = font {
            SelectObject(item.hDC, HGDIOBJ::from(font));
        }
        SetBkMode(item.hDC, TRANSPARENT);
        SetTextColor(item.hDC, colorref(text));
    }
    let mut caption = read_text(item.hwndItem)
        .unwrap_or_default()
        .encode_utf16()
        .collect::<Vec<_>>();
    if caption.is_empty() {
        return;
    }
    unsafe {
        DrawTextW(
            item.hDC,
            &mut caption,
            &mut bounds,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        )
    };
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

fn colorref(rgb: u32) -> COLORREF {
    COLORREF(((rgb & 0xff) << 16) | (rgb & 0xff00) | ((rgb >> 16) & 0xff))
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_are_converted_from_rgb_to_win32_bgr() {
        assert_eq!(colorref(0x17171a).0, 0x1a1717);
        assert_eq!(colorref(0xf4f5f9).0, 0xf9f5f4);
        assert_eq!(colorref(0x000000).0, 0x000000);
    }
}
