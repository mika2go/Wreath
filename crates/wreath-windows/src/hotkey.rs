use std::fmt;

use wreath_core::config::HotkeyConfig;

const MOD_ALT_VALUE: u32 = 0x0001;
const MOD_CONTROL_VALUE: u32 = 0x0002;
const MOD_SHIFT_VALUE: u32 = 0x0004;
const MOD_WIN_VALUE: u32 = 0x0008;
const MOD_NOREPEAT_VALUE: u32 = 0x4000;

const VK_F1: u32 = 0x70;
const VK_F24: u32 = 0x87;

/// Turns the virtual-key value delivered by Windows into the stable value kept
/// in Wreath's config. OEM keys intentionally retain their virtual-key number:
/// their printed label depends on the user's active keyboard layout.
pub fn key_name_from_virtual_key(virtual_key: u32) -> Option<String> {
    let name = match virtual_key {
        0x08 => "BACKSPACE",
        0x09 => "TAB",
        0x0d => "ENTER",
        0x13 => "PAUSE",
        0x14 => "CAPSLOCK",
        0x20 => "SPACE",
        0x21 => "PAGEUP",
        0x22 => "PAGEDOWN",
        0x23 => "END",
        0x24 => "HOME",
        0x25 => "LEFT",
        0x26 => "UP",
        0x27 => "RIGHT",
        0x28 => "DOWN",
        0x2c => "PRINTSCREEN",
        0x2d => "INSERT",
        0x2e => "DELETE",
        0x60..=0x69 => return Some(format!("NUMPAD{}", virtual_key - 0x60)),
        0x6a => "MULTIPLY",
        0x6b => "ADD",
        0x6c => "SEPARATOR",
        0x6d => "SUBTRACT",
        0x6e => "DECIMAL",
        0x6f => "DIVIDE",
        VK_F1..=VK_F24 => return Some(format!("F{}", virtual_key - VK_F1 + 1)),
        0x90 => "NUMLOCK",
        0x91 => "SCROLLLOCK",
        0xa6 => "BROWSER_BACK",
        0xa7 => "BROWSER_FORWARD",
        0xa8 => "BROWSER_REFRESH",
        0xa9 => "BROWSER_STOP",
        0xaa => "BROWSER_SEARCH",
        0xab => "BROWSER_FAVORITES",
        0xac => "BROWSER_HOME",
        0xad => "VOLUME_MUTE",
        0xae => "VOLUME_DOWN",
        0xaf => "VOLUME_UP",
        0xb0 => "MEDIA_NEXT",
        0xb1 => "MEDIA_PREVIOUS",
        0xb2 => "MEDIA_STOP",
        0xb3 => "MEDIA_PLAY_PAUSE",
        0xb4 => "LAUNCH_MAIL",
        0xb5 => "LAUNCH_MEDIA",
        0xb6 => "LAUNCH_APP1",
        0xb7 => "LAUNCH_APP2",
        0xba..=0xc0 | 0xdb..=0xdf | 0xe2 => return Some(format!("VK_{virtual_key:02X}")),
        key if key <= 0xff && (key as u8).is_ascii_alphanumeric() => {
            return Some(char::from(key as u8).to_ascii_uppercase().to_string());
        }
        _ => return None,
    };
    Some(name.into())
}

