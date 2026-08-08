use std::path::Path;
use std::path::PathBuf;
use std::sync::mpsc;

use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT, UpdateWindow};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreatePopupMenu,
    CreateWindowExW, DefWindowProcW, DestroyMenu, DispatchMessageW, FindWindowW, GWLP_USERDATA,
    GetClientRect, GetCursorPos, GetMessageW, GetWindowLongPtrW, HMENU, IDC_ARROW, LoadCursorW,
    MF_CHECKED, MF_SEPARATOR, MF_STRING, MINMAXINFO, MSG, PostQuitMessage, RegisterClassW,
    SIZE_MINIMIZED, SW_HIDE, SW_RESTORE, SW_SHOW, SWP_NOACTIVATE, SWP_NOZORDER,
    SetForegroundWindow, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage, WINDOW_EX_STYLE, WM_CHAR, WM_COMMAND,
    WM_DESTROY, WM_DPICHANGED, WM_GETMINMAXINFO, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEMOVE, WM_NCCREATE, WM_PAINT, WM_RBUTTONUP, WM_SIZE, WM_SYSKEYDOWN, WM_TIMER, WNDCLASSW,
    WS_CHILD, WS_DISABLED, WS_OVERLAPPEDWINDOW,
};
use windows::core::{PCWSTR, w};
use wreath_core::config::Codec;
use wreath_core::ipc::{Request, Response};

use crate::model::{Action, DeleteTarget, DisplayOption, UiModel};
use crate::player::{PLAYER_EVENT, Player};
use crate::renderer::{
    Renderer, editor_player_bounds, editor_timeline_fraction, editor_timeline_rail, player_bounds,
};

const WINDOW_CLASS: windows::core::PCWSTR = w!("WreathApplicationWindow");
const COMMAND_CLIP_RENAME: usize = 500;
const COMMAND_CLIP_DELETE: usize = 501;
const COMMAND_CLIP_MOVE_LIBRARY: usize = 502;
const COMMAND_CLIP_EDIT: usize = 503;
const COMMAND_CLIP_MOVE_COLLECTION_BASE: usize = 600;
const PLAYER_TIMER: usize = 2;

struct AppState {
    model: UiModel,
    renderer: Renderer,
    width: u32,
    height: u32,
    dpi: u32,
    player: Option<Player>,
    video_window: Option<HWND>,
    context_clip: Option<usize>,
    trim_updates: mpsc::Receiver<TrimUpdate>,
    trim_sender: mpsc::Sender<TrimUpdate>,
    editor_drag: Option<EditorDrag>,
}

#[derive(Clone, Copy)]
enum EditorDrag {
    Start,
    End,
}

