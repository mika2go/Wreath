use std::path::Path;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, POINT, WPARAM,
};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_ERROR, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CREATESTRUCTW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
    DestroyWindow, DispatchMessageW, GWLP_USERDATA, GetCursorPos, GetMessageW, GetWindowLongPtrW,
    HMENU, MF_SEPARATOR, MF_STRING, MSG, PostQuitMessage, RegisterClassW, RegisterWindowMessageW,
    SetForegroundWindow, SetTimer, SetWindowLongPtrW, TPM_BOTTOMALIGN, TPM_RIGHTBUTTON,
    TrackPopupMenu, TranslateMessage, WINDOW_STYLE, WM_APP, WM_COMMAND, WM_DESTROY,
    WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_NCCREATE, WM_RBUTTONUP, WM_TIMER, WNDCLASSW,
    WS_EX_TOOLWINDOW,
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
const COMMAND_TOGGLE_ELEVATED: usize = 107;
const COMMAND_EXIT: usize = 109;

struct AppState {
    paths: AppPaths,
    clips_directory: std::path::PathBuf,
    icon: NOTIFYICONDATAW,
    icon_added: bool,
    recovery: crate::recovery::RecoveryThrottle,
    strings: &'static crate::text::Strings,
}

fn taskbar_created_message() -> u32 {
    static MESSAGE: std::sync::OnceLock<u32> = std::sync::OnceLock::new();

    *MESSAGE.get_or_init(|| unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) })
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
    match crate::autostart::repair() {
        Ok(true) => wreath_core::diagnostic!(
            "Wreath tray: the Windows startup entry pointed at another installation and now points here"
        ),
        Ok(false) => {}
        Err(error) => wreath_core::diagnostic!(
            "Wreath tray: cannot repair the Windows startup entry: {error}"
        ),
    }
    if let Err(error) = ensure_daemon() {
        wreath_core::diagnostic!("Wreath tray: cannot start the recorder yet: {error}");
    }
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
    let config = wreath_core::config::Config::load(&paths).map_err(|error| error.to_string())?;
    let clips_directory = config.storage.directory.clone();
    let state = Box::new(AppState {
        paths,
        clips_directory,
        icon: NOTIFYICONDATAW::default(),
        icon_added: false,
        recovery: crate::recovery::RecoveryThrottle::new(Instant::now()),
        strings: load_strings(),
    });
    let state = Box::into_raw(state);
    let window = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class_name,
            w!("Wreath"),
            WINDOW_STYLE::default(),
            0,
            0,
            0,
            0,
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
                state.icon = tray_icon(window, state.strings.tray_starting_up);
                state.icon_added = unsafe { Shell_NotifyIconW(NIM_ADD, &state.icon) }.as_bool();
            }
        }
        return LRESULT(1);
    }

    if message != 0 && message == taskbar_created_message() {
        if let Some(state) = state_mut(window) {
            let _ = unsafe { Shell_NotifyIconW(NIM_DELETE, &state.icon) };
            state.icon_added = false;
        }
        ensure_icon(window);
        return LRESULT(0);
    }

    match message {
        wreath_windows::feedback::CLIP_SAVED_MESSAGE => {
            wreath_windows::feedback::play_clip_saved_sound();
            LRESULT(0)
        }
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
            ensure_icon(window);
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
                notify_error(window, &error);
            }
        }
        COMMAND_SAVE => match send(Request::Save) {
            Ok(Response::Saved { path: _ }) => {
                wreath_windows::feedback::play_clip_saved_sound();
                wreath_windows::feedback::notify_app_clip_saved();
            }
            Ok(Response::Error { message }) | Err(message) => notify_error(window, &message),
            Ok(_) => {}
        },
        COMMAND_PAUSE => handle_simple(window, Request::Pause),
        COMMAND_RESUME => handle_simple(window, Request::Resume),
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
            if crate::autostart::elevated_is_enabled() {
                notify_error(window, strings(window).tray_elevated_blocks_autostart);
                return;
            }
            let enable = !crate::autostart::is_enabled();
            match crate::autostart::set_enabled(enable) {
                Ok(()) => {}
                Err(error) => notify_error(window, &error),
            }
        }
        COMMAND_TOGGLE_ELEVATED => {
            let enable = !crate::autostart::elevated_is_enabled();
            match crate::autostart::set_elevated(enable) {
                Ok(()) => {}
                Err(error) => notify_error(window, &error),
            }
        }
        COMMAND_RELOAD_CONFIG => {
            reload_strings(window);
            handle_simple(window, Request::Reload);
        }
        COMMAND_EXIT => {
            let _ = send(Request::Shutdown);
            let _ = unsafe { DestroyWindow(window) };
        }
        _ => {}
    }
    refresh_status(window);
}

