use std::path::Path;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, GlobalFree, HANDLE, HGLOBAL, HWND, LPARAM,
    LRESULT, RECT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO,
    MonitorFromWindow, PAINTSTRUCT, UpdateWindow,
};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW,
    DispatchMessageW, FindWindowW, GWL_STYLE, GWLP_USERDATA, GetClientRect, GetMessageW,
    GetWindowLongPtrW, GetWindowPlacement, IDC_ARROW, LoadCursorW, MINMAXINFO, MSG,
    PostQuitMessage, RegisterClassW, SIZE_MINIMIZED, SW_HIDE, SW_RESTORE, SW_SHOW,
    SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetForegroundWindow,
    SetTimer, SetWindowLongPtrW, SetWindowPlacement, SetWindowPos, ShowWindow, TranslateMessage,
    WINDOW_EX_STYLE, WINDOWPLACEMENT, WM_CHAR, WM_DESTROY, WM_DPICHANGED, WM_GETMINMAXINFO,
    WM_KEYDOWN, WM_KEYUP, WM_KILLFOCUS, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCCREATE,
    WM_PAINT, WM_RBUTTONUP, WM_SIZE, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_TIMER, WNDCLASSW, WS_CHILD,
    WS_DISABLED, WS_OVERLAPPEDWINDOW, WS_POPUP, WS_VISIBLE,
};
use windows::core::{PCWSTR, w};
use wreath_core::config::Codec;
use wreath_core::ipc::{Request, Response};

use crate::model::{
    Action, ClipContextMenu, DeleteTarget, DisplayOption, NoticeExpiry, PromptKind, SettingsMenu,
    SettingsMenuItem, SettingsMenuKind, TextInput, UiModel,
};
use crate::player::{PLAYER_EVENT, Player};
use crate::renderer::{
    Renderer, editor_player_bounds, editor_timeline_fraction, editor_timeline_rail, player_bounds,
    player_timeline_rail, player_volume_rail,
};

const WINDOW_CLASS: windows::core::PCWSTR = w!("WreathApplicationWindow");
const CF_UNICODETEXT_FORMAT: u32 = 13;
const PLAYER_TIMER: usize = 2;
const WM_MOUSELEAVE_MESSAGE: u32 = 0x02a3;
const PLAYER_SEEK_INTERVAL: Duration = Duration::from_millis(50);
const PREVIEW_SEEK_INTERVAL: Duration = Duration::from_millis(33);
const PREVIEW_SEEK_SETTLE: Duration = Duration::from_millis(160);
const PREVIEW_SLACK_SECONDS: f64 = 0.05;
const NOTICE_LIFETIME: Duration = Duration::from_secs(3);

struct AppState {
    model: UiModel,
    renderer: Renderer,
    width: u32,
    height: u32,
    dpi: u32,
    player: Option<Player>,
    video_window: Option<HWND>,
    text_drag: Option<TextDrag>,
    trim_updates: mpsc::Receiver<TrimUpdate>,
    trim_sender: mpsc::Sender<TrimUpdate>,
    hotkey_updates: mpsc::Receiver<HotkeyUpdate>,
    hotkey_sender: mpsc::Sender<HotkeyUpdate>,
    editor_drag: Option<EditorDrag>,
    slider_drag: Option<SliderDrag>,
    player_seek: Option<Instant>,
    preview_seek: Option<Instant>,
    mouse_tracking: bool,
    fullscreen: Option<FullscreenState>,
    notice_expiry: NoticeExpiry,
}

struct FullscreenState {
    style: isize,
    placement: WINDOWPLACEMENT,
}

#[derive(Clone, Copy)]
enum EditorDrag {
    Start,
    End,
}

#[derive(Clone, Copy)]
enum SliderDrag {
    PlayerSeek,
    PlayerVolume,
    EditorPlayhead,
}

#[derive(Clone, Copy)]
enum TextDrag {
    Search,
    Prompt,
}

enum TrimUpdate {
    Timing {
        source: PathBuf,
        result: Result<wreath_core::trim::ClipTiming, String>,
    },
    Finished {
        source: PathBuf,
        replacing: bool,
        result: Result<wreath_core::trim::TrimReport, String>,
    },
}

struct HotkeyUpdate {
    hotkey: wreath_core::config::HotkeyConfig,
    result: Result<HotkeyActivation, String>,
}

enum HotkeyActivation {
    Active,
    SavedForNextStart,
}

pub fn run() -> Result<(), String> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        CoInitializeEx(None, COINIT_APARTMENTTHREADED)
            .ok()
            .map_err(|error| error.to_string())?;
    }
    let result = run_initialized();
    unsafe { CoUninitialize() };
    result
}

