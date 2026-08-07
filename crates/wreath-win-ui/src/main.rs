#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(target_os = "windows")]
mod autostart;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "windows")]
fn main() -> std::process::ExitCode {
    match windows::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            windows::show_error(&error);
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("wreath-win-ui is available only on Windows");
}
