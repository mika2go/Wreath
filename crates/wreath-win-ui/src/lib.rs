#[cfg(target_os = "windows")]
pub mod app;
#[cfg(target_os = "windows")]
pub mod autostart;
#[cfg(target_os = "windows")]
pub mod icon;
#[cfg(any(target_os = "windows", test))]
pub mod model;
#[cfg(target_os = "windows")]
pub mod player;
#[cfg(any(target_os = "windows", test))]
pub mod recovery;
#[cfg(target_os = "windows")]
pub mod renderer;
#[cfg(target_os = "windows")]
pub mod tray;