pub fn virtual_key_from_name(key: &str) -> Option<u32> {
    let named = match key {
        "BACKSPACE" => 0x08,
        "TAB" => 0x09,
        "ENTER" => 0x0d,
        "PAUSE" => 0x13,
        "CAPSLOCK" => 0x14,
        "SPACE" => 0x20,
        "PAGEUP" => 0x21,
        "PAGEDOWN" => 0x22,
        "END" => 0x23,
        "HOME" => 0x24,
        "LEFT" => 0x25,
        "UP" => 0x26,
        "RIGHT" => 0x27,
        "DOWN" => 0x28,
        "PRINTSCREEN" => 0x2c,
        "INSERT" => 0x2d,
        "DELETE" => 0x2e,
        "MULTIPLY" => 0x6a,
        "ADD" => 0x6b,
        "SEPARATOR" => 0x6c,
        "SUBTRACT" => 0x6d,
        "DECIMAL" => 0x6e,
        "DIVIDE" => 0x6f,
        "NUMLOCK" => 0x90,
        "SCROLLLOCK" => 0x91,
        "BROWSER_BACK" => 0xa6,
        "BROWSER_FORWARD" => 0xa7,
        "BROWSER_REFRESH" => 0xa8,
        "BROWSER_STOP" => 0xa9,
        "BROWSER_SEARCH" => 0xaa,
        "BROWSER_FAVORITES" => 0xab,
        "BROWSER_HOME" => 0xac,
        "VOLUME_MUTE" => 0xad,
        "VOLUME_DOWN" => 0xae,
        "VOLUME_UP" => 0xaf,
        "MEDIA_NEXT" => 0xb0,
        "MEDIA_PREVIOUS" => 0xb1,
        "MEDIA_STOP" => 0xb2,
        "MEDIA_PLAY_PAUSE" => 0xb3,
        "LAUNCH_MAIL" => 0xb4,
        "LAUNCH_MEDIA" => 0xb5,
        "LAUNCH_APP1" => 0xb6,
        "LAUNCH_APP2" => 0xb7,
        _ => {
            if let Some(hex) = key.strip_prefix("VK_") {
                let virtual_key = u32::from_str_radix(hex, 16).ok()?;
                return (key_name_from_virtual_key(virtual_key).as_deref() == Some(key))
                    .then_some(virtual_key);
            }
            if let Some(number) = key.strip_prefix('F')
                && let Ok(number) = number.parse::<u32>()
                && (1..=24).contains(&number)
            {
                return Some(VK_F1 + number - 1);
            }
            if let Some(number) = key.strip_prefix("NUMPAD")
                && let Ok(number) = number.parse::<u32>()
                && number <= 9
            {
                return Some(0x60 + number);
            }
            let bytes = key.as_bytes();
            return match bytes {
                [key] if key.is_ascii_alphanumeric() => Some(u32::from(key.to_ascii_uppercase())),
                _ => None,
            };
        }
    };
    Some(named)
}

#[cfg(target_os = "windows")]
pub fn localized_hotkey_label(hotkey: &HotkeyConfig) -> String {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyNameTextW, MAPVK_VK_TO_VSC_EX, MapVirtualKeyW,
    };

    if !hotkey.is_bound() {
        return "Unbound".into();
    }
    let key = virtual_key_from_name(&hotkey.key)
        .and_then(|virtual_key| {
            let scan_code = unsafe { MapVirtualKeyW(virtual_key, MAPVK_VK_TO_VSC_EX) };
            if scan_code == 0 {
                return None;
            }
            let mut key_message = ((scan_code & 0xff) << 16) as i32;
            if scan_code & 0xff00 != 0 {
                key_message |= 1 << 24;
            }
            let mut buffer = [0_u16; 64];
            let length = unsafe { GetKeyNameTextW(key_message, &mut buffer) };
            (length > 0).then(|| String::from_utf16_lossy(&buffer[..length as usize]))
        })
        .unwrap_or_else(|| hotkey.key.clone());
    hotkey
        .modifiers
        .iter()
        .map(|modifier| match modifier.as_str() {
            "SUPER" => "Win",
            "CTRL" => "Ctrl",
            "ALT" => "Alt",
            "SHIFT" => "Shift",
            value => value,
        })
        .chain(std::iter::once(key.as_str()))
        .collect::<Vec<_>>()
        .join(" + ")
}

pub fn default_windows_hotkey() -> HotkeyConfig {
    HotkeyConfig {
        modifiers: vec!["CTRL".into()],
        key: "R".into(),
    }
}

/// Migrates the original Windows default, which used the OS-reserved Windows
/// key and therefore was not a dependable global shortcut.
pub fn migrate_legacy_windows_hotkey(hotkey: &mut HotkeyConfig) -> bool {
    if (hotkey.modifiers == ["SUPER", "SHIFT"] || hotkey.modifiers == ["CTRL", "ALT"])
        && hotkey.key == "R"
    {
        *hotkey = default_windows_hotkey();
        true
    } else {
        false
    }
}

/// Keeps two hotkey presses from starting two replay saves at once.
///
/// The guard expires on its own. It used to be a plain flag that only the save
/// worker cleared, so a worker that never came back left every later press
/// logged as "a save is already running" - the shortcut was dead for good
/// while the daemon kept running and looked healthy.
#[derive(Debug)]
pub struct SaveGuard {
    started: std::sync::Mutex<Option<std::time::Instant>>,
    stale_after: std::time::Duration,
}

impl SaveGuard {
    pub fn new(stale_after: std::time::Duration) -> Self {
        Self {
            started: std::sync::Mutex::new(None),
            stale_after,
        }
    }

    /// Grants the guard when no save is running, or when the running one has
    /// outlived every timeout the save path can hit.
    pub fn acquire(&self, now: std::time::Instant) -> bool {
        let mut started = self.locked();
        match *started {
            Some(at) if now.duration_since(at) < self.stale_after => false,
            _ => {
                *started = Some(now);
                true
            }
        }
    }

