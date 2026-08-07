use std::path::Path;

use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, RECT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT, UpdateWindow};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DispatchMessageW, FindWindowW,
    GWLP_USERDATA, GetClientRect, GetMessageW, GetWindowLongPtrW, MSG, PostQuitMessage,
    RegisterClassW, SW_HIDE, SW_RESTORE, SW_SHOW, SWP_NOZORDER, SetForegroundWindow,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage, WINDOW_EX_STYLE, WM_CHAR,
    WM_DESTROY, WM_KEYDOWN, WM_LBUTTONUP, WM_NCCREATE, WM_PAINT, WM_SIZE, WNDCLASSW, WS_CHILD,
    WS_DISABLED, WS_OVERLAPPEDWINDOW,
};
use windows::core::{PCWSTR, w};
use wreath_core::config::Codec;
use wreath_core::ipc::{Request, Response};

use crate::model::{Action, UiModel};
use crate::player::{PLAYER_EVENT, Player};
use crate::renderer::{Renderer, player_bounds};

const WINDOW_CLASS: windows::core::PCWSTR = w!("WreathApplicationWindow");

struct AppState {
    model: UiModel,
    renderer: Renderer,
    width: u32,
    height: u32,
    player: Option<Player>,
    video_window: Option<HWND>,
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

    let class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        lpszClassName: WINDOW_CLASS,
        ..Default::default()
    };
    if unsafe { RegisterClassW(&class) } == 0 {
        let _ = unsafe { CloseHandle(single_instance) };
        return Err(std::io::Error::last_os_error().to_string());
    }

    let state = Box::new(AppState {
        model: UiModel::load()?,
        renderer: Renderer::new()?,
        width: 1280,
        height: 760,
        player: None,
        video_window: None,
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
        (*state).player = Some(Player::new(video_window, window));
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
        WM_SIZE => {
            if let Some(state) = state_mut(window) {
                state.width = low_word(lparam.0) as u32;
                state.height = high_word(lparam.0) as u32;
                state.renderer.resize(state.width, state.height);
                update_player_window(state);
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
                if let Err(error) = state.renderer.paint(
                    window,
                    &state.model,
                    state.width.max(1),
                    state.height.max(1),
                ) {
                    state.model.notice = Some(format!("Rendering failed: {error}"));
                }
            }
            let _ = unsafe { EndPaint(window, &paint) };
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            if let Some(state) = state_mut(window) {
                let x = signed_low_word(lparam.0) as f32;
                let y = signed_high_word(lparam.0) as f32;
                if let Some(action) = state.renderer.hit_test(x, y) {
                    handle_action(window, state, action);
                }
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
        WM_KEYDOWN => {
            if wparam.0 == 0x20 {
                if let Some(state) = state_mut(window)
                    && state.model.page == crate::model::Page::Player
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
                }
                redraw(window);
            }
            LRESULT(0)
        }
        PLAYER_EVENT => {
            if let Some(state) = state_mut(window)
                && let Some(player) = &state.player
            {
                player.update_video();
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
            state.model.navigate(page);
            update_player_window(state);
        }
        Action::SettingsSection(section) => state.model.settings_section = section,
        Action::OpenClip(index) => {
            state.model.open_clip(index);
            open_current_clip(state);
        }
        Action::Back => {
            state.model.navigate(state.model.previous_page);
            update_player_window(state);
        }
        Action::Refresh => {
            let result = state.model.refresh();
            set_result(&mut state.model, result, "Library refreshed");
        }
        Action::SaveReplay => match send(Request::Save) {
            Ok(Response::Saved { path }) => {
                state.model.notice = Some(format!("Saved {}", path.display()))
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
        Action::ToggleCursor => {
            state.model.config.capture.cursor = !state.model.config.capture.cursor
        }
        Action::ToggleDesktopAudio => {
            state.model.config.audio.desktop = !state.model.config.audio.desktop
        }
        Action::ToggleMicrophone => {
            state.model.config.audio.microphone = !state.model.config.audio.microphone
        }
        Action::CycleDuration => {
            state.model.config.capture.duration_seconds = cycle(
                &[15, 30, 45, 60, 90, 120],
                state.model.config.capture.duration_seconds,
            )
        }
        Action::CycleFrameRate => {
            state.model.config.capture.frames_per_second = cycle(
                &[30, 60, 120, 144],
                state.model.config.capture.frames_per_second,
            )
        }
        Action::CycleCodec => {
            state.model.config.capture.codec = match state.model.config.capture.codec {
                Codec::Auto => Codec::H264,
                Codec::H264 => Codec::Hevc,
                Codec::Hevc => Codec::Av1,
                Codec::Av1 => Codec::Auto,
            }
        }
        Action::CycleQuality => {
            state.model.config.capture.quality =
                cycle(&[50, 65, 75, 85, 95], state.model.config.capture.quality)
        }
        Action::CycleDisplay => cycle_display(&mut state.model),
        Action::CycleMicrophone => cycle_microphone(&mut state.model),
        Action::CycleMicrophoneGain => {
            state.model.config.audio.microphone_gain_percent = cycle(
                &[50, 75, 100, 125, 150, 200],
                state.model.config.audio.microphone_gain_percent,
            )
        }
        Action::CaptureHotkey => {
            state.model.hotkey_capture = true;
            state.model.notice = Some("Shortcut capture active — press Escape to cancel".into());
        }
        Action::ChooseStorage => choose_storage(&mut state.model),
        Action::SaveSettings => save_settings(&mut state.model),
        Action::CreateCollection => {
            state.model.notice = Some("Type a collection name, then press Enter".into())
        }
        Action::DeleteActiveCollection => {
            state.model.notice = Some("Collection deletion requires confirmation".into())
        }
        Action::PlayPause => match state.player.as_ref().map(Player::toggle) {
            Some(Err(error)) => state.model.notice = Some(error),
            None => state.model.notice = Some("No clip is loaded".into()),
            Some(Ok(())) => {}
        },
    }
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

fn update_player_window(state: &mut AppState) {
    let Some(window) = state.video_window else {
        return;
    };
    if state.model.page != crate::model::Page::Player {
        unsafe {
            let _ = ShowWindow(window, SW_HIDE);
        }
        return;
    }
    let bounds = player_bounds(state.width, state.height);
    let _ = unsafe {
        SetWindowPos(
            window,
            None,
            bounds.left.round() as i32,
            bounds.top.round() as i32,
            (bounds.right - bounds.left).round().max(1.0) as i32,
            (bounds.bottom - bounds.top).round().max(1.0) as i32,
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

fn save_settings(model: &mut UiModel) {
    match model.config.save(&model.paths) {
        Ok(()) => match send(Request::Reload) {
            Ok(Response::Ok) => model.notice = Some("Settings saved and capture reloaded".into()),
            Ok(Response::Error { message }) | Err(message) => {
                model.notice = Some(format!("Saved, but reload failed: {message}"))
            }
            Ok(_) => model.notice = Some("Settings saved".into()),
        },
        Err(error) => model.notice = Some(format!("Cannot save settings: {error}")),
    }
}

fn cycle_display(model: &mut UiModel) {
    match wreath_windows::display::displays() {
        Ok(displays) if !displays.is_empty() => {
            let current = model.config.capture.monitor.as_deref();
            let next = displays
                .iter()
                .position(|display| Some(display.name.as_str()) == current)
                .map_or(0, |index| (index + 1) % displays.len());
            model.config.capture.monitor = Some(displays[next].name.clone());
        }
        Ok(_) => model.notice = Some("Windows reported no displays".into()),
        Err(error) => model.notice = Some(error.to_string()),
    }
}

fn cycle_microphone(model: &mut UiModel) {
    match wreath_windows::audio::microphones() {
        Ok(devices) if !devices.is_empty() => {
            let current = model.config.audio.microphone_device.as_deref();
            let next = devices
                .iter()
                .position(|device| Some(device.id.as_str()) == current)
                .map_or(0, |index| (index + 1) % devices.len());
            model.config.audio.microphone_device = Some(devices[next].id.clone());
        }
        Ok(_) => model.notice = Some("Windows reported no microphones".into()),
        Err(error) => model.notice = Some(error.to_string()),
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

fn cycle<T: Copy + PartialEq>(values: &[T], current: T) -> T {
    values
        .iter()
        .position(|value| *value == current)
        .map_or(values[0], |index| values[(index + 1) % values.len()])
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
