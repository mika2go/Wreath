#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(target_os = "windows")]
fn main() -> std::process::ExitCode {
    match wreath_win_ui::tray::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            wreath_win_ui::tray::show_error(&error);
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("wreath-tray is available only on Windows");
}
