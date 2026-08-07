use std::fmt;

use wreath_core::config::HotkeyConfig;

const MOD_ALT_VALUE: u32 = 0x0001;
const MOD_CONTROL_VALUE: u32 = 0x0002;
const MOD_SHIFT_VALUE: u32 = 0x0004;
const MOD_WIN_VALUE: u32 = 0x0008;
const MOD_NOREPEAT_VALUE: u32 = 0x4000;

pub fn default_windows_hotkey() -> HotkeyConfig {
    HotkeyConfig {
        modifiers: vec!["CTRL".into(), "ALT".into()],
        key: "R".into(),
    }
}

/// Migrates the original Windows default, which used the OS-reserved Windows
/// key and therefore was not a dependable global shortcut.
pub fn migrate_legacy_windows_hotkey(hotkey: &mut HotkeyConfig) -> bool {
    if hotkey.modifiers == ["SUPER", "SHIFT"] && hotkey.key == "R" {
        *hotkey = default_windows_hotkey();
        true
    } else {
        false
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeHotkey {
    pub modifiers: u32,
    pub virtual_key: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyError {
    UnsupportedModifier(String),
    UnsupportedKey(String),
    Registration(String),
}

impl fmt::Display for HotkeyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedModifier(modifier) => {
                write!(formatter, "unsupported Windows hotkey modifier: {modifier}")
            }
            Self::UnsupportedKey(key) => write!(formatter, "unsupported Windows hotkey: {key}"),
            Self::Registration(message) => {
                write!(formatter, "cannot register Windows hotkey: {message}")
            }
        }
    }
}

impl std::error::Error for HotkeyError {}

impl TryFrom<&HotkeyConfig> for NativeHotkey {
    type Error = HotkeyError;

    fn try_from(hotkey: &HotkeyConfig) -> Result<Self, Self::Error> {
        let modifiers = hotkey.modifiers.iter().try_fold(
            MOD_NOREPEAT_VALUE,
            |mask, modifier| match modifier.as_str() {
                "ALT" => Ok(mask | MOD_ALT_VALUE),
                "CTRL" => Ok(mask | MOD_CONTROL_VALUE),
                "SHIFT" => Ok(mask | MOD_SHIFT_VALUE),
                "SUPER" => Ok(mask | MOD_WIN_VALUE),
                _ => Err(HotkeyError::UnsupportedModifier(modifier.clone())),
            },
        )?;
        let bytes = hotkey.key.as_bytes();
        let virtual_key = match bytes {
            [key] if key.is_ascii_alphanumeric() => u32::from(key.to_ascii_uppercase()),
            _ => return Err(HotkeyError::UnsupportedKey(hotkey.key.clone())),
        };
        Ok(Self {
            modifiers,
            virtual_key,
        })
    }
}

#[cfg(target_os = "windows")]
pub struct HotkeyRegistration {
    id: i32,
}

#[cfg(target_os = "windows")]
impl HotkeyRegistration {
    pub fn register(id: i32, hotkey: &HotkeyConfig) -> Result<Self, HotkeyError> {
        use windows::Win32::UI::Input::KeyboardAndMouse::{HOT_KEY_MODIFIERS, RegisterHotKey};

        let native = NativeHotkey::try_from(hotkey)?;
        unsafe {
            RegisterHotKey(
                None,
                id,
                HOT_KEY_MODIFIERS(native.modifiers),
                native.virtual_key,
            )
        }
        .map_err(|error| HotkeyError::Registration(error.to_string()))?;
        Ok(Self { id })
    }
}

#[cfg(target_os = "windows")]
impl Drop for HotkeyRegistration {
    fn drop(&mut self) {
        use windows::Win32::UI::Input::KeyboardAndMouse::UnregisterHotKey;

        let _ = unsafe { UnregisterHotKey(None, self.id) };
    }
}

#[cfg(target_os = "windows")]
pub struct HotkeyListener {
    thread_id: u32,
    rebind: std::sync::mpsc::SyncSender<HotkeyRebind>,
    thread: Option<std::thread::JoinHandle<()>>,
}

#[cfg(target_os = "windows")]
struct HotkeyRebind {
    hotkey: HotkeyConfig,
    reply: std::sync::mpsc::SyncSender<Result<(), HotkeyError>>,
}

#[cfg(target_os = "windows")]
const REBIND_MESSAGE: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 1;

