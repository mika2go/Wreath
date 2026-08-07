#[cfg(target_os = "windows")]
pub mod app;
#[cfg(target_os = "windows")]
pub mod autostart;
#[cfg(any(target_os = "windows", test))]
pub mod recovery;
#[cfg(target_os = "windows")]
pub mod tray;