    pub fn release(&self) {
        *self.locked() = None;
    }

    fn locked(&self) -> std::sync::MutexGuard<'_, Option<std::time::Instant>> {
        self.started
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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
    UnsafeCombination,
    Registration(String),
}

impl fmt::Display for HotkeyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedModifier(modifier) => {
                write!(formatter, "unsupported Windows hotkey modifier: {modifier}")
            }
            Self::UnsupportedKey(key) => write!(formatter, "unsupported Windows hotkey: {key}"),
            Self::UnsafeCombination => formatter.write_str(
                "use Ctrl or Shift with one other key; F1-F24 and Print Screen also work alone",
            ),
            Self::Registration(message) => {
                write!(formatter, "cannot register Windows hotkey: {message}")
            }
        }
    }
}

impl std::error::Error for HotkeyError {}

/// Refuses ordinary typing and multi-modifier chords. A newly selected shortcut
/// is either Ctrl/Shift plus exactly one key, or a standalone function/Print
/// Screen key. Existing config files remain loadable; this rule is applied only
/// when choosing a new key.
pub fn validate_hotkey_choice(hotkey: &HotkeyConfig) -> Result<(), HotkeyError> {
    if !hotkey.is_bound() {
        return Ok(());
    }
    NativeHotkey::try_from(hotkey)?;
    let standalone_key = hotkey.key == "PRINTSCREEN"
        || hotkey
            .key
            .strip_prefix('F')
            .and_then(|number| number.parse::<u8>().ok())
            .is_some_and(|number| (1..=24).contains(&number));
    let supported_pair = matches!(hotkey.modifiers.as_slice(), [modifier] if matches!(modifier.as_str(), "CTRL" | "SHIFT"));
    if (hotkey.modifiers.is_empty() && standalone_key) || supported_pair {
        Ok(())
    } else {
        Err(HotkeyError::UnsafeCombination)
    }
}

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
        let virtual_key = virtual_key_from_name(&hotkey.key)
            .ok_or_else(|| HotkeyError::UnsupportedKey(hotkey.key.clone()))?;
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
    rebind: std::sync::mpsc::Sender<HotkeyRebind>,
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
const HOTKEY_WATCHDOG_INTERVAL_MS: u32 = 5_000;