fn run_initialized() -> Result<(), String> {
    let single_instance = unsafe {
        windows::Win32::System::Threading::CreateMutexW(None, false, w!("Local\\WreathApplication"))
    }
    .map_err(|error| error.to_string())?;
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        activate_existing_window();
        let _ = unsafe { CloseHandle(single_instance) };
        return Ok(());
    }
    ensure_tray()?;

    let icon = crate::icon::load();
    let class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        lpszClassName: WINDOW_CLASS,
        style: CS_HREDRAW | CS_VREDRAW,
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default(),
        hIcon: icon,
        ..Default::default()
    };
    if unsafe { RegisterClassW(&class) } == 0 {
        let _ = unsafe { CloseHandle(single_instance) };
        return Err(std::io::Error::last_os_error().to_string());
    }

    let mut model = UiModel::load()?;
    model.autostart_enabled = crate::autostart::is_enabled();
    refresh_displays(&mut model);
    refresh_microphones(&mut model);
    let (trim_sender, trim_updates) = mpsc::channel();
    let (hotkey_sender, hotkey_updates) = mpsc::channel();
    let state = Box::new(AppState {
        model,
        renderer: Renderer::new()?,
        width: 1440,
        height: 900,
        dpi: 96,
        player: None,
        video_window: None,
        text_drag: None,
        trim_updates,
        trim_sender,
        hotkey_updates,
        hotkey_sender,
        editor_drag: None,
        slider_drag: None,
        player_seek: None,
        preview_seek: None,
        mouse_tracking: false,
        fullscreen: None,
        notice_expiry: NoticeExpiry::default(),
    });
    let state = Box::into_raw(state);
    let window = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            WINDOW_CLASS,
            w!("Wreath"),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1440,
            900,
            None,
            None,
            None,
            Some(state.cast()),
        )
    };
    let window = match window {
        Ok(window) => window,
        Err(error) => {
            let _ = unsafe { Box::from_raw(state) };
            let _ = unsafe { CloseHandle(single_instance) };
            return Err(error.to_string());
        }
    };
    let video_window = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("STATIC"),
            w!(""),
            WS_CHILD | WS_DISABLED,
            0,
            0,
            0,
            0,
            Some(window),
            None,
            None,
            None,
        )
    }
    .map_err(|error| error.to_string())?;
    unsafe {
        (*state).video_window = Some(video_window);
        match Player::new(video_window, window) {
            Ok(player) => (*state).player = Some(player),
            Err(error) => {
                (*state).model.notice = Some(format!("Clip player unavailable: {error}"));
            }
        }
        (*state).dpi = GetDpiForWindow(window).max(96);
        // Keep the native playhead visually continuous without tying playback
        // timing to paint events. Media Foundation remains the source of truth.
        let _ = SetTimer(Some(window), PLAYER_TIMER, 33, None);
    }
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
    let _ = unsafe { Box::from_raw(state) };
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
    if message == WM_NCCREATE {
        let create = lparam.0 as *const CREATESTRUCTW;
        if create.is_null() {
            return LRESULT(0);
        }
        unsafe { SetWindowLongPtrW(window, GWLP_USERDATA, (*create).lpCreateParams as isize) };
        return LRESULT(1);
    }
    match message {
        wreath_windows::feedback::CLIP_SAVED_MESSAGE => {
            if let Some(state) = state_mut(window) {
                let result = state.model.refresh();
                if result.is_ok() {
                    state.renderer.retry_unavailable_thumbnails();
                } else if let Err(error) = result {
                    state.model.notice = Some(error);
                }
                redraw(window);
            }
            LRESULT(0)
        }
        WM_SIZE => {
            if let Some(state) = state_mut(window) {
                if wparam.0 as u32 == SIZE_MINIMIZED {
                    // Nothing is on screen, so the decoded thumbnails are pure
                    // footprint until the window comes back.
                    state.renderer.release_cached_images();
                    return LRESULT(0);
                }
                state.width = low_word(lparam.0) as u32;
                state.height = high_word(lparam.0) as u32;
                if state.width > 0 && state.height > 0 {
                    state.renderer.resize(state.width, state.height);
                    update_player_window(state);
                    redraw(window);
                }
            }
            LRESULT(0)
        }
        WM_GETMINMAXINFO => {
            let info = lparam.0 as *mut MINMAXINFO;
            if !info.is_null() {
                let scale = state_mut(window).map_or(1.0, |state| state.dpi as f32 / 96.0);
                unsafe {
                    (*info).ptMinTrackSize.x = (980.0 * scale).round() as i32;
                    (*info).ptMinTrackSize.y = (680.0 * scale).round() as i32;
                }
            }
            LRESULT(0)
        }
        WM_DPICHANGED => {
            if let Some(state) = state_mut(window) {
                state.dpi = low_word(wparam.0 as isize).max(96) as u32;
            }
            let recommended = lparam.0 as *const RECT;
            if !recommended.is_null() {
                let recommended = unsafe { &*recommended };
                let _ = unsafe {
                    SetWindowPos(
                        window,
                        None,
                        recommended.left,
                        recommended.top,
                        recommended.right - recommended.left,
                        recommended.bottom - recommended.top,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    )
                };
            }
            LRESULT(0)
        }
        WM_PAINT => {
            let mut paint = PAINTSTRUCT::default();
            unsafe { BeginPaint(window, &mut paint) };
            if let Some(state) = state_mut(window) {
                let mut client = RECT::default();
                let _ = unsafe { GetClientRect(window, &mut client) };
                state.width = client.right.max(0) as u32;
                state.height = client.bottom.max(0) as u32;
                let scale = state.dpi as f32 / 96.0;
                let painted = state.renderer.paint(
                    window,
                    &state.model,
                    ((state.width as f32 / scale).round() as u32).max(1),
                    ((state.height as f32 / scale).round() as u32).max(1),
                );
                if let Err(error) = painted
                    && state.renderer.is_failing()
                {
                    state.model.notice = Some(format!("Rendering failed: {error}"));
                }
                if state.renderer.wants_recovery_repaint() {
                    redraw(window);
                }
            }
            let _ = unsafe { EndPaint(window, &paint) };
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            if let Some(state) = state_mut(window) {
                let scale = state.dpi as f32 / 96.0;
                let x = signed_low_word(lparam.0) as f32 / scale;
                let y = signed_high_word(lparam.0) as f32 / scale;
                let hit = state.renderer.hit_test(x, y);
                state.editor_drag = match hit.as_ref() {
                    Some(Action::DragEditorStart) => Some(EditorDrag::Start),
                    Some(Action::DragEditorEnd) => Some(EditorDrag::End),
                    _ => None,
                };
                state.slider_drag = match hit.as_ref() {
                    Some(Action::DragPlayerSeek) => Some(SliderDrag::PlayerSeek),
                    Some(Action::DragPlayerVolume) => Some(SliderDrag::PlayerVolume),
                    Some(Action::DragEditorPlayhead) => Some(SliderDrag::EditorPlayhead),
                    _ => None,
                };
                state.text_drag = match hit.as_ref() {
                    Some(Action::PlaceSearchCaret(position)) => {
                        state.model.search_focused = true;
                        state.model.search.move_caret(*position, key_pressed(0x10));
                        Some(TextDrag::Search)
                    }
                    Some(Action::PlacePromptCaret(position)) => {
                        if let Some(prompt) = &mut state.model.prompt {
                            prompt.input.move_caret(*position, key_pressed(0x10));
                        }
                        Some(TextDrag::Prompt)
                    }
                    _ => None,
                };
                if state.editor_drag.is_some()
                    || state.slider_drag.is_some()
                    || state.text_drag.is_some()
                {
                    unsafe { SetCapture(window) };
                    if state.editor_drag.is_some() {
                        update_editor_drag(state, x, true);
                    }
                    if state.slider_drag.is_some() {
                        update_slider_drag(state, x, y, true);
                    }
                    redraw(window);
                }
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            if let Some(state) = state_mut(window) {
                let scale = state.dpi as f32 / 96.0;
                let x = signed_low_word(lparam.0) as f32 / scale;
                let y = signed_high_word(lparam.0) as f32 / scale;
                if !state.mouse_tracking {
                    let mut tracking = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: window,
                        ..Default::default()
                    };
                    if unsafe { TrackMouseEvent(&mut tracking) }.is_ok() {
                        state.mouse_tracking = true;
                    }
                }
                let menu_highlight_changed = if let Some(Action::SelectSettingsOption(index)) =
                    state.renderer.hit_test(x, y)
                    && let Some(menu) = &mut state.model.settings_menu
                    && menu.highlighted != index
                {
                    menu.highlighted = index;
                    true
                } else {
                    false
                };
                let hover_changed = state.renderer.update_hover(x, y);
                if state.editor_drag.is_some() {
                    update_editor_drag(state, x, false);
                }
                if state.slider_drag.is_some() {
                    update_slider_drag(state, x, y, false);
                }
                if state.text_drag.is_some() {
                    update_text_drag(state, x, y);
                }
                if menu_highlight_changed
                    || hover_changed
                    || state.editor_drag.is_some()
                    || state.slider_drag.is_some()
                    || state.text_drag.is_some()
                {
                    redraw(window);
                }
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE_MESSAGE => {
            if let Some(state) = state_mut(window) {
                state.mouse_tracking = false;
                if state.renderer.clear_hover() {
                    redraw(window);
                }
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            if let Some(state) = state_mut(window) {
                let scale = state.dpi as f32 / 96.0;
                let x = signed_low_word(lparam.0) as f32 / scale;
                let y = signed_high_word(lparam.0) as f32 / scale;
                if state.editor_drag.is_some() {
                    update_editor_drag(state, x, true);
                    state.editor_drag = None;
                    let _ = unsafe { ReleaseCapture() };
                    redraw(window);
                } else if state.slider_drag.is_some() {
                    update_slider_drag(state, x, y, true);
                    state.slider_drag = None;
                    let _ = unsafe { ReleaseCapture() };
                    redraw(window);
                } else if state.text_drag.is_some() {
                    update_text_drag(state, x, y);
                    state.text_drag = None;
                    let _ = unsafe { ReleaseCapture() };
                    redraw(window);
                } else if let Some(action) = state.renderer.hit_test(x, y) {
                    handle_action(window, state, action);
                }
            }
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            if let Some(state) = state_mut(window) {
                let scale = state.dpi as f32 / 96.0;
                let x = signed_low_word(lparam.0) as f32 / scale;
                let y = signed_high_word(lparam.0) as f32 / scale;
                if let Some(Action::OpenClip(index)) = state.renderer.hit_test(x, y) {
                    state.model.context_menu = Some(ClipContextMenu { clip: index, x, y });
                    redraw(window);
                }
            }
            LRESULT(0)
        }
        WM_CHAR if state_mut(window).is_some_and(|state| state.model.settings_menu.is_some()) => {
            LRESULT(0)
        }
        WM_CHAR => {
            if let Some(state) = state_mut(window) {
                if state.model.prompt.is_some() {
                    match wparam.0 as u32 {
                        13 => handle_action(window, state, Action::ConfirmPrompt),
                        27 => handle_action(window, state, Action::CancelPrompt),
                        character => {
                            if let Some(prompt) = &mut state.model.prompt {
                                match character {
                                    1 | 3 | 22 | 24 => {}
                                    8 => prompt.input.backspace(),
                                    character => {
                                        if let Some(character) = char::from_u32(character) {
                                            prompt.input.insert(character);
                                        }
                                    }
                                }
                            }
                            redraw(window);
                        }
                    }
                } else {
                    handle_character(state, wparam.0 as u32);
                    redraw(window);
                }
            }
            LRESULT(0)
        }
        WM_KEYDOWN
            if state_mut(window).is_some_and(|state| state.model.settings_menu.is_some()) =>
        {
            if let Some(state) = state_mut(window) {
                let mut select = None;
                if wparam.0 as u32 == 0x1b {
                    state.model.settings_menu = None;
                } else if let Some(menu) = &mut state.model.settings_menu {
                    match wparam.0 as u32 {
                        0x26 => menu.move_highlight(-1),
                        0x28 => menu.move_highlight(1),
                        0x24 => menu.highlighted = 0,
                        0x23 => menu.highlighted = menu.items.len().saturating_sub(1),
                        0x0d | 0x20 => select = Some(menu.highlighted),
                        _ => {}
                    }
                }
                if let Some(index) = select {
                    handle_action(window, state, Action::SelectSettingsOption(index));
                } else {
                    redraw(window);
                }
            }
            LRESULT(0)
        }
        WM_KEYDOWN if state_mut(window).is_some_and(|state| state.model.prompt.is_some()) => {
            let extend = key_pressed(0x10);
            let handled = state_mut(window)
                .and_then(|state| state.model.prompt.as_mut())
                .is_some_and(|prompt| {
                    handle_text_key(window, &mut prompt.input, wparam.0 as u32, extend)
                });
            if handled {
                redraw(window);
                LRESULT(0)
            } else {
                unsafe { DefWindowProcW(window, message, wparam, lparam) }
            }
        }
        WM_KEYDOWN if state_mut(window).is_some_and(|state| state.model.search_focused) => {
            let extend = key_pressed(0x10);
            let handled = state_mut(window).is_some_and(|state| {
                if matches!(wparam.0 as u32, 0x0d | 0x1b) {
                    state.model.search_focused = false;
                    true
                } else {
                    handle_text_key(window, &mut state.model.search, wparam.0 as u32, extend)
                }
            });
            if handled {
                redraw(window);
                LRESULT(0)
            } else {
                unsafe { DefWindowProcW(window, message, wparam, lparam) }
            }
        }
        WM_KEYDOWN | WM_SYSKEYDOWN
            if state_mut(window).is_some_and(|state| state.model.hotkey_capture) =>
        {
            if let Some(state) = state_mut(window) {
                if let Some(hotkey) = capture_hotkey(&mut state.model, wparam.0 as u32) {
                    begin_hotkey_update(state, hotkey);
                }
                redraw(window);
            }
            LRESULT(0)
        }
        WM_KEYUP | WM_SYSKEYUP
            if state_mut(window).is_some_and(|state| state.model.hotkey_capture) =>
        {
            if let Some(state) = state_mut(window) {
                state.model.hotkey_modifiers = pressed_hotkey_modifiers();
                redraw(window);
            }
            LRESULT(0)
        }
        WM_KILLFOCUS if state_mut(window).is_some_and(|state| state.model.hotkey_capture) => {
            if let Some(state) = state_mut(window) {
                cancel_hotkey_capture(&mut state.model);
                redraw(window);
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            if wparam.0 == 0x4b && key_pressed(0x11) {
                if let Some(state) = state_mut(window) {
                    handle_action(window, state, Action::Search);
                }
            } else if wparam.0 == 0x20 {
                if let Some(state) = state_mut(window)
                    && state.model.prompt.is_none()
                    && matches!(
                        state.model.page,
                        crate::model::Page::Player | crate::model::Page::Editor
                    )
                    && let Some(player) = &state.player
                    && let Err(error) = player.toggle()
                {
                    state.model.notice = Some(error);
                }
                redraw(window);
            } else if wparam.0 == 0x7a {
                if let Some(state) = state_mut(window)
                    && state.model.page == crate::model::Page::Player
                {
                    toggle_player_fullscreen(window, state);
                }
                redraw(window);
            } else if wparam.0 == 0x1b {
                if let Some(state) = state_mut(window) {
                    if state.fullscreen.is_some() {
                        exit_player_fullscreen(window, state);
                        redraw(window);
                        return LRESULT(0);
                    }
                    state.model.search_focused = false;
                    state.model.hotkey_capture = false;
                    state.model.notice = None;
                    state.model.pending_delete = None;
                    state.model.prompt = None;
                }
                redraw(window);
            }
            LRESULT(0)
        }
        PLAYER_EVENT => {
            if let Some(state) = state_mut(window) {
                if let Some(player) = &mut state.player
                    && let Err(error) = player.handle_event(wparam.0 as i32, lparam.0 as i32)
                {
                    state.model.notice = Some(format!("Cannot play this clip: {error}"));
                }
                sync_player_state(state);
                update_player_window(state);
                redraw(window);
            }
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == PLAYER_TIMER => {
            if let Some(state) = state_mut(window) {
                let trim_changed = poll_trim_updates(state);
                let hotkey_changed = poll_hotkey_updates(state);
                let notice_changed = expire_notice(state);
                let motion_changed = state.renderer.advance_motion();
                if trim_changed
                    || hotkey_changed
                    || notice_changed
                    || motion_changed
                    || matches!(
                        state.model.page,
                        crate::model::Page::Player | crate::model::Page::Editor
                    )
                {
                    sync_player_state(state);
                    keep_editor_preview_inside_selection(state);
                    redraw(window);
                }
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

fn handle_action(window: HWND, state: &mut AppState, action: Action) {
    state.model.notice = None;
    match action {
        Action::Navigate(page) => {
            if matches!(
                state.model.page,
                crate::model::Page::Player | crate::model::Page::Editor
            ) && !matches!(
                page,
                crate::model::Page::Player | crate::model::Page::Editor
            ) {
                exit_player_fullscreen(window, state);
                stop_player(state);
            }
            if page == crate::model::Page::Settings {
                state.model.autostart_enabled = crate::autostart::is_enabled();
            }
            state.model.navigate(page);
            if matches!(
                page,
                crate::model::Page::Library | crate::model::Page::Collections
            ) {
                state.renderer.retry_unavailable_thumbnails();
            }
            update_player_window(state);
        }
        Action::SettingsSection(section) => {
            state.model.settings_menu = None;
            cancel_hotkey_capture(&mut state.model);
            state.model.hotkey_error = None;
            state.model.settings_section = section;
        }
        Action::OpenClip(index) => {
            state.model.context_menu = None;
            state.model.open_clip(index);
            open_current_clip(state);
        }
        Action::EditClip(index) => {
            state.model.context_menu = None;
            state.model.open_clip(index);
            open_current_clip(state);
            begin_editor(state);
        }
        Action::RenameClip(index) => {
            state.model.context_menu = None;
            state.model.begin_rename(index);
        }
        Action::DeleteClip(index) => {
            state.model.context_menu = None;
            state.model.pending_delete = Some(DeleteTarget::Clip(index));
        }
        Action::MoveClipToCollection { clip, collection } => {
            state.model.context_menu = None;
            let result = state
                .model
                .clips
                .get(clip)
                .cloned()
                .zip(state.model.collections.get(collection).cloned())
                .ok_or_else(|| "Clip or collection is no longer available".to_owned())
                .and_then(|(clip, collection)| {
                    wreath_core::clips::move_to_collection(
                        &clip,
                        &state.model.config.storage.directory,
                        &collection.path,
                        &state.model.paths.thumbnail_dir,
                    )
                    .map_err(|error| error.to_string())
                });
            match result {
                Ok(_) => {
                    let refresh = state.model.refresh();
                    set_result(&mut state.model, refresh, "Clip moved");
                }
                Err(error) => state.model.notice = Some(error),
            }
        }
        Action::DismissContextMenu => state.model.context_menu = None,
        Action::Back => {
            exit_player_fullscreen(window, state);
            stop_player(state);
            state.model.navigate(state.model.previous_page);
            state.renderer.retry_unavailable_thumbnails();
            update_player_window(state);
        }
        Action::Refresh => {
            let result = state.model.refresh();
            if result.is_ok() {
                state.renderer.retry_unavailable_thumbnails();
            }
            set_result(&mut state.model, result, "Library refreshed");
        }
        Action::SaveReplay => match send(Request::Save) {
            Ok(Response::Saved { path: _ }) => {
                let result = state.model.refresh();
                if result.is_ok() {
                    state.renderer.retry_unavailable_thumbnails();
                } else if let Err(error) = result {
                    state.model.notice = Some(error);
                }
                wreath_windows::feedback::broadcast_clip_saved();
            }
            Ok(Response::Error { message }) | Err(message) => state.model.notice = Some(message),
            Ok(_) => {}
        },
        Action::OpenClipsFolder => open_path(&state.model.config.storage.directory),
        Action::Search => {
            if !matches!(state.model.page, crate::model::Page::Library) {
                exit_player_fullscreen(window, state);
                stop_player(state);
                state.model.navigate(crate::model::Page::Library);
                update_player_window(state);
            }
            state.model.search_focused = true;
            state.model.search.select_all();
        }
        Action::ClearSearch => {
            state.model.search.clear();
            state.model.active_collection = None;
        }
        Action::PlaceSearchCaret(_) | Action::PlacePromptCaret(_) => {}
        Action::DismissNotice => state.model.notice = None,
        Action::ToggleSidebar => {
            state.model.sidebar_expanded = !state.model.sidebar_expanded;
            update_player_window(state);
        }
        Action::ToggleAutostart => {
            let enabled = !state.model.autostart_enabled;
            match crate::autostart::set_enabled(enabled) {
                Ok(()) => state.model.autostart_enabled = enabled,
                Err(error) => state.model.notice = Some(error),
            }
        }
        Action::ToggleCursor => {
            state.model.config.capture.cursor = !state.model.config.capture.cursor
        }
        Action::ToggleDesktopAudio => {
            state.model.config.audio.desktop = !state.model.config.audio.desktop
        }
        Action::ToggleDiscordExclusion => {
            state.model.config.audio.exclude_discord = !state.model.config.audio.exclude_discord
        }
        Action::ChooseDesktopGain => choose_desktop_gain(&mut state.model),
        Action::ToggleMicrophone => {
            state.model.config.audio.microphone = !state.model.config.audio.microphone
        }
        Action::ChooseDuration => choose_duration(&mut state.model),
        Action::ChooseFrameRate => choose_frame_rate(&mut state.model),
        Action::ChooseCodec => choose_codec(&mut state.model),
        Action::ChooseQuality => choose_quality(&mut state.model),
        Action::ChooseDisplay => choose_display(&mut state.model),
        Action::ChooseMicrophone => choose_microphone(&mut state.model),
        Action::ChooseMicrophoneGain => choose_microphone_gain(&mut state.model),
        Action::ChooseStorageLimit => choose_storage_limit(&mut state.model),
        Action::DismissSettingsMenu => state.model.settings_menu = None,
        Action::SelectSettingsOption(index) => select_settings_option(&mut state.model, index),
        Action::CaptureHotkey => {
            if !state.model.hotkey_pending {
                state.model.hotkey_capture = true;
                state.model.hotkey_modifiers.clear();
                state.model.hotkey_error = None;
                state.model.notice = None;
            }
        }
        Action::ClearHotkey => {
            if !state.model.hotkey_pending {
                cancel_hotkey_capture(&mut state.model);
                begin_hotkey_update(state, wreath_core::config::HotkeyConfig::unbound());
            }
        }
        Action::ChooseStorage => choose_storage(&mut state.model),
        Action::SaveSettings => {
            save_settings(&mut state.model, "Settings saved and capture reloaded")
        }
        Action::CreateCollection => state.model.begin_new_collection(),
        Action::CancelPrompt => state.model.prompt = None,
        Action::ConfirmPrompt => confirm_prompt(state),
        Action::DeleteActiveCollection => {
            if let Some(collection) = state.model.active_collection.clone() {
                state.model.pending_delete = Some(DeleteTarget::Collection(collection));
            }
        }
        Action::CancelDelete => state.model.pending_delete = None,
        Action::ConfirmDelete => confirm_delete(&mut state.model),
        Action::SelectCollection(index) => {
            state.model.active_collection = index
                .and_then(|index| state.model.collections.get(index))
                .map(|collection| collection.path.clone());
        }
        Action::PreviousClip => switch_clip(state, -1),
        Action::NextClip => switch_clip(state, 1),
        Action::PlayPause => match state.player.as_ref().map(Player::toggle) {
            Some(Err(error)) => state.model.notice = Some(error),
            None => state.model.notice = Some("No clip is loaded".into()),
            Some(Ok(())) => {}
        },
        Action::DragPlayerSeek | Action::DragPlayerVolume | Action::DragEditorPlayhead => {}
        Action::ToggleMute => toggle_player_mute(state),
        Action::ToggleFullscreen => toggle_player_fullscreen(window, state),
        Action::EditActiveClip => begin_editor(state),
        Action::DragEditorStart | Action::DragEditorEnd => {}
        Action::SaveCut => save_cut(state, wreath_core::trim::TrimOutput::NewClip(None)),
        Action::ReplaceCut => save_cut(state, wreath_core::trim::TrimOutput::Replace),
    }
    redraw(window);
}

fn begin_editor(state: &mut AppState) {
    let Some(source) = state.model.active_clip().map(|clip| clip.path.clone()) else {
        state.model.notice = Some("Clip is no longer available".into());
        return;
    };
    if !state.model.edit_active_clip() {
        return;
    }
    update_player_window(state);
    let updates = state.trim_sender.clone();
    let spawned = std::thread::Builder::new()
        .name("wreath-editor-timing".into())
        .spawn(move || {
            use wreath_core::trim::TrimBackend;

            let result = wreath_windows::trim::MediaFoundationTrimmer::new()
                .and_then(|backend| backend.timing(&source))
                .map_err(|error| error.to_string());
            let _ = updates.send(TrimUpdate::Timing { source, result });
        });
    if spawned.is_err() {
        state.model.editor_loading = false;
        state.model.notice = Some("Cannot start the editor worker".into());
    }
}

fn save_cut(state: &mut AppState, output: wreath_core::trim::TrimOutput) {
    if state.model.editor_working || state.model.editor_timing.is_none() {
        return;
    }
    let Some(source) = state.model.active_clip().map(|clip| clip.path.clone()) else {
        state.model.notice = Some("Clip is no longer available".into());
        return;
    };
    let replacing = matches!(output, wreath_core::trim::TrimOutput::Replace);
    let request = wreath_core::trim::TrimRequest {
        source: source.clone(),
        start: state.model.editor_start,
        end: state.model.editor_end,
        mode: wreath_core::trim::TrimMode::Auto,
        output,
    };
    let thumbnails = state.model.paths.thumbnail_dir.clone();
    let updates = state.trim_sender.clone();
    if replacing {
        stop_player(state);
    }
    state.model.editor_working = true;
    state.model.notice = Some(if replacing {
        "Replacing the original clip on a background worker…".into()
    } else {
        "Cutting on a background worker…".into()
    });
    let spawned = std::thread::Builder::new()
        .name("wreath-editor-cut".into())
        .spawn(move || {
            let result = wreath_windows::trim::MediaFoundationTrimmer::new()
                .and_then(|backend| wreath_core::trim::trim(&backend, &request, &thumbnails))
                .map_err(|error| error.to_string());
            let _ = updates.send(TrimUpdate::Finished {
                source,
                replacing,
                result,
            });
        });
    if spawned.is_err() {
        state.model.editor_working = false;
        state.model.notice = Some("Cannot start the cutting worker".into());
        if replacing {
            open_current_clip(state);
        }
    }
}

fn poll_trim_updates(state: &mut AppState) -> bool {
    let mut changed = false;
    while let Ok(update) = state.trim_updates.try_recv() {
        let active = state.model.editor_source.clone();
        match update {
            TrimUpdate::Timing { source, result } if active.as_ref() == Some(&source) => {
                changed = true;
                match result {
                    Ok(timing) if !timing.duration.is_zero() => {
                        state.model.apply_editor_timing(timing);
                        state.model.notice = None;
                    }
                    Ok(_) => {
                        state.model.editor_loading = false;
                        state.model.notice = Some("This clip has no readable duration".into());
                    }
                    Err(error) => {
                        state.model.editor_loading = false;
                        state.model.notice = Some(format!("Cannot open editor: {error}"));
                    }
                }
            }
            TrimUpdate::Finished {
                source,
                replacing,
                result,
            } if active.as_ref() == Some(&source) => {
                changed = true;
                state.model.editor_working = false;
                let replaced_successfully = match result {
                    Ok(report) => {
                        let result = state.model.refresh();
                        if result.is_ok() && state.model.page == crate::model::Page::Editor {
                            state.model.active_clip = state
                                .model
                                .clips
                                .iter()
                                .position(|clip| clip.path == source);
                        }
                        if result.is_ok() {
                            state.renderer.retry_unavailable_thumbnails();
                        }
                        let name = report.path.file_name().map_or_else(
                            || report.path.display().to_string(),
                            |name| name.to_string_lossy().into_owned(),
                        );
                        let message = format!(
                            "{} · {name}",
                            if report.reencoded {
                                if replacing {
                                    "Original replaced and re-encoded for an exact start"
                                } else {
                                    "Re-encoded for an exact start"
                                }
                            } else if replacing {
                                "Original replaced with a lossless cut"
                            } else {
                                "Losslessly cut"
                            }
                        );
                        set_result(&mut state.model, result, &message);
                        replacing
                    }
                    Err(error) => {
                        state.model.notice = Some(format!("Cannot cut clip: {error}"));
                        false
                    }
                };
                if replacing {
                    open_current_clip(state);
                    if replaced_successfully && state.model.page == crate::model::Page::Editor {
                        begin_editor(state);
                    }
                }
            }
            _ => {}
        }
    }
    changed
}

fn begin_hotkey_update(state: &mut AppState, hotkey: wreath_core::config::HotkeyConfig) {
    cancel_hotkey_capture(&mut state.model);
    if hotkey == state.model.config.hotkey {
        state.model.notice = None;
        return;
    }
    state.model.hotkey_pending = true;
    state.model.hotkey_deferred = false;
    state.model.hotkey_error = None;
    state.model.notice = None;
    let paths = state.model.paths.clone();
    let updates = state.hotkey_sender.clone();
    let hotkey_for_worker = hotkey.clone();
    let spawned = std::thread::Builder::new()
        .name("wreath-hotkey-update".into())
        .spawn(move || {
            let result = activate_hotkey(&paths, &hotkey_for_worker);
            let _ = updates.send(HotkeyUpdate {
                hotkey: hotkey_for_worker,
                result,
            });
        });
    if let Err(error) = spawned {
        state.model.hotkey_pending = false;
        state.model.hotkey_error = Some(format!("Cannot start shortcut update: {error}"));
        state.model.notice = None;
    }
}

fn activate_hotkey(
    paths: &wreath_core::paths::AppPaths,
    hotkey: &wreath_core::config::HotkeyConfig,
) -> Result<HotkeyActivation, String> {
    const RETRIES: usize = 40;
    const RETRY_DELAY: Duration = Duration::from_millis(50);

    let request = Request::SetHotkey {
        hotkey: hotkey.clone(),
    };
    let mut last_connection_error = String::new();
    for attempt in 0..RETRIES {
        match send(request.clone()) {
            Ok(Response::Ok) => return Ok(HotkeyActivation::Active),
            Ok(Response::Error { message }) => return Err(message),
            Ok(_) => return Err("Background service returned an unexpected response".into()),
            Err(error) => last_connection_error = error,
        }
        if attempt + 1 < RETRIES {
            std::thread::sleep(RETRY_DELAY);
        }
    }

    wreath_windows::hotkey::validate_hotkey_choice(hotkey).map_err(|error| error.to_string())?;
    let availability = hotkey
        .is_bound()
        .then(|| wreath_windows::hotkey::HotkeyRegistration::register(2, hotkey))
        .transpose()
        .map_err(|error| format!("That shortcut is unavailable: {error}"))?;
    drop(availability);
    let mut config = wreath_core::config::Config::load(paths).map_err(|error| error.to_string())?;
    config.hotkey = hotkey.clone();
    config.save(paths).map_err(|error| {
        format!(
            "Background service was unavailable ({last_connection_error}); cannot save shortcut: {error}"
        )
    })?;
    Ok(HotkeyActivation::SavedForNextStart)
}

fn poll_hotkey_updates(state: &mut AppState) -> bool {
    let mut changed = false;
    while let Ok(update) = state.hotkey_updates.try_recv() {
        changed = true;
        state.model.hotkey_pending = false;
        match update.result {
            Ok(HotkeyActivation::Active) => {
                state.model.config.hotkey = update.hotkey;
                state.model.hotkey_deferred = false;
                state.model.hotkey_error = None;
                state.model.notice = None;
            }
            Ok(HotkeyActivation::SavedForNextStart) => {
                state.model.config.hotkey = update.hotkey;
                state.model.hotkey_deferred = true;
                state.model.hotkey_error = None;
                state.model.notice = None;
            }
            Err(error) => {
                state.model.hotkey_deferred = false;
                state.model.hotkey_error = Some(format!("Cannot change shortcut: {error}"));
                state.model.notice = None;
            }
        }
    }
    changed
}

fn capture_hotkey(
    model: &mut UiModel,
    virtual_key: u32,
) -> Option<wreath_core::config::HotkeyConfig> {
    match virtual_key {
        0x1b => {
            cancel_hotkey_capture(model);
            model.hotkey_error = None;
            model.notice = None;
            None
        }
        0x10 | 0x11 | 0x12 | 0x5b | 0x5c => {
            let mut modifiers = pressed_hotkey_modifiers()
                .into_iter()
                .chain(model.hotkey_modifiers.iter().cloned())
                .collect::<Vec<_>>();
            let modifier = match virtual_key {
                0x10 => "SHIFT",
                0x11 => "CTRL",
                0x12 => "ALT",
                0x5b | 0x5c => "SUPER",
                _ => unreachable!(),
            };
            if !modifiers.iter().any(|value| value == modifier) {
                modifiers.push(modifier.into());
            }
            model.hotkey_modifiers = canonical_hotkey_modifiers(modifiers);
            None
        }
        key => {
            let Some(key_name) = wreath_windows::hotkey::key_name_from_virtual_key(key) else {
                model.hotkey_error = Some("That key cannot be used as a Windows shortcut.".into());
                model.notice = None;
                return None;
            };
            let modifiers = canonical_hotkey_modifiers(
                pressed_hotkey_modifiers()
                    .into_iter()
                    .chain(model.hotkey_modifiers.iter().cloned())
                    .collect(),
            );
            model.hotkey_modifiers = modifiers.clone();
            let hotkey = wreath_core::config::HotkeyConfig {
                modifiers,
                key: key_name,
            };
            if let Err(error) = wreath_windows::hotkey::validate_hotkey_choice(&hotkey) {
                model.hotkey_error = Some(format!("Choose a safer shortcut: {error}."));
                model.notice = None;
                return None;
            }
            Some(hotkey)
        }
    }
}

fn pressed_hotkey_modifiers() -> Vec<String> {
    let mut modifiers = Vec::new();
    if key_pressed(0x5b) || key_pressed(0x5c) {
        modifiers.push("SUPER".into());
    }
    if key_pressed(0x11) {
        modifiers.push("CTRL".into());
    }
    if key_pressed(0x12) {
        modifiers.push("ALT".into());
    }
    if key_pressed(0x10) {
        modifiers.push("SHIFT".into());
    }
    modifiers
}

fn canonical_hotkey_modifiers(modifiers: Vec<String>) -> Vec<String> {
    ["SUPER", "CTRL", "ALT", "SHIFT"]
        .into_iter()
        .filter(|candidate| modifiers.iter().any(|value| value == candidate))
        .map(str::to_owned)
        .collect()
}

fn cancel_hotkey_capture(model: &mut UiModel) {
    model.hotkey_capture = false;
    model.hotkey_modifiers.clear();
}

fn key_pressed(virtual_key: i32) -> bool {
    (unsafe { GetKeyState(virtual_key) }) < 0
}

fn confirm_prompt(state: &mut AppState) {
    let Some(prompt) = state.model.prompt.take() else {
        return;
    };
    let name = prompt.input.value.trim().to_owned();
    let outcome = match prompt.kind {
        PromptKind::NewCollection => {
            let directory = state.model.config.storage.directory.clone();
            match wreath_core::clips::create_collection(&directory, &name) {
                Ok(path) => {
                    state.model.active_collection = Some(path);
                    Ok("Collection created")
                }
                Err(error) => Err(format!("Cannot create collection: {error}")),
            }
        }
        PromptKind::RenameClip(index) => {
            let Some(clip) = state.model.clips.get(index).cloned() else {
                state.model.notice = Some("Clip is no longer available".into());
                return;
            };
            let thumbnails = state.model.paths.thumbnail_dir.clone();
            match wreath_core::clips::rename(&clip, &name, &thumbnails) {
                Ok(_) => Ok("Clip renamed"),
                Err(error) => Err(format!("Cannot rename clip: {error}")),
            }
        }
    };
    match outcome {
        Ok(message) => {
            let refreshed = state.model.refresh();
            if refreshed.is_ok() {
                state.renderer.retry_unavailable_thumbnails();
            }
            set_result(&mut state.model, refreshed, message);
        }
        Err(error) => state.model.notice = Some(error),
    }
}

fn confirm_delete(model: &mut UiModel) {
    let Some(target) = model.pending_delete.take() else {
        return;
    };
    match target {
        DeleteTarget::Clip(index) => {
            let Some(clip) = model.clips.get(index).cloned() else {
                model.notice = Some("Clip is no longer available".into());
                return;
            };
            match wreath_core::clips::delete(&clip, &model.paths.thumbnail_dir) {
                Ok(()) => {
                    let result = model.refresh();
                    set_result(model, result, "Clip deleted");
                }
                Err(error) => model.notice = Some(format!("Cannot delete clip: {error}")),
            }
        }
        DeleteTarget::Collection(collection) => match wreath_core::clips::delete_collection(
            &model.config.storage.directory,
            &collection,
            &model.paths.thumbnail_dir,
        ) {
            Ok(()) => {
                model.active_collection = None;
                let result = model.refresh();
                set_result(model, result, "Collection deleted; clips moved to Library");
            }
            Err(error) => model.notice = Some(format!("Cannot delete collection: {error}")),
        },
    }
}

fn open_current_clip(state: &mut AppState) {
    // Never display a position or duration that belongs to the media item we
    // just left. The new values arrive from Media Foundation after it has
    // resolved the current (possibly replaced) file.
    state.model.reset_player_state();
    update_player_window(state);
    let Some(path) = state.model.active_clip().map(|clip| clip.path.clone()) else {
        return;
    };
    if let Some(player) = &mut state.player {
        if let Err(error) = player.open(&path) {
            state.model.notice = Some(error);
        }
    }
}

fn switch_clip(state: &mut AppState, offset: isize) {
    if state.model.page != crate::model::Page::Player || !state.model.select_adjacent_clip(offset) {
        return;
    }
    open_current_clip(state);
}

fn set_player_volume(state: &mut AppState, percent: u8) {
    if state.model.page != crate::model::Page::Player {
        return;
    }
    state.model.set_player_volume(percent);
    if let Some(player) = &state.player
        && let Err(error) = player.set_volume(state.model.player_volume_percent)
    {
        state.model.notice = Some(format!("Cannot set playback volume: {error}"));
    }
}

fn toggle_player_mute(state: &mut AppState) {
    if state.model.page != crate::model::Page::Player {
        return;
    }
    state.model.toggle_player_mute();
    if let Some(player) = &state.player
        && let Err(error) = player.set_volume(state.model.player_volume_percent)
    {
        state.model.notice = Some(format!("Cannot change playback mute: {error}"));
    }
}

fn stop_player(state: &mut AppState) {
    if let Some(player) = &mut state.player {
        player.close();
    }
    state.player_seek = None;
    state.preview_seek = None;
    sync_player_state(state);
}

fn update_editor_drag(state: &mut AppState, x: f32, settle: bool) {
    let Some(handle) = state.editor_drag else {
        return;
    };
    let scale = state.dpi as f32 / 96.0;
    let width = ((state.width as f32 / scale).round() as u32).max(1);
    let height = ((state.height as f32 / scale).round() as u32).max(1);
    let rail = editor_timeline_rail(
        width,
        height,
        state.model.player_aspect_ratio,
        state.model.sidebar_expanded,
    );
    let thousandths = editor_timeline_fraction(rail, x);
    match handle {
        EditorDrag::Start => state.model.set_editor_start(thousandths),
        EditorDrag::End => state.model.set_editor_end(thousandths),
    }
    let position = match handle {
        EditorDrag::Start => state.model.editor_start,
        EditorDrag::End => state.model.editor_end,
    };
    seek_editor_preview(state, position, settle);
}

fn update_slider_drag(state: &mut AppState, x: f32, y: f32, settle: bool) {
    let Some(drag) = state.slider_drag else {
        return;
    };
    let scale = state.dpi as f32 / 96.0;
    let width = ((state.width as f32 / scale).round() as u32).max(1);
    let height = ((state.height as f32 / scale).round() as u32).max(1);
    match drag {
        SliderDrag::PlayerSeek => {
            let rail = player_timeline_rail(width, height, state.model.sidebar_expanded);
            let fraction = ((x - rail.left) / (rail.right - rail.left).max(1.0)).clamp(0.0, 1.0);
            let fraction = f64::from(fraction);
            state.model.player_position_seconds = state.model.player_duration_seconds * fraction;
            let should_seek = settle
                || state
                    .player_seek
                    .is_none_or(|issued| issued.elapsed() >= PLAYER_SEEK_INTERVAL);
            if should_seek {
                if let Some(player) = &state.player {
                    let _ = player.seek_fraction(fraction);
                }
                state.player_seek = Some(Instant::now());
            }
        }
        SliderDrag::PlayerVolume => {
            let rail = player_volume_rail(
                width,
                height,
                state.model.player_aspect_ratio,
                state.model.sidebar_expanded,
            );
            let fraction =
                (1.0 - (y - rail.top) / (rail.bottom - rail.top).max(1.0)).clamp(0.0, 1.0);
            set_player_volume(state, (fraction * 100.0).round() as u8);
        }
        SliderDrag::EditorPlayhead => {
            let Some(timing) = &state.model.editor_timing else {
                return;
            };
            let rail = editor_timeline_rail(
                width,
                height,
                state.model.player_aspect_ratio,
                state.model.sidebar_expanded,
            );
            let thousandths = editor_timeline_fraction(rail, x);
            let requested = timing.duration.mul_f64(f64::from(thousandths) / 1_000.0);
            let position = requested
                .max(state.model.editor_start)
                .min(state.model.editor_end);
            seek_editor_preview(state, position, settle);
        }
    }
}

fn update_text_drag(state: &mut AppState, x: f32, y: f32) {
    let Some(drag) = state.text_drag else {
        return;
    };
    match (drag, state.renderer.hit_test(x, y)) {
        (TextDrag::Search, Some(Action::PlaceSearchCaret(position))) => {
            state.model.search.move_caret(position, true);
        }
        (TextDrag::Prompt, Some(Action::PlacePromptCaret(position))) => {
            if let Some(prompt) = &mut state.model.prompt {
                prompt.input.move_caret(position, true);
            }
        }
        _ => {}
    }
}

fn seek_editor_preview(state: &mut AppState, position: Duration, settle: bool) {
    if !state.model.player_ready {
        return;
    }
    state.model.player_position_seconds = position.as_secs_f64();
    if !settle
        && state
            .preview_seek
            .is_some_and(|issued| issued.elapsed() < PREVIEW_SEEK_INTERVAL)
    {
        return;
    }
    let duration = state.model.player_duration_seconds;
    if duration <= f64::EPSILON {
        return;
    }
    let Some(player) = &state.player else {
        return;
    };
    let _ = player.seek_fraction(position.as_secs_f64() / duration);
    state.preview_seek = Some(Instant::now());
}

fn keep_editor_preview_inside_selection(state: &mut AppState) {
    if state.model.page != crate::model::Page::Editor
        || state.model.editor_timing.is_none()
        || state.editor_drag.is_some()
        || !state.model.player_ready
    {
        return;
    }
    let position = state.model.player_position_seconds;
    let start = state.model.editor_start.as_secs_f64();
    let end = state.model.editor_end.as_secs_f64();
    if position + PREVIEW_SLACK_SECONDS >= start && position < end {
        state.preview_seek = None;
        return;
    }
    if state
        .preview_seek
        .is_some_and(|issued| issued.elapsed() < PREVIEW_SEEK_SETTLE)
    {
        return;
    }
    let resume = state.model.player_playing;
    seek_editor_preview(state, state.model.editor_start, true);
    if resume && let Some(player) = &state.player {
        let _ = player.play();
    }
}

fn update_player_window(state: &mut AppState) {
    let Some(window) = state.video_window else {
        return;
    };
    if !matches!(
        state.model.page,
        crate::model::Page::Player | crate::model::Page::Editor
    ) {
        if let Some(player) = &mut state.player {
            player.close();
        }
        unsafe {
            let _ = ShowWindow(window, SW_HIDE);
        }
        return;
    }
    let scale = state.dpi as f32 / 96.0;
    let logical_width = (state.width as f32 / scale).round() as u32;
    let logical_height = (state.height as f32 / scale).round() as u32;
    let bounds = if state.fullscreen.is_some() && state.model.page == crate::model::Page::Player {
        crate::renderer::LogicalRect {
            left: 0.0,
            top: 0.0,
            right: logical_width as f32,
            bottom: logical_height as f32,
        }
    } else if state.model.page == crate::model::Page::Editor {
        editor_player_bounds(
            logical_width,
            logical_height,
            state.model.player_aspect_ratio,
            state.model.sidebar_expanded,
        )
    } else {
        player_bounds(
            logical_width,
            logical_height,
            state.model.player_aspect_ratio,
            state.model.sidebar_expanded,
        )
    };
    let _ = unsafe {
        SetWindowPos(
            window,
            None,
            (bounds.left * scale).round() as i32,
            (bounds.top * scale).round() as i32,
            ((bounds.right - bounds.left) * scale).round().max(1.0) as i32,
            ((bounds.bottom - bounds.top) * scale).round().max(1.0) as i32,
            SWP_NOZORDER,
        )
    };
    unsafe {
        let _ = ShowWindow(window, SW_SHOW);
    }
    if let Some(player) = &state.player {
        player.update_video();
    }
}

fn toggle_player_fullscreen(window: HWND, state: &mut AppState) {
    if state.fullscreen.is_some() {
        exit_player_fullscreen(window, state);
        return;
    }
    if state.model.page != crate::model::Page::Player {
        return;
    }
    let mut placement = WINDOWPLACEMENT {
        length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
        ..Default::default()
    };
    if let Err(error) = unsafe { GetWindowPlacement(window, &mut placement) } {
        state.model.notice = Some(format!("Cannot enter fullscreen: {error}"));
        return;
    }
    let monitor = unsafe { MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST) };
    let mut monitor_info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(monitor, &mut monitor_info) }.as_bool() {
        state.model.notice = Some("Cannot find the display for fullscreen".into());
        return;
    }
    let style = unsafe { GetWindowLongPtrW(window, GWL_STYLE) };
    state.fullscreen = Some(FullscreenState { style, placement });
    let fullscreen_style = (style as u32 & !WS_OVERLAPPEDWINDOW.0) | WS_POPUP.0 | WS_VISIBLE.0;
    unsafe {
        SetWindowLongPtrW(window, GWL_STYLE, fullscreen_style as isize);
        let monitor = monitor_info.rcMonitor;
        let _ = SetWindowPos(
            window,
            None,
            monitor.left,
            monitor.top,
            monitor.right - monitor.left,
            monitor.bottom - monitor.top,
            SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
    update_player_window(state);
}

fn exit_player_fullscreen(window: HWND, state: &mut AppState) {
    let Some(fullscreen) = state.fullscreen.take() else {
        return;
    };
    unsafe {
        SetWindowLongPtrW(window, GWL_STYLE, fullscreen.style);
        let _ = SetWindowPlacement(window, &fullscreen.placement);
        let _ = SetWindowPos(
            window,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
    update_player_window(state);
}

fn expire_notice(state: &mut AppState) -> bool {
    state
        .notice_expiry
        .tick(&mut state.model.notice, Instant::now(), NOTICE_LIFETIME)
}

fn sync_player_state(state: &mut AppState) {
    let Some(player) = &state.player else {
        return;
    };
    let snapshot = player.snapshot();
    state.model.player_ready = snapshot.ready;
    state.model.player_playing = snapshot.playing;
    let dragging_playhead = state.editor_drag.is_some()
        || matches!(
            state.slider_drag,
            Some(SliderDrag::PlayerSeek | SliderDrag::EditorPlayhead)
        );
    if !dragging_playhead {
        state.model.player_position_seconds = snapshot.position_seconds;
    }
    state.model.player_duration_seconds = snapshot.duration_seconds;
    state.model.player_aspect_ratio = snapshot.aspect_ratio;
}

fn handle_text_key(window: HWND, input: &mut TextInput, key: u32, extend: bool) -> bool {
    if key_pressed(0x11) {
        return match key {
            0x41 => {
                input.select_all();
                true
            }
            0x43 => {
                let selected = input.selected_text();
                if !selected.is_empty() {
                    let _ = write_clipboard(window, &selected);
                }
                true
            }
            0x58 => {
                let selected = input.selected_text();
                if !selected.is_empty() && write_clipboard(window, &selected).is_ok() {
                    input.delete();
                }
                true
            }
            0x56 => {
                if let Ok(value) = read_clipboard(window) {
                    input.insert_text(&value);
                }
                true
            }
            _ => false,
        };
    }
    match key {
        0x25 => input.caret_left(extend),
        0x27 => input.caret_right(extend),
        0x24 => input.caret_home(extend),
        0x23 => input.caret_end(extend),
        0x2e => input.delete(),
        _ => return false,
    }
    true
}

fn read_clipboard(window: HWND) -> Result<String, String> {
    unsafe { OpenClipboard(Some(window)) }.map_err(|error| error.to_string())?;
    let result = (|| {
        let handle = unsafe { GetClipboardData(CF_UNICODETEXT_FORMAT) }
            .map_err(|error| error.to_string())?;
        let memory = HGLOBAL(handle.0);
        let size = unsafe { GlobalSize(memory) } / std::mem::size_of::<u16>();
        let pointer = unsafe { GlobalLock(memory) }.cast::<u16>();
        if pointer.is_null() {
            return Err("Clipboard text is unavailable".into());
        }
        let units = unsafe { std::slice::from_raw_parts(pointer, size) };
        let length = units.iter().position(|unit| *unit == 0).unwrap_or(size);
        let value = String::from_utf16_lossy(&units[..length]);
        let _ = unsafe { GlobalUnlock(memory) };
        Ok(value)
    })();
    let _ = unsafe { CloseClipboard() };
    result
}

fn write_clipboard(window: HWND, value: &str) -> Result<(), String> {
    let units = value.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let memory = unsafe { GlobalAlloc(GMEM_MOVEABLE, units.len() * std::mem::size_of::<u16>()) }
        .map_err(|error| error.to_string())?;
    let pointer = unsafe { GlobalLock(HGLOBAL(memory.0)) }.cast::<u16>();
    if pointer.is_null() {
        let _ = unsafe { GlobalFree(Some(memory)) };
        return Err("Cannot allocate clipboard text".into());
    }
    unsafe { std::ptr::copy_nonoverlapping(units.as_ptr(), pointer, units.len()) };
    let _ = unsafe { GlobalUnlock(HGLOBAL(memory.0)) };

    if let Err(error) = unsafe { OpenClipboard(Some(window)) } {
        let _ = unsafe { GlobalFree(Some(memory)) };
        return Err(error.to_string());
    }
    let result = (|| {
        unsafe { EmptyClipboard() }.map_err(|error| error.to_string())?;
        unsafe { SetClipboardData(CF_UNICODETEXT_FORMAT, Some(HANDLE(memory.0))) }
            .map_err(|error| error.to_string())?;
        Ok(())
    })();
    let _ = unsafe { CloseClipboard() };
    if result.is_err() {
        let _ = unsafe { GlobalFree(Some(memory)) };
    }
    result
}

fn handle_character(state: &mut AppState, character: u32) {
    if !state.model.search_focused {
        return;
    }
    match character {
        1 | 3 | 22 | 24 => {}
        8 => {
            state.model.search.backspace();
        }
        13 => state.model.search_focused = false,
        32..=0x10ffff => {
            if let Some(character) = char::from_u32(character)
                && !character.is_control()
            {
                state.model.search.insert(character);
            }
        }
        _ => {}
    }
}

fn save_settings(model: &mut UiModel, success: &str) {
    match model.config.save(&model.paths) {
        Ok(()) => match reload_capture() {
            Ok(Response::Ok) => model.notice = Some(success.into()),
            Ok(Response::Error { message }) | Err(message) => {
                model.notice = Some(format!("Saved, but reload failed: {message}"))
            }
            Ok(_) => model.notice = Some("Settings saved".into()),
        },
        Err(error) => model.notice = Some(format!("Cannot save settings: {error}")),
    }
}

fn reload_capture() -> Result<Response, String> {
    let mut last_error = match send(Request::Reload) {
        Ok(response) => return Ok(response),
        Err(error) => error,
    };
    start_daemon()
        .map_err(|start_error| format!("background service could not be started: {start_error}"))?;
    for _ in 0..crate::recovery::daemon_startup_attempts() {
        std::thread::sleep(crate::recovery::DAEMON_RETRY_INTERVAL);
        match send(Request::Reload) {
            Ok(response) => return Ok(response),
            Err(error) => last_error = error,
        }
    }
    Err(format!(
        "background service did not become ready within {} seconds (last error: {last_error})",
        crate::recovery::DAEMON_STARTUP_TIMEOUT.as_secs()
    ))
}

fn choose_duration(model: &mut UiModel) {
    let values = [15, 30, 45, 60, 90, 120];
    let labels = values
        .iter()
        .map(|seconds| format!("{seconds} seconds"))
        .collect::<Vec<_>>();
    let current = values
        .iter()
        .position(|value| *value == model.config.capture.duration_seconds);
    open_choice_menu(model, SettingsMenuKind::Duration, labels, current);
}

fn choose_frame_rate(model: &mut UiModel) {
    let values = model.frame_rate_options();
    let labels = values
        .iter()
        .map(|rate| format!("{rate} fps"))
        .collect::<Vec<_>>();
    let current = values
        .iter()
        .position(|value| *value == model.config.capture.frames_per_second);
    open_choice_menu(model, SettingsMenuKind::FrameRate, labels, current);
}

fn choose_codec(model: &mut UiModel) {
    let values = [Codec::Auto, Codec::H264, Codec::Hevc, Codec::Av1];
    let labels = ["Auto (recommended)", "H.264", "HEVC", "AV1"]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let current = values
        .iter()
        .position(|value| *value == model.config.capture.codec);
    open_choice_menu(model, SettingsMenuKind::Codec, labels, current);
}

fn choose_quality(model: &mut UiModel) {
    let options = model.quality_options();
    let items = options
        .iter()
        .map(|option| SettingsMenuItem {
            label: option.label.clone(),
            detail: Some(format!(
                "≈ {} MB total · {} s",
                option.megabytes, option.seconds
            )),
        })
        .collect::<Vec<_>>();
    let current = options
        .iter()
        .position(|option| option.value == model.config.capture.quality);
    model.settings_menu = Some(SettingsMenu::new(SettingsMenuKind::Quality, items, current));
}

fn choose_display(model: &mut UiModel) {
    if let Err(error) = load_displays(model) {
        model.notice = Some(error);
        return;
    }
    let labels = model
        .displays
        .iter()
        .map(|display| display.label.clone())
        .collect::<Vec<_>>();
    let current = model.config.capture.monitor.as_deref().and_then(|name| {
        model
            .displays
            .iter()
            .position(|display| display.name.eq_ignore_ascii_case(name))
    });
    open_choice_menu(model, SettingsMenuKind::Display, labels, current);
}

fn choose_microphone(model: &mut UiModel) {
    refresh_microphones(model);
    let mut labels = vec!["Windows default".to_string()];
    labels.extend(model.microphone_names.iter().map(|(_, name)| name.clone()));
    let current = model
        .config
        .audio
        .microphone_device
        .as_deref()
        .and_then(|id| {
            model
                .microphone_names
                .iter()
                .position(|(device_id, _)| device_id == id)
        })
        .map_or(Some(0), |index| Some(index + 1));
    open_choice_menu(model, SettingsMenuKind::Microphone, labels, current);
}

fn choose_microphone_gain(model: &mut UiModel) {
    let values = [25, 50, 75, 100];
    let labels = values
        .iter()
        .map(|gain| format!("{gain}%"))
        .collect::<Vec<_>>();
    let current = values
        .iter()
        .position(|value| *value == model.config.audio.microphone_gain_percent);
    open_choice_menu(model, SettingsMenuKind::MicrophoneGain, labels, current);
}

fn choose_desktop_gain(model: &mut UiModel) {
    let values = [0, 25, 50, 75, 100, 125, 150, 175, 200];
    let labels = values
        .iter()
        .map(|gain| format!("{gain}%"))
        .collect::<Vec<_>>();
    let current = values
        .iter()
        .position(|value| *value == model.config.audio.desktop_gain_percent);
    open_choice_menu(model, SettingsMenuKind::DesktopGain, labels, current);
}

fn choose_storage_limit(model: &mut UiModel) {
    let values = [1_024, 5_120, 10_240, 25_600, 51_200, 102_400];
    let labels = ["1 GB", "5 GB", "10 GB", "25 GB", "50 GB", "100 GB"]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let current = values
        .iter()
        .position(|value| *value == model.config.storage.max_megabytes);
    open_choice_menu(model, SettingsMenuKind::StorageLimit, labels, current);
}

fn open_choice_menu(
    model: &mut UiModel,
    kind: SettingsMenuKind,
    labels: Vec<String>,
    current: Option<usize>,
) {
    let items = labels
        .into_iter()
        .map(|label| SettingsMenuItem {
            label,
            detail: None,
        })
        .collect();
    model.settings_menu = Some(SettingsMenu::new(kind, items, current));
}

fn select_settings_option(model: &mut UiModel, index: usize) {
    let Some(menu) = model.settings_menu.as_ref() else {
        return;
    };
    if index >= menu.items.len() {
        return;
    }
    let kind = menu.kind;
    match kind {
        SettingsMenuKind::Duration => {
            if let Some(value) = [15, 30, 45, 60, 90, 120].get(index) {
                model.config.capture.duration_seconds = *value;
            }
        }
        SettingsMenuKind::FrameRate => {
            if let Some(value) = model.frame_rate_options().get(index) {
                model.config.capture.frames_per_second = *value;
            }
        }
        SettingsMenuKind::Codec => {
            if let Some(value) = [Codec::Auto, Codec::H264, Codec::Hevc, Codec::Av1].get(index) {
                model.config.capture.codec = *value;
            }
        }
        SettingsMenuKind::Quality => {
            if let Some(option) = model.quality_options().get(index) {
                model.config.capture.quality = option.value;
            }
        }
        SettingsMenuKind::Display => {
            if let Some(display) = model.displays.get(index) {
                model.config.capture.monitor = Some(display.name.clone());
                let native_rate = (display.refresh_rate.round() as u16)
                    .clamp(15, wreath_core::config::MAX_FRAMES_PER_SECOND);
                model.config.capture.frames_per_second =
                    model.config.capture.frames_per_second.min(native_rate);
            }
        }
        SettingsMenuKind::Microphone => {
            model.config.audio.microphone_device = index
                .checked_sub(1)
                .and_then(|index| model.microphone_names.get(index))
                .map(|(id, _)| id.clone());
        }
        SettingsMenuKind::MicrophoneGain => {
            if let Some(value) = [25, 50, 75, 100].get(index) {
                model.config.audio.microphone_gain_percent = *value;
            }
        }
        SettingsMenuKind::DesktopGain => {
            if let Some(value) = [0, 25, 50, 75, 100, 125, 150, 175, 200].get(index) {
                model.config.audio.desktop_gain_percent = *value;
            }
        }
        SettingsMenuKind::StorageLimit => {
            if let Some(value) = [1_024, 5_120, 10_240, 25_600, 51_200, 102_400].get(index) {
                model.config.storage.max_megabytes = *value;
            }
        }
    }
    model.settings_menu = None;
}

fn load_displays(model: &mut UiModel) -> Result<(), String> {
    let displays = wreath_windows::display::displays().map_err(|error| error.to_string())?;
    if displays.is_empty() {
        return Err("Windows reported no displays".into());
    }
    model.displays = displays
        .into_iter()
        .map(|display| {
            let friendly_name = display.name.trim_start_matches(r#"\\.\"#);
            let primary = if display.primary { " · Primary" } else { "" };
            DisplayOption {
                label: format!(
                    "{} · {}×{} · {:.0} Hz{}",
                    friendly_name, display.width, display.height, display.refresh_rate, primary
                ),
                name: display.name,
                refresh_rate: display.refresh_rate,
                width: display.width,
                height: display.height,
            }
        })
        .collect();
    Ok(())
}

fn refresh_displays(model: &mut UiModel) {
    let _ = load_displays(model);
}

fn refresh_microphones(model: &mut UiModel) {
    if let Ok(devices) = wreath_windows::audio::microphones() {
        model.microphone_names = devices
            .into_iter()
            .map(|device| (device.id, device.name))
            .collect();
    }
}

fn choose_storage(model: &mut UiModel) {
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
    use windows::Win32::UI::Shell::{
        FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH,
    };
    let result = (|| -> Result<std::path::PathBuf, String> {
        let dialog: IFileOpenDialog =
            unsafe { CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER) }
                .map_err(|error| error.to_string())?;
        let options = unsafe { dialog.GetOptions() }.map_err(|error| error.to_string())?;
        unsafe { dialog.SetOptions(options | FOS_PICKFOLDERS) }
            .map_err(|error| error.to_string())?;
        unsafe { dialog.Show(None) }.map_err(|error| error.to_string())?;
        let item = unsafe { dialog.GetResult() }.map_err(|error| error.to_string())?;
        let path =
            unsafe { item.GetDisplayName(SIGDN_FILESYSPATH) }.map_err(|error| error.to_string())?;
        let value = unsafe { path.to_string() }.map_err(|error| error.to_string())?;
        unsafe { windows::Win32::System::Com::CoTaskMemFree(Some(path.0.cast())) };
        Ok(value.into())
    })();
    match result {
        Ok(path) => model.config.storage.directory = path,
        Err(error) if error.contains("0x800704C7") => {}
        Err(error) => model.notice = Some(format!("Folder picker failed: {error}")),
    }
}

fn set_result(model: &mut UiModel, result: Result<(), String>, success: &str) {
    model.notice = Some(result.map_or_else(|error| error, |_| success.into()));
}

fn send(request: Request) -> Result<Response, String> {
    let paths = wreath_core::paths::AppPaths::discover();
    wreath_windows::control::send_request(paths.pipe_name(), &request)
        .map_err(|error| error.to_string())
}

fn ensure_tray() -> Result<(), String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let tray = executable
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("wreath-tray.exe");
    std::process::Command::new(&tray)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("cannot start {}: {error}", tray.display()))
}

fn start_daemon() -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    use windows::Win32::System::Threading::CREATE_NO_WINDOW;

    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let daemon = executable
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("wreathd.exe");
    std::process::Command::new(&daemon)
        .creation_flags(CREATE_NO_WINDOW.0)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("cannot start {}: {error}", daemon.display()))
}

fn open_path(path: &Path) {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let path = wide(&path.display().to_string());
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(path.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        );
    }
}

fn state_mut(window: HWND) -> Option<&'static mut AppState> {
    let pointer = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) } as *mut AppState;
    unsafe { pointer.as_mut() }
}

fn redraw(window: HWND) {
    unsafe {
        let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(window), None, false);
    }
}

fn low_word(value: isize) -> u16 {
    value as u16
}
fn high_word(value: isize) -> u16 {
    (value >> 16) as u16
}
fn signed_low_word(value: isize) -> i16 {
    low_word(value) as i16
}
fn signed_high_word(value: isize) -> i16 {
    high_word(value) as i16
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