#[cfg(target_os = "windows")]
impl HotkeyListener {
    pub fn spawn(
        id: i32,
        hotkey: &HotkeyConfig,
        mut on_hotkey: impl FnMut() + Send + 'static,
    ) -> Result<Self, HotkeyError> {
        use std::sync::mpsc;

        use windows::Win32::System::Threading::GetCurrentThreadId;
        use windows::Win32::UI::WindowsAndMessaging::{GetMessageW, MSG, WM_HOTKEY};

        let hotkey = hotkey.clone();
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let (rebind_sender, rebind_receiver) = mpsc::sync_channel::<HotkeyRebind>(1);
        let thread = std::thread::Builder::new()
            .name("wreath-hotkey".into())
            .spawn(move || {
                let thread_id = unsafe { GetCurrentThreadId() };
                let mut current_hotkey = hotkey;
                let mut registration = match HotkeyRegistration::register(id, &current_hotkey) {
                    Ok(registration) => registration,
                    Err(error) => {
                        let _ = ready_sender.send(Err(error));
                        return;
                    }
                };
                if ready_sender.send(Ok(thread_id)).is_err() {
                    return;
                }
                let mut message = MSG::default();
                loop {
                    let result = unsafe { GetMessageW(&mut message, None, 0, 0) };
                    if result.0 <= 0 {
                        break;
                    }
                    if message.message == WM_HOTKEY && message.wParam.0 == id as usize {
                        on_hotkey();
                    } else if message.message == REBIND_MESSAGE {
                        let Ok(request) = rebind_receiver.try_recv() else {
                            continue;
                        };
                        drop(registration);
                        match HotkeyRegistration::register(id, &request.hotkey) {
                            Ok(new_registration) => {
                                current_hotkey = request.hotkey;
                                registration = new_registration;
                                let _ = request.reply.send(Ok(()));
                            }
                            Err(error) => {
                                match HotkeyRegistration::register(id, &current_hotkey) {
                                    Ok(previous_registration) => {
                                        registration = previous_registration;
                                        let _ = request.reply.send(Err(error));
                                    }
                                    Err(restore_error) => {
                                        let _ = request.reply.send(Err(HotkeyError::Registration(
                                            format!(
                                                "new shortcut failed ({error}); previous shortcut could not be restored ({restore_error})"
                                            ),
                                        )));
                                        return;
                                    }
                                }
                            }
                        }
                    }
                }
                drop(registration);
            })
            .map_err(|error| HotkeyError::Registration(error.to_string()))?;
        let thread_id = ready_receiver
            .recv()
            .map_err(|error| HotkeyError::Registration(error.to_string()))??;
        Ok(Self {
            thread_id,
            rebind: rebind_sender,
            thread: Some(thread),
        })
    }

    /// Replaces the registered shortcut on the listener thread. If Windows
    /// rejects the new shortcut, the previous registration is restored.
    pub fn rebind(&self, hotkey: &HotkeyConfig) -> Result<(), HotkeyError> {
        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::PostThreadMessageW;

        NativeHotkey::try_from(hotkey)?;
        let (reply_sender, reply_receiver) = std::sync::mpsc::sync_channel(1);
        self.rebind
            .send(HotkeyRebind {
                hotkey: hotkey.clone(),
                reply: reply_sender,
            })
            .map_err(|error| HotkeyError::Registration(error.to_string()))?;
        unsafe { PostThreadMessageW(self.thread_id, REBIND_MESSAGE, WPARAM(0), LPARAM(0)) }
            .map_err(|error| HotkeyError::Registration(error.to_string()))?;
        reply_receiver
            .recv()
            .map_err(|error| HotkeyError::Registration(error.to_string()))?
    }
}

#[cfg(target_os = "windows")]
impl Drop for HotkeyListener {
    fn drop(&mut self) {
        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};

        let _ = unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) };
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_default_hotkey_without_key_repeat() {
        let hotkey = default_windows_hotkey();
        let native = NativeHotkey::try_from(&hotkey).unwrap();

        assert_eq!(native.virtual_key, u32::from(b'R'));
        assert_eq!(
            native.modifiers,
            MOD_CONTROL_VALUE | MOD_ALT_VALUE | MOD_NOREPEAT_VALUE
        );
    }

    #[test]
    fn migrates_the_reserved_legacy_windows_shortcut() {
        let mut hotkey = HotkeyConfig {
            modifiers: vec!["SUPER".into(), "SHIFT".into()],
            key: "R".into(),
        };

        assert!(migrate_legacy_windows_hotkey(&mut hotkey));
        assert_eq!(hotkey, default_windows_hotkey());
        assert!(!migrate_legacy_windows_hotkey(&mut hotkey));
    }

    #[test]
    fn rejects_key_names_that_are_not_windows_virtual_keys() {
        let hotkey = HotkeyConfig {
            modifiers: vec!["CTRL".into()],
            key: "ENTER".into(),
        };

        assert!(matches!(
            NativeHotkey::try_from(&hotkey),
            Err(HotkeyError::UnsupportedKey(key)) if key == "ENTER"
        ));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn registered_hotkey_dispatches_its_callback() {
        use std::sync::mpsc;
        use std::time::Duration;

        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_HOTKEY};

        let (sender, receiver) = mpsc::sync_channel(1);
        let listener = HotkeyListener::spawn(42, &default_windows_hotkey(), move || {
            let _ = sender.send(());
        })
        .unwrap();
        unsafe {
            PostThreadMessageW(listener.thread_id, WM_HOTKEY, WPARAM(42), LPARAM(0)).unwrap();
        }

        receiver.recv_timeout(Duration::from_secs(2)).unwrap();
    }
}