#[cfg(target_os = "windows")]
impl HotkeyListener {
    pub fn spawn(
        id: i32,
        hotkey: &HotkeyConfig,
        mut on_hotkey: impl FnMut() + Send + 'static,
    ) -> Result<Self, HotkeyError> {
        use std::sync::mpsc;

        use windows::Win32::System::Threading::GetCurrentThreadId;
        use windows::Win32::UI::WindowsAndMessaging::{
            GetMessageW, KillTimer, MSG, SetTimer, WM_HOTKEY, WM_TIMER,
        };

        let hotkey = hotkey.clone();
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let (rebind_sender, rebind_receiver) = mpsc::channel::<HotkeyRebind>();
        let thread = std::thread::Builder::new()
            .name("wreath-hotkey".into())
            .spawn(move || {
                let thread_id = unsafe { GetCurrentThreadId() };
                let mut current_hotkey = hotkey;
                let mut registration = match register_optional(id, &current_hotkey) {
                    Ok(registration) => registration,
                    // A shortcut Windows refuses right now - usually because
                    // another application holds it - must not take the capture
                    // service down with it. Start without it and let the
                    // watchdog claim it as soon as it is free again. A
                    // shortcut that can never be translated stays fatal: the
                    // configuration itself is broken and no retry fixes it.
                    Err(error @ HotkeyError::Registration(_)) => {
                        wreath_core::diagnostic!(
                            "Wreath hotkey: {current_hotkey} is unavailable, watchdog will retry every {} seconds: {error}",
                            HOTKEY_WATCHDOG_INTERVAL_MS / 1_000
                        );
                        None
                    }
                    Err(error) => {
                        let _ = ready_sender.send(Err(error));
                        return;
                    }
                };
                if ready_sender.send(Ok(thread_id)).is_err() {
                    return;
                }
                let watchdog_timer = unsafe {
                    SetTimer(
                        None,
                        0,
                        HOTKEY_WATCHDOG_INTERVAL_MS,
                        None,
                    )
                };
                if watchdog_timer == 0 {
                    wreath_core::diagnostic!(
                        "Wreath hotkey: cannot start registration watchdog: {}",
                        std::io::Error::last_os_error()
                    );
                }
                let mut message = MSG::default();
                loop {
                    let result = unsafe { GetMessageW(&mut message, None, 0, 0) };
                    if result.0 == 0 {
                        break;
                    }
                    if result.0 == -1 {
                        wreath_core::diagnostic!(
                            "Wreath hotkey: Windows message loop failed, retrying: {}",
                            std::io::Error::last_os_error()
                        );
                        std::thread::sleep(std::time::Duration::from_millis(100));
                        continue;
                    }
                    if message.message == WM_HOTKEY && message.wParam.0 == id as usize {
                        on_hotkey();
                    } else if message.message == WM_TIMER
                        && watchdog_timer != 0
                        && message.wParam.0 == watchdog_timer
                    {
                        refresh_registration(id, &current_hotkey, &mut registration);
                    } else if message.message == REBIND_MESSAGE {
                        while let Ok(request) = rebind_receiver.try_recv() {
                            drop(registration.take());
                            match register_optional(id, &request.hotkey) {
                                Ok(new_registration) => {
                                    if request.reply.send(Ok(())).is_ok() {
                                        current_hotkey = request.hotkey;
                                        registration = new_registration;
                                    } else {
                                        drop(new_registration);
                                        match register_optional(id, &current_hotkey) {
                                            Ok(previous_registration) => {
                                                registration = previous_registration;
                                            }
                                            Err(error) => wreath_core::diagnostic!(
                                                "Wreath hotkey: previous shortcut could not be restored after a cancelled update; watchdog will retry: {error}"
                                            ),
                                        }
                                    }
                                }
                                Err(error) => {
                                    match register_optional(id, &current_hotkey) {
                                        Ok(previous_registration) => {
                                            registration = previous_registration;
                                            let _ = request.reply.send(Err(error));
                                        }
                                        Err(restore_error) => {
                                            let _ = request.reply.send(Err(
                                                HotkeyError::Registration(format!(
                                                    "new shortcut failed ({error}); previous shortcut could not be restored ({restore_error})"
                                                )),
                                            ));
                                            wreath_core::diagnostic!(
                                                "Wreath hotkey: shortcut restoration failed; watchdog will retry: {restore_error}"
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                if watchdog_timer != 0 {
                    let _ = unsafe { KillTimer(None, watchdog_timer) };
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

        if hotkey.is_bound() {
            NativeHotkey::try_from(hotkey)?;
        }
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
            .recv_timeout(std::time::Duration::from_secs(2))
            .map_err(|error| HotkeyError::Registration(error.to_string()))?
    }
}

/// Reclaims the shortcut after a failed registration, and only then. Windows
/// hands a hotkey to exactly one thread and holds it there until that thread
/// releases it, so a registration we still own cannot go stale - while giving
/// it up first, as this watchdog used to do on every tick, both dropped the
/// presses that landed in the gap and let any other application claim the
/// combination for good.
#[cfg(target_os = "windows")]
fn refresh_registration(
    id: i32,
    hotkey: &HotkeyConfig,
    registration: &mut Option<HotkeyRegistration>,
) {
    if registration.is_some() || !hotkey.is_bound() {
        return;
    }
    match register_optional(id, hotkey) {
        Ok(new_registration) => {
            *registration = new_registration;
            wreath_core::diagnostic!("Wreath hotkey: shortcut {hotkey} registered again");
        }
        Err(error) => wreath_core::diagnostic!(
            "Wreath hotkey: automatic registration repair failed; retrying in {} seconds: {error}",
            HOTKEY_WATCHDOG_INTERVAL_MS / 1_000
        ),
    }
}

#[cfg(target_os = "windows")]
fn register_optional(
    id: i32,
    hotkey: &HotkeyConfig,
) -> Result<Option<HotkeyRegistration>, HotkeyError> {
    hotkey
        .is_bound()
        .then(|| HotkeyRegistration::register(id, hotkey))
        .transpose()
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
        assert_eq!(native.modifiers, MOD_CONTROL_VALUE | MOD_NOREPEAT_VALUE);
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
            key: "NOT_A_KEY".into(),
        };

        assert!(matches!(
            NativeHotkey::try_from(&hotkey),
            Err(HotkeyError::UnsupportedKey(key)) if key == "NOT_A_KEY"
        ));
    }

    #[test]
    fn translates_keys_from_different_windows_keyboard_sections() {
        for (key, virtual_key) in [
            ("ENTER", 0x0d),
            ("LEFT", 0x25),
            ("NUMPAD7", 0x67),
            ("MEDIA_PLAY_PAUSE", 0xb3),
            ("VK_BA", 0xba),
            ("VK_E2", 0xe2),
        ] {
            assert_eq!(virtual_key_from_name(key), Some(virtual_key));
            assert_eq!(key_name_from_virtual_key(virtual_key).as_deref(), Some(key));
        }
    }

    #[test]
    fn excludes_modifiers_escape_mouse_and_reserved_virtual_keys() {
        for virtual_key in [0x01, 0x10, 0x11, 0x12, 0x1b, 0x5b, 0x5c, 0xe7] {
            assert_eq!(key_name_from_virtual_key(virtual_key), None);
        }
    }

    #[test]
    fn translates_function_keys_with_or_without_modifiers() {
        for (key, virtual_key) in [("F1", 0x70), ("F12", 0x7b), ("F24", 0x87)] {
            let hotkey = HotkeyConfig {
                modifiers: Vec::new(),
                key: key.into(),
            };
            assert_eq!(
                NativeHotkey::try_from(&hotkey).unwrap().virtual_key,
                virtual_key
            );
        }
    }

    #[test]
    fn new_shortcuts_are_exactly_one_modifier_plus_one_key() {
        for hotkey in [
            HotkeyConfig {
                modifiers: Vec::new(),
                key: "R".into(),
            },
            HotkeyConfig {
                modifiers: vec!["CTRL".into(), "SHIFT".into()],
                key: "R".into(),
            },
            HotkeyConfig {
                modifiers: vec!["ALT".into()],
                key: "R".into(),
            },
            HotkeyConfig {
                modifiers: vec!["CTRL".into(), "SHIFT".into()],
                key: "F8".into(),
            },
        ] {
            assert_eq!(
                validate_hotkey_choice(&hotkey),
                Err(HotkeyError::UnsafeCombination)
            );
        }

        for hotkey in [
            HotkeyConfig {
                modifiers: Vec::new(),
                key: "F8".into(),
            },
            HotkeyConfig {
                modifiers: vec!["CTRL".into()],
                key: "C".into(),
            },
            HotkeyConfig {
                modifiers: vec!["SHIFT".into()],
                key: "9".into(),
            },
            default_windows_hotkey(),
        ] {
            assert_eq!(validate_hotkey_choice(&hotkey), Ok(()));
        }
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

    #[cfg(target_os = "windows")]
    #[test]
    fn failed_rebind_restores_the_previous_registered_hotkey() {
        use std::sync::mpsc;
        use std::time::Duration;

        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_HOTKEY};

        let original = HotkeyConfig {
            modifiers: vec!["CTRL".into(), "ALT".into(), "SHIFT".into()],
            key: "F23".into(),
        };
        let occupied = HotkeyConfig {
            modifiers: vec!["CTRL".into(), "ALT".into(), "SHIFT".into()],
            key: "F22".into(),
        };
        let blocker = HotkeyRegistration::register(98, &occupied).unwrap();
        let (sender, receiver) = mpsc::sync_channel(1);
        let listener = HotkeyListener::spawn(43, &original, move || {
            let _ = sender.send(());
        })
        .unwrap();

        assert!(listener.rebind(&occupied).is_err());
        unsafe {
            PostThreadMessageW(listener.thread_id, WM_HOTKEY, WPARAM(43), LPARAM(0)).unwrap();
        }
        receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        drop(listener);
        drop(blocker);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn watchdog_reclaims_a_lost_shortcut_without_surrendering_a_working_one() {
        let hotkey = HotkeyConfig {
            modifiers: vec!["CTRL".into(), "ALT".into(), "SHIFT".into()],
            key: "F21".into(),
        };
        let mut registration = None;

        refresh_registration(44, &hotkey, &mut registration);
        assert!(registration.is_some());
        assert!(HotkeyRegistration::register(99, &hotkey).is_err());

        // A tick over a healthy registration has to leave it registered, with
        // no window in between for another application to take it.
        refresh_registration(44, &hotkey, &mut registration);
        assert!(registration.is_some());
        assert!(HotkeyRegistration::register(99, &hotkey).is_err());
    }

    #[test]
    fn one_save_runs_at_a_time() {
        use std::time::{Duration, Instant};

        let guard = SaveGuard::new(Duration::from_secs(60));
        let press = Instant::now();

        assert!(guard.acquire(press));
        assert!(!guard.acquire(press + Duration::from_secs(1)));
        guard.release();
        assert!(guard.acquire(press + Duration::from_secs(2)));
    }

    #[test]
    fn a_save_that_never_finishes_stops_blocking_the_shortcut() {
        use std::time::{Duration, Instant};

        let guard = SaveGuard::new(Duration::from_secs(60));
        let press = Instant::now();
        assert!(guard.acquire(press));

        assert!(!guard.acquire(press + Duration::from_secs(59)));
        assert!(guard.acquire(press + Duration::from_secs(60)));
    }
}