enum TrimUpdate {
    Timing {
        source: PathBuf,
        result: Result<wreath_core::trim::ClipTiming, String>,
    },
    Finished {
        source: PathBuf,
        result: Result<wreath_core::trim::TrimReport, String>,
    },
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
    refresh_displays(&mut model);
    refresh_microphones(&mut model);
    let (trim_sender, trim_updates) = mpsc::channel();
    let state = Box::new(AppState {
        model,
        renderer: Renderer::new()?,
        width: 1280,
        height: 760,
        dpi: 96,
        player: None,
        video_window: None,
        context_clip: None,
        trim_updates,
        trim_sender,
        editor_drag: None,
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
            1280,
            760,
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
        let _ = SetTimer(Some(window), PLAYER_TIMER, 250, None);
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
                }
                set_result(&mut state.model, result, "Clip added to Library");
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
                    (*info).ptMinTrackSize.x = (900.0 * scale).round() as i32;
                    (*info).ptMinTrackSize.y = (640.0 * scale).round() as i32;
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
                if let Err(error) = state.renderer.paint(
                    window,
                    &state.model,
                    ((state.width as f32 / scale).round() as u32).max(1),
                    ((state.height as f32 / scale).round() as u32).max(1),
                ) {
                    state.model.notice = Some(format!("Rendering failed: {error}"));
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
                state.editor_drag = match state.renderer.hit_test(x, y) {
                    Some(Action::DragEditorStart) => Some(EditorDrag::Start),
                    Some(Action::DragEditorEnd) => Some(EditorDrag::End),
                    _ => None,
                };
                if state.editor_drag.is_some() {
                    unsafe { SetCapture(window) };
                    update_editor_drag(state, x);
                    redraw(window);
                }
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            if let Some(state) = state_mut(window)
                && state.editor_drag.is_some()
            {
                let scale = state.dpi as f32 / 96.0;
                let x = signed_low_word(lparam.0) as f32 / scale;
                update_editor_drag(state, x);
                redraw(window);
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            if let Some(state) = state_mut(window) {
                let scale = state.dpi as f32 / 96.0;
                let x = signed_low_word(lparam.0) as f32 / scale;
                let y = signed_high_word(lparam.0) as f32 / scale;
                if state.editor_drag.is_some() {
                    update_editor_drag(state, x);
                    state.editor_drag = None;
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
                    state.context_clip = Some(index);
                    show_clip_menu(window, &state.model);
                }
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            if let Some(state) = state_mut(window) {
                handle_clip_command(window, state, wparam.0 & 0xffff);
            }
            LRESULT(0)
        }
        WM_CHAR => {
            if let Some(state) = state_mut(window) {
                handle_character(state, wparam.0 as u32);
                redraw(window);
            }
            LRESULT(0)
        }
        WM_KEYDOWN | WM_SYSKEYDOWN
            if state_mut(window).is_some_and(|state| state.model.hotkey_capture) =>
        {
            if let Some(state) = state_mut(window) {
                if capture_hotkey(&mut state.model, wparam.0 as u32) {
                    let shortcut =
                        wreath_windows::hotkey::localized_hotkey_label(&state.model.config.hotkey);
                    save_settings(&mut state.model, &format!("{shortcut} is saved and active"));
                }
                redraw(window);
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            if wparam.0 == 0x20 {
                if let Some(state) = state_mut(window)
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
            } else if wparam.0 == 0x1b {
                if let Some(state) = state_mut(window) {
                    state.model.search_focused = false;
                    state.model.hotkey_capture = false;
                    state.model.notice = None;
                    state.model.pending_delete = None;
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
                if trim_changed
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
                stop_player(state);
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
        Action::SettingsSection(section) => state.model.settings_section = section,
        Action::OpenClip(index) => {
            state.model.open_clip(index);
            open_current_clip(state);
        }
        Action::Back => {
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
            Ok(Response::Saved { path }) => {
                let result = state.model.refresh();
                if result.is_ok() {
                    state.renderer.retry_unavailable_thumbnails();
                }
                set_result(
                    &mut state.model,
                    result,
                    &format!("Saved {}", path.display()),
                );
                wreath_windows::feedback::broadcast_clip_saved();
            }
            Ok(Response::Error { message }) | Err(message) => state.model.notice = Some(message),
            Ok(_) => {}
        },
        Action::OpenClipsFolder => open_path(&state.model.config.storage.directory),
        Action::Search => state.model.search_focused = true,
        Action::ClearSearch => {
            state.model.query.clear();
            state.model.active_collection = None;
        }
        Action::DismissNotice => state.model.notice = None,
        Action::ToggleCursor => {
            state.model.config.capture.cursor = !state.model.config.capture.cursor
        }
        Action::ToggleDesktopAudio => {
            state.model.config.audio.desktop = !state.model.config.audio.desktop
        }
        Action::ToggleMicrophone => {
            state.model.config.audio.microphone = !state.model.config.audio.microphone
        }
        Action::ChooseDuration => choose_duration(window, &mut state.model),
        Action::ChooseFrameRate => choose_frame_rate(window, &mut state.model),
        Action::ChooseCodec => choose_codec(window, &mut state.model),
        Action::ChooseQuality => choose_quality(window, &mut state.model),
        Action::ChooseDisplay => choose_display(window, &mut state.model),
        Action::ChooseMicrophone => choose_microphone(window, &mut state.model),
        Action::ChooseMicrophoneGain => choose_microphone_gain(window, &mut state.model),
        Action::ChooseStorageLimit => choose_storage_limit(window, &mut state.model),
        Action::CaptureHotkey => {
            state.model.hotkey_capture = true;
            state.model.notice =
                Some("Press any keyboard key or combination now; Escape cancels".into());
        }
        Action::ClearHotkey => {
            state.model.config.hotkey = wreath_core::config::HotkeyConfig::unbound();
            save_settings(&mut state.model, "Save replay shortcut unbound");
        }
        Action::ChooseStorage => choose_storage(&mut state.model),
        Action::SaveSettings => {
            save_settings(&mut state.model, "Settings saved and capture reloaded")
        }
        Action::CreateCollection => create_collection(window, &mut state.model),
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
        Action::PlayPause => match state.player.as_ref().map(Player::toggle) {
            Some(Err(error)) => state.model.notice = Some(error),
            None => state.model.notice = Some("No clip is loaded".into()),
            Some(Ok(())) => {}
        },
        Action::SeekPercent(percent) => {
            if let Some(player) = &state.player
                && let Err(error) = player.seek_fraction(f64::from(percent) / 100.0)
            {
                state.model.notice = Some(error);
            }
            sync_player_state(state);
        }
        Action::EditActiveClip => begin_editor(state),
        Action::DragEditorStart | Action::DragEditorEnd => {}
        Action::SaveCut => save_cut(state),
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
    let _ = std::thread::Builder::new()
        .name("wreath-editor-timing".into())
        .spawn(move || {
            use wreath_core::trim::TrimBackend;

            let result = wreath_windows::trim::MediaFoundationTrimmer::new()
                .and_then(|backend| backend.timing(&source))
                .map_err(|error| error.to_string());
            let _ = updates.send(TrimUpdate::Timing { source, result });
        });
}

fn save_cut(state: &mut AppState) {
    if state.model.editor_working || state.model.editor_timing.is_none() {
        return;
    }
    let Some(source) = state.model.active_clip().map(|clip| clip.path.clone()) else {
        state.model.notice = Some("Clip is no longer available".into());
        return;
    };
    let request = wreath_core::trim::TrimRequest {
        source: source.clone(),
        start: state.model.editor_start,
        end: state.model.editor_end,
        mode: wreath_core::trim::TrimMode::Auto,
        output: wreath_core::trim::TrimOutput::NewClip(None),
    };
    let thumbnails = state.model.paths.thumbnail_dir.clone();
    let updates = state.trim_sender.clone();
    state.model.editor_working = true;
    state.model.notice = Some("Cutting on a background worker…".into());
    let _ = std::thread::Builder::new()
        .name("wreath-editor-cut".into())
        .spawn(move || {
            let result = wreath_windows::trim::MediaFoundationTrimmer::new()
                .and_then(|backend| wreath_core::trim::trim(&backend, &request, &thumbnails))
                .map_err(|error| error.to_string());
            let _ = updates.send(TrimUpdate::Finished { source, result });
        });
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
            TrimUpdate::Finished { source, result } if active.as_ref() == Some(&source) => {
                changed = true;
                state.model.editor_working = false;
                match result {
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
                                "Re-encoded for an exact start"
                            } else {
                                "Losslessly cut"
                            }
                        );
                        set_result(&mut state.model, result, &message);
                    }
                    Err(error) => state.model.notice = Some(format!("Cannot cut clip: {error}")),
                }
            }
            _ => {}
        }
    }
    changed
}

fn capture_hotkey(model: &mut UiModel, virtual_key: u32) -> bool {
    match virtual_key {
        0x1b => {
            model.hotkey_capture = false;
            model.notice = Some("Shortcut change cancelled".into());
            false
        }
        0x10 | 0x11 | 0x12 | 0x5b | 0x5c => false,
        key => {
            let Some(key_name) = wreath_windows::hotkey::key_name_from_virtual_key(key) else {
                model.notice = Some("That key cannot be used as a Windows shortcut".into());
                return false;
            };
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
            let hotkey = wreath_core::config::HotkeyConfig {
                modifiers,
                key: key_name,
            };
            if hotkey != model.config.hotkey
                && let Err(error) = wreath_windows::hotkey::HotkeyRegistration::register(2, &hotkey)
            {
                model.notice = Some(format!("That shortcut is unavailable: {error}"));
                return false;
            }
            model.config.hotkey = hotkey;
            model.hotkey_capture = false;
            true
        }
    }
}

fn key_pressed(virtual_key: i32) -> bool {
    (unsafe { GetKeyState(virtual_key) }) < 0
}

fn create_collection(window: HWND, model: &mut UiModel) {
    let name = match crate::input_dialog::prompt(window, "New collection", "Collection name", "") {
        Ok(Some(name)) => name,
        Ok(None) => return,
        Err(error) => {
            model.notice = Some(error);
            return;
        }
    };
    match wreath_core::clips::create_collection(&model.config.storage.directory, &name) {
        Ok(path) => {
            if let Err(error) = model.refresh() {
                model.notice = Some(error);
            } else {
                model.active_collection = Some(path);
                model.notice = Some("Collection created".into());
            }
        }
        Err(error) => model.notice = Some(format!("Cannot create collection: {error}")),
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

fn show_clip_menu(window: HWND, model: &UiModel) {
    let Ok(menu) = (unsafe { CreatePopupMenu() }) else {
        return;
    };
    append_menu_item(menu, COMMAND_CLIP_RENAME, "Rename");
    append_menu_item(menu, COMMAND_CLIP_EDIT, "Edit clip");
    append_menu_item(menu, COMMAND_CLIP_MOVE_LIBRARY, "Move to Library");
    for (index, collection) in model.collections.iter().take(64).enumerate() {
        append_menu_item(
            menu,
            COMMAND_CLIP_MOVE_COLLECTION_BASE + index,
            &format!("Move to {}", collection.name),
        );
    }
    let _ = unsafe { AppendMenuW(menu, MF_SEPARATOR, 0, None) };
    append_menu_item(menu, COMMAND_CLIP_DELETE, "Delete clip");
    let mut point = POINT::default();
    if unsafe { GetCursorPos(&mut point) }.is_ok() {
        unsafe {
            let _ = SetForegroundWindow(window);
            let _ = TrackPopupMenu(menu, TPM_RIGHTBUTTON, point.x, point.y, None, window, None);
        }
    }
    let _ = unsafe { DestroyMenu(menu) };
}

fn append_menu_item(menu: HMENU, command: usize, label: &str) {
    let label = wide(label);
    let _ = unsafe { AppendMenuW(menu, MF_STRING, command, PCWSTR(label.as_ptr())) };
}

fn handle_clip_command(window: HWND, state: &mut AppState, command: usize) {
    let Some(index) = state.context_clip else {
        return;
    };
    let Some(clip) = state.model.clips.get(index).cloned() else {
        return;
    };
    if command == COMMAND_CLIP_DELETE {
        state.model.pending_delete = Some(DeleteTarget::Clip(index));
        state.context_clip = None;
        redraw(window);
        return;
    }
    if command == COMMAND_CLIP_EDIT {
        state.model.open_clip(index);
        open_current_clip(state);
        begin_editor(state);
        state.context_clip = None;
        redraw(window);
        return;
    }
    let result = match command {
        COMMAND_CLIP_RENAME => {
            let name = match crate::input_dialog::prompt(
                window,
                "Rename clip",
                "Clip name",
                &clip.title,
            ) {
                Ok(Some(name)) => name,
                Ok(None) => return,
                Err(error) => {
                    state.model.notice = Some(error);
                    return;
                }
            };
            wreath_core::clips::rename(&clip, &name, &state.model.paths.thumbnail_dir)
                .map(|_| "Clip renamed")
        }
        COMMAND_CLIP_MOVE_LIBRARY => wreath_core::clips::move_to_library(
            &clip,
            &state.model.config.storage.directory,
            &state.model.paths.thumbnail_dir,
        )
        .map(|_| "Clip moved to Library"),
        command if command >= COMMAND_CLIP_MOVE_COLLECTION_BASE => {
            let collection_index = command - COMMAND_CLIP_MOVE_COLLECTION_BASE;
            let Some(collection) = state.model.collections.get(collection_index) else {
                return;
            };
            wreath_core::clips::move_to_collection(
                &clip,
                &state.model.config.storage.directory,
                &collection.path,
                &state.model.paths.thumbnail_dir,
            )
            .map(|_| "Clip moved")
        }
        _ => return,
    };
    match result {
        Ok(message) => {
            let refresh = state.model.refresh();
            set_result(&mut state.model, refresh, message);
        }
        Err(error) => state.model.notice = Some(error.to_string()),
    }
    state.context_clip = None;
    redraw(window);
}

fn open_current_clip(state: &mut AppState) {
    update_player_window(state);
    let Some(path) = state.model.active_clip().map(|clip| clip.path.clone()) else {
        return;
    };
    if let Some(player) = &mut state.player
        && let Err(error) = player.open(&path)
    {
        state.model.notice = Some(error);
    }
}

fn stop_player(state: &mut AppState) {
    if let Some(player) = &state.player
        && let Err(error) = player.stop()
    {
        state.model.notice = Some(format!("Cannot stop clip playback: {error}"));
    }
    sync_player_state(state);
}

fn update_editor_drag(state: &mut AppState, x: f32) {
    let Some(handle) = state.editor_drag else {
        return;
    };
    let scale = state.dpi as f32 / 96.0;
    let width = ((state.width as f32 / scale).round() as u32).max(1);
    let height = ((state.height as f32 / scale).round() as u32).max(1);
    let rail = editor_timeline_rail(width, height, state.model.player_aspect_ratio);
    let thousandths = editor_timeline_fraction(rail, x);
    match handle {
        EditorDrag::Start => state.model.set_editor_start(thousandths),
        EditorDrag::End => state.model.set_editor_end(thousandths),
    }
    if let Some(player) = &state.player
        && let Some(timing) = &state.model.editor_timing
    {
        let fraction = state.model.editor_start.as_secs_f64()
            / timing.duration.as_secs_f64().max(f64::EPSILON);
        let _ = player.seek_fraction(fraction);
        let _ = player.play();
    }
}

fn keep_editor_preview_inside_selection(state: &mut AppState) {
    if state.model.page != crate::model::Page::Editor || state.model.editor_timing.is_none() {
        return;
    }
    let position = state.model.player_position_seconds;
    let start = state.model.editor_start.as_secs_f64();
    let end = state.model.editor_end.as_secs_f64();
    if position + 0.02 < start || position >= end - 0.02 {
        if let Some(player) = &state.player {
            let duration = state.model.player_duration_seconds.max(f64::EPSILON);
            let _ = player.seek_fraction(start / duration);
            let _ = player.play();
        }
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
        unsafe {
            let _ = ShowWindow(window, SW_HIDE);
        }
        return;
    }
    let scale = state.dpi as f32 / 96.0;
    let logical_width = (state.width as f32 / scale).round() as u32;
    let logical_height = (state.height as f32 / scale).round() as u32;
    let bounds = if state.model.page == crate::model::Page::Editor {
        editor_player_bounds(
            logical_width,
            logical_height,
            state.model.player_aspect_ratio,
        )
    } else {
        player_bounds(
            logical_width,
            logical_height,
            state.model.player_aspect_ratio,
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

fn sync_player_state(state: &mut AppState) {
    let Some(player) = &state.player else {
        return;
    };
    let snapshot = player.snapshot();
    state.model.player_ready = snapshot.ready;
    state.model.player_playing = snapshot.playing;
    state.model.player_position_seconds = snapshot.position_seconds;
    state.model.player_duration_seconds = snapshot.duration_seconds;
    state.model.player_aspect_ratio = snapshot.aspect_ratio;
}

fn handle_character(state: &mut AppState, character: u32) {
    if !state.model.search_focused {
        return;
    }
    match character {
        8 => {
            state.model.query.pop();
        }
        13 => state.model.search_focused = false,
        32..=0x10ffff => {
            if let Some(character) = char::from_u32(character)
                && !character.is_control()
                && state.model.query.chars().count() < 80
            {
                state.model.query.push(character);
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

fn choose_duration(window: HWND, model: &mut UiModel) {
    let values = [15, 30, 45, 60, 90, 120];
    let labels = values
        .iter()
        .map(|seconds| format!("{seconds} seconds"))
        .collect::<Vec<_>>();
    let current = values
        .iter()
        .position(|value| *value == model.config.capture.duration_seconds);
    if let Some(index) = show_choice_menu(window, &labels, current) {
        model.config.capture.duration_seconds = values[index];
    }
}

fn choose_frame_rate(window: HWND, model: &mut UiModel) {
    let values = model.frame_rate_options();
    let labels = values
        .iter()
        .map(|rate| format!("{rate} fps"))
        .collect::<Vec<_>>();
    let current = values
        .iter()
        .position(|value| *value == model.config.capture.frames_per_second);
    if let Some(index) = show_choice_menu(window, &labels, current) {
        model.config.capture.frames_per_second = values[index];
    }
}

fn choose_codec(window: HWND, model: &mut UiModel) {
    let values = [Codec::Auto, Codec::H264, Codec::Hevc, Codec::Av1];
    let labels = ["Auto (recommended)", "H.264", "HEVC", "AV1"]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let current = values
        .iter()
        .position(|value| *value == model.config.capture.codec);
    if let Some(index) = show_choice_menu(window, &labels, current) {
        model.config.capture.codec = values[index];
    }
}

fn choose_quality(window: HWND, model: &mut UiModel) {
    let options = model.quality_options();
    let labels = options
        .iter()
        .map(|(_, label)| label.clone())
        .collect::<Vec<_>>();
    let current = options
        .iter()
        .position(|(value, _)| *value == model.config.capture.quality);
    if let Some(index) = show_choice_menu(window, &labels, current) {
        model.config.capture.quality = options[index].0;
    }
}

fn choose_display(window: HWND, model: &mut UiModel) {
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
    let Some(index) = show_choice_menu(window, &labels, current) else {
        return;
    };
    let display = &model.displays[index];
    model.config.capture.monitor = Some(display.name.clone());
    let native_rate =
        (display.refresh_rate.round() as u16).clamp(15, wreath_core::config::MAX_FRAMES_PER_SECOND);
    model.config.capture.frames_per_second =
        model.config.capture.frames_per_second.min(native_rate);
}

fn choose_microphone(window: HWND, model: &mut UiModel) {
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
    if let Some(index) = show_choice_menu(window, &labels, current) {
        model.config.audio.microphone_device = index
            .checked_sub(1)
            .and_then(|index| model.microphone_names.get(index))
            .map(|(id, _)| id.clone());
    }
}

fn choose_microphone_gain(window: HWND, model: &mut UiModel) {
    let values = [25, 50, 75, 100];
    let labels = values
        .iter()
        .map(|gain| format!("{gain}%"))
        .collect::<Vec<_>>();
    let current = values
        .iter()
        .position(|value| *value == model.config.audio.microphone_gain_percent);
    if let Some(index) = show_choice_menu(window, &labels, current) {
        model.config.audio.microphone_gain_percent = values[index];
    }
}

fn choose_storage_limit(window: HWND, model: &mut UiModel) {
    let values = [1_024, 5_120, 10_240, 25_600, 51_200, 102_400];
    let labels = ["1 GB", "5 GB", "10 GB", "25 GB", "50 GB", "100 GB"]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let current = values
        .iter()
        .position(|value| *value == model.config.storage.max_megabytes);
    if let Some(index) = show_choice_menu(window, &labels, current) {
        model.config.storage.max_megabytes = values[index];
    }
}

fn show_choice_menu(window: HWND, labels: &[String], current: Option<usize>) -> Option<usize> {
    let Ok(menu) = (unsafe { CreatePopupMenu() }) else {
        return None;
    };
    for (index, label) in labels.iter().enumerate() {
        let label = wide(label);
        let flags = if current == Some(index) {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING
        };
        let _ = unsafe { AppendMenuW(menu, flags, index + 1, PCWSTR(label.as_ptr())) };
    }
    let mut point = POINT::default();
    let selected = if unsafe { GetCursorPos(&mut point) }.is_ok() {
        unsafe {
            let _ = SetForegroundWindow(window);
            TrackPopupMenu(
                menu,
                TPM_RIGHTBUTTON | TPM_RETURNCMD,
                point.x,
                point.y,
                None,
                window,
                None,
            )
            .0 as usize
        }
    } else {
        0
    };
    let _ = unsafe { DestroyMenu(menu) };
    selected
        .checked_sub(1)
        .filter(|index| *index < labels.len())
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
