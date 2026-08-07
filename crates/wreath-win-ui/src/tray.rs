use std::path::Path;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, POINT, WPARAM,
};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_ERROR, NIIF_INFO, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CREATESTRUCTW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
    DestroyWindow, DispatchMessageW, GWLP_USERDATA, GetCursorPos, GetMessageW, GetWindowLongPtrW,
    HMENU, HWND_MESSAGE, MF_SEPARATOR, MF_STRING, MSG, PostQuitMessage, RegisterClassW,
    SetForegroundWindow, SetTimer, SetWindowLongPtrW, TPM_BOTTOMALIGN, TPM_RIGHTBUTTON,
    TrackPopupMenu, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_COMMAND,
    WM_DESTROY, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_NCCREATE, WM_RBUTTONUP, WM_TIMER, WNDCLASSW,
};
use windows::core::{PCWSTR, w};
use wreath_core::ipc::{DaemonState, Request, Response};
use wreath_core::paths::AppPaths;

const TRAY_MESSAGE: u32 = WM_APP + 1;
const TRAY_ID: u32 = 1;
const STATUS_TIMER: usize = 1;
const STATUS_TIMER_INTERVAL_MS: u32 = 5_000;
const RECOVERY_RETRY_INTERVAL: Duration = Duration::from_secs(30);
const COMMAND_OPEN_APP: usize = 99;
const COMMAND_SAVE: usize = 100;
const COMMAND_PAUSE: usize = 101;
const COMMAND_RESUME: usize = 102;
const COMMAND_OPEN_CLIPS: usize = 103;
const COMMAND_OPEN_CONFIG: usize = 104;
const COMMAND_TOGGLE_AUTOSTART: usize = 105;
const COMMAND_RELOAD_CONFIG: usize = 106;
const COMMAND_EXIT: usize = 109;

struct AppState {
    paths: AppPaths,
    clips_directory: std::path::PathBuf,
    icon: NOTIFYICONDATAW,
    recovery: crate::recovery::RecoveryThrottle,
}