fn handle_simple(window: HWND, request: Request) {
    match send(request) {
        Ok(Response::Ok) => {}
        Ok(Response::Error { message }) | Err(message) => notify_error(window, &message),
        Ok(_) => {}
    }
}

fn ensure_icon(window: HWND) {
    if let Some(state) = state_mut(window)
        && !state.icon_added
    {
        state.icon.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        state.icon_added = unsafe { Shell_NotifyIconW(NIM_ADD, &state.icon) }.as_bool();
    }
}

/// Reads the interface language from the configuration. The result is cached in
/// the tray state so the status tick does not parse the file every time.
fn load_strings() -> &'static crate::text::Strings {
    let language = wreath_core::config::Config::load(&wreath_core::paths::AppPaths::discover())
        .map(|config| config.appearance.language)
        .unwrap_or_default();
    crate::text::strings(crate::text::resolve(language))
}

fn strings(window: HWND) -> &'static crate::text::Strings {
    state_mut(window).map_or_else(load_strings, |state| state.strings)
}

fn reload_strings(window: HWND) {
    let strings = load_strings();
    if let Some(state) = state_mut(window) {
        state.strings = strings;
    }
}

fn refresh_status(window: HWND) {
    let text = strings(window);
    let tooltip = match send(Request::Status) {
        Ok(Response::Status {
            state,
            monitor,
            source,
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
                        Ok(Response::Ok) => text.tray_recovering.to_owned(),
                        Ok(Response::Error { message }) | Err(message) => {
                            format!("{}: {detail} · {message}", text.tray_error_title)
                        }
                        Ok(_) => format!("{}: {detail}", text.tray_error_title),
                    }
                } else {
                    format!("{}: {detail}", text.tray_error_title)
                }
            } else {
                reset_recovery(window);
                let state = match state {
                    DaemonState::Starting => text.tray_state_starting,
                    DaemonState::Recording => text.tray_state_recording,
                    DaemonState::Paused => text.tray_state_paused,
                    DaemonState::Error => text.tray_state_error,
                };
                let codec = codec.as_deref().unwrap_or("no codec");
                let adapter = adapter
                    .as_ref()
                    .map(|adapter| adapter.name.as_str())
                    .unwrap_or("no adapter");
                format!(
                    "wreath — {state} — {} — {codec} — {adapter} — {}s",
                    source
                        .as_deref()
                        .or(monitor.as_deref())
                        .unwrap_or("no display"),
                    buffered_seconds
                )
            }
        }
        _ => {
            if recovery_due(window)
                && let Err(error) = ensure_daemon()
            {
                wreath_core::diagnostic!("Wreath tray: cannot start the recorder: {error}");
            }
            text.tray_unavailable.to_owned()
        }
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

fn notify_error(window: HWND, detail: &str) {
    if let Some(state) = state_mut(window) {
        let title = state.strings.tray_error_title;
        copy_wide(&mut state.icon.szInfoTitle, title);
        copy_wide(&mut state.icon.szInfo, detail);
        state.icon.uFlags = NIF_INFO;
        state.icon.dwInfoFlags = NIIF_ERROR;
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
    reload_strings(window);
    let text = strings(window);
    append_item(menu, COMMAND_OPEN_APP, text.tray_open_app);
    append_item(menu, COMMAND_SAVE, text.tray_save_replay);
    append_separator(menu);
    append_item(menu, COMMAND_PAUSE, text.tray_pause);
    append_item(menu, COMMAND_RESUME, text.tray_resume);
    append_separator(menu);
    append_item(menu, COMMAND_OPEN_CLIPS, text.tray_open_clips);
    append_item(menu, COMMAND_OPEN_CONFIG, text.tray_open_config);
    append_item(menu, COMMAND_RELOAD_CONFIG, text.tray_reload_config);
    let elevated = crate::autostart::elevated_is_enabled();
    append_item(
        menu,
        COMMAND_TOGGLE_AUTOSTART,
        if crate::autostart::is_enabled() || elevated {
            text.tray_autostart_disable
        } else {
            text.tray_autostart_enable
        },
    );
    append_item(
        menu,
        COMMAND_TOGGLE_ELEVATED,
        if elevated {
            text.tray_elevated_disable
        } else {
            text.tray_elevated_enable
        },
    );
    append_separator(menu);
    append_item(menu, COMMAND_EXIT, text.tray_exit);
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