pub fn run() -> Result<(), String> {
    let single_instance = unsafe {
        windows::Win32::System::Threading::CreateMutexW(None, false, w!("Local\\WreathTray"))
    }
    .map_err(|error| error.to_string())?;
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        let _ = unsafe { CloseHandle(single_instance) };
        return Ok(());
    }
    ensure_daemon()?;
    let class_name = w!("WreathTrayWindow");
    let class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        lpszClassName: class_name,
        ..Default::default()
    };
    if unsafe { RegisterClassW(&class) } == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }

    let paths = AppPaths::discover();
    let clips_directory = wreath_core::config::Config::load(&paths)
        .map_err(|error| error.to_string())?
        .storage
        .directory;
    let state = Box::new(AppState {
        paths,
        clips_directory,
        icon: NOTIFYICONDATAW::default(),
        recovery: crate::recovery::RecoveryThrottle::new(Instant::now()),
    });
    let state = Box::into_raw(state);
    let window = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            w!("Wreath"),
            WINDOW_STYLE::default(),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            None,
            Some(state.cast()),
        )
    };
    let window = match window {
        Ok(window) => window,
        Err(error) => {
            let _ = unsafe { Box::from_raw(state) };
            return Err(error.to_string());
        }
    };
    if unsafe { SetTimer(Some(window), STATUS_TIMER, STATUS_TIMER_INTERVAL_MS, None) } == 0 {
        let _ = unsafe { DestroyWindow(window) };
        let _ = unsafe { Box::from_raw(state) };
        return Err(std::io::Error::last_os_error().to_string());
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

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = lparam.0 as *const CREATESTRUCTW;
        if !create.is_null() {
            let state = unsafe { (*create).lpCreateParams } as isize;
            unsafe { SetWindowLongPtrW(window, GWLP_USERDATA, state) };
            if let Some(state) = state_mut(window) {
                state.icon = tray_icon(window, "Wreath is starting");
                if !unsafe { Shell_NotifyIconW(NIM_ADD, &state.icon) }.as_bool() {
                    return LRESULT(0);
                }
            }
        }
        return LRESULT(1);
    }

    match message {
        WM_COMMAND => {
            handle_command(window, wparam.0 & 0xffff);
            LRESULT(0)
        }
        TRAY_MESSAGE => {
            let event = lparam.0 as u32;
            if event == WM_RBUTTONUP {
                show_menu(window);
            } else if event == WM_LBUTTONUP {
                handle_command(window, COMMAND_OPEN_APP);
            } else if event == WM_LBUTTONDBLCLK {
                handle_command(window, COMMAND_SAVE);
            }
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == STATUS_TIMER => {
            refresh_status(window);
            LRESULT(0)
        }
        WM_DESTROY => {
            if let Some(state) = state_mut(window) {
                let _ = unsafe { Shell_NotifyIconW(NIM_DELETE, &state.icon) };
            }
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

fn handle_command(window: HWND, command: usize) {
    match command {
        COMMAND_OPEN_APP => {
            if let Err(error) = open_app() {
                notify(window, "Wreath error", &error, true);
            }
        }
        COMMAND_SAVE => match send(Request::Save) {
            Ok(Response::Saved { path }) => {
                notify(window, "Clip saved", &path.display().to_string(), false)
            }
            Ok(Response::Error { message }) | Err(message) => {
                notify(window, "Wreath error", &message, true)
            }
            Ok(_) => {}
        },
        COMMAND_PAUSE => handle_simple(window, Request::Pause, "Capture paused"),
        COMMAND_RESUME => handle_simple(window, Request::Resume, "Capture resumed"),
        COMMAND_OPEN_CLIPS => {
            if let Some(state) = state_mut(window) {
                open_path(&state.clips_directory);
            }
        }
        COMMAND_OPEN_CONFIG => {
            if let Some(state) = state_mut(window) {
                open_path(&state.paths.config_file);
            }
        }
        COMMAND_TOGGLE_AUTOSTART => {
            let enable = !crate::autostart::is_enabled();
            match crate::autostart::set_enabled(enable) {
                Ok(()) => notify(
                    window,
                    "Wreath",
                    if enable {
                        "Wreath will start with Windows"
                    } else {
                        "Wreath autostart disabled"
                    },
                    false,
                ),
                Err(error) => notify(window, "Wreath error", &error, true),
            }
        }
        COMMAND_RELOAD_CONFIG => handle_simple(window, Request::Reload, "Settings reloaded"),
        COMMAND_EXIT => {
            let _ = send(Request::Shutdown);
            let _ = unsafe { DestroyWindow(window) };
        }
        _ => {}
    }
    refresh_status(window);
}

fn handle_simple(window: HWND, request: Request, confirmation: &str) {
    match send(request) {
        Ok(Response::Ok) => notify(window, "Wreath", confirmation, false),
        Ok(Response::Error { message }) | Err(message) => {
            notify(window, "Wreath error", &message, true)
        }
        Ok(_) => {}
    }
}

fn refresh_status(window: HWND) {
    let tooltip = match send(Request::Status) {
        Ok(Response::Status {
            state,
            monitor,
            codec,
            adapter,
            replay_bytes: _,
            buffered_seconds,
            error,
        }) => {
            if state == DaemonState::Error {
                let detail = error.unwrap_or_else(|| "capture pipeline stopped".into());
                if recovery_due(window) {
                    match send(Request::Reload) {
                        Ok(Response::Ok) => "Wreath — recovering capture".into(),
                        Ok(Response::Error { message }) | Err(message) => {
                            format!("Wreath error: {detail}; retry failed: {message}")
                        }
                        Ok(_) => format!("Wreath error: {detail}"),
                    }
                } else {
                    format!("Wreath error: {detail}")
                }
            } else {
                reset_recovery(window);
                let state = match state {
                    DaemonState::Starting => "Starting",
                    DaemonState::Recording => "Recording",
                    DaemonState::Paused => "Paused",
                    DaemonState::Error => "Error",
                };
                let codec = codec.as_deref().unwrap_or("no codec");
                let adapter = adapter
                    .as_ref()
                    .map(|adapter| adapter.name.as_str())
                    .unwrap_or("no adapter");
                format!(
                    "Wreath — {state} — {codec} — {adapter} — {}s — {}",
                    buffered_seconds,
                    monitor.as_deref().unwrap_or("no display")
                )
            }
        }
        _ => "Wreath — daemon unavailable".into(),
    };
    if let Some(state) = state_mut(window) {
        copy_wide(&mut state.icon.szTip, &tooltip);
        state.icon.uFlags = NIF_TIP;
        let _ = unsafe { Shell_NotifyIconW(NIM_MODIFY, &state.icon) };
    }
}

fn recovery_due(window: HWND) -> bool {
    let now = Instant::now();
    let Some(state) = state_mut(window) else {
        return false;
    };
    state.recovery.acquire(now, RECOVERY_RETRY_INTERVAL)
}

fn reset_recovery(window: HWND) {
    if let Some(state) = state_mut(window) {
        state.recovery.reset(Instant::now());
    }
}

fn notify(window: HWND, title: &str, detail: &str, error: bool) {
    if let Some(state) = state_mut(window) {
        copy_wide(&mut state.icon.szInfoTitle, title);
        copy_wide(&mut state.icon.szInfo, detail);
        state.icon.uFlags = NIF_INFO;
        state.icon.dwInfoFlags = if error { NIIF_ERROR } else { NIIF_INFO };
        let _ = unsafe { Shell_NotifyIconW(NIM_MODIFY, &state.icon) };
    }
}

fn tray_icon(window: HWND, tooltip: &str) -> NOTIFYICONDATAW {
    let mut icon = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: window,
        uID: TRAY_ID,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
        uCallbackMessage: TRAY_MESSAGE,
        hIcon: crate::icon::load(),
        ..Default::default()
    };
    copy_wide(&mut icon.szTip, tooltip);
    icon
}

fn show_menu(window: HWND) {
    let Ok(menu) = (unsafe { CreatePopupMenu() }) else {
        return;
    };
    append_item(menu, COMMAND_OPEN_APP, "Open Wreath");
    append_item(menu, COMMAND_SAVE, "Save replay");
    append_separator(menu);
    append_item(menu, COMMAND_PAUSE, "Pause capture");
    append_item(menu, COMMAND_RESUME, "Resume capture");
    append_separator(menu);
    append_item(menu, COMMAND_OPEN_CLIPS, "Open clips");
    append_item(menu, COMMAND_OPEN_CONFIG, "Open settings file");
    append_item(menu, COMMAND_RELOAD_CONFIG, "Reload settings");
    append_item(
        menu,
        COMMAND_TOGGLE_AUTOSTART,
        if crate::autostart::is_enabled() {
            "Disable start with Windows"
        } else {
            "Start with Windows"
        },
    );
    append_separator(menu);
    append_item(menu, COMMAND_EXIT, "Exit Wreath");
    let mut point = POINT::default();
    if unsafe { GetCursorPos(&mut point) }.is_ok() {
        unsafe {
            let _ = SetForegroundWindow(window);
            let _ = TrackPopupMenu(
                menu,
                TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
                point.x,
                point.y,
                None,
                window,
                None,
            );
        }
    }
    let _ = unsafe { DestroyMenu(menu) };
}

fn append_item(menu: HMENU, id: usize, label: &str) {
    let label = wide(label);
    let _ = unsafe { AppendMenuW(menu, MF_STRING, id, PCWSTR(label.as_ptr())) };
}

fn append_separator(menu: HMENU) {
    let _ = unsafe { AppendMenuW(menu, MF_SEPARATOR, 0, None) };
}

fn send(request: Request) -> Result<Response, String> {
    let paths = AppPaths::discover();
    wreath_windows::control::send_request(paths.pipe_name(), &request)
        .map_err(|error| error.to_string())
}

fn ensure_daemon() -> Result<(), String> {
    if send(Request::Status).is_ok() {
        return Ok(());
    }
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

fn open_app() -> Result<(), String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let application = executable
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("wreath-win-ui.exe");
    std::process::Command::new(&application)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("cannot start {}: {error}", application.display()))
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

fn copy_wide<const N: usize>(destination: &mut [u16; N], value: &str) {
    destination.fill(0);
    for (slot, unit) in destination
        .iter_mut()
        .take(N.saturating_sub(1))
        .zip(value.encode_utf16())
    {
        *slot = unit;
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
