#[cfg(any(target_os = "windows", test))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessFacts {
    pub executable: String,
    pub path: String,
    pub modules: Vec<String>,
}

#[cfg(any(target_os = "windows", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowFacts {
    pub width: u32,
    pub height: u32,
    pub monitor_width: u32,
    pub monitor_height: u32,
    pub borderless: bool,
}

#[cfg(any(target_os = "windows", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameConfidence {
    None,
    Likely,
    Certain,
}

#[cfg(any(target_os = "windows", test))]
const DENIED_EXECUTABLES: &[&str] = &[
    "applicationframehost.exe",
    "battle.net.exe",
    "brave.exe",
    "chrome.exe",
    "code.exe",
    "devenv.exe",
    "discord.exe",
    "dwm.exe",
    "epicgameslauncher.exe",
    "explorer.exe",
    "firefox.exe",
    "gog galaxy.exe",
    "idea64.exe",
    "logioverlay.exe",
    "lockapp.exe",
    "mpv.exe",
    "msedge.exe",
    "mstsc.exe",
    "obs64.exe",
    "opera.exe",
    "outlook.exe",
    "photoshop.exe",
    "powerpnt.exe",
    "riotclientservices.exe",
    "riotclientux.exe",
    "searchhost.exe",
    "shellexperiencehost.exe",
    "slack.exe",
    "spotify.exe",
    "startmenuexperiencehost.exe",
    "steam.exe",
    "steamwebhelper.exe",
    "taskmgr.exe",
    "teams.exe",
    "textinputhost.exe",
    "vivaldi.exe",
    "vlc.exe",
    "wreath-tray.exe",
    "wreath.exe",
    "wreathd.exe",
    "zen.exe",
    "zoom.exe",
];

#[cfg(any(target_os = "windows", test))]
const GAME_EXECUTABLES: &[&str] = &[
    "cs2.exe",
    "csgo.exe",
    "dota2.exe",
    "factorio.exe",
    "gta5.exe",
    "gtav.exe",
    "javaw.exe",
    "league of legends.exe",
    "leagueclient.exe",
    "minecraft.exe",
    "minecraftlauncher.exe",
    "osu!.exe",
    "overwatch.exe",
    "r5apex.exe",
    "robloxplayerbeta.exe",
    "rocketleague.exe",
    "starcraft ii.exe",
    "terraria.exe",
    "valorant-win64-shipping.exe",
    "valorant.exe",
    "vrchat.exe",
    "wow.exe",
    "wowclassic.exe",
];

#[cfg(any(target_os = "windows", test))]
const GAME_EXECUTABLE_SUFFIXES: &[&str] = &[
    "-win64-shipping.exe",
    "-win32-shipping.exe",
    "-wingdk-shipping.exe",
    "-winuwp64-shipping.exe",
];

#[cfg(any(target_os = "windows", test))]
const GAME_PATH_MARKERS: &[&str] = &[
    "\\battle.net\\",
    "\\ea games\\",
    "\\electronic arts\\",
    "\\epic games\\",
    "\\games\\",
    "\\gog galaxy\\games\\",
    "\\origin games\\",
    "\\riot games\\",
    "\\roblox\\",
    "\\steamapps\\common\\",
    "\\ubisoft\\",
    "\\ubisoft game launcher\\",
    "\\xboxgames\\",
];

#[cfg(any(target_os = "windows", test))]
const GAME_MODULES: &[&str] = &[
    "beclient.dll",
    "beclient_x64.dll",
    "easyanticheat.dll",
    "easyanticheat_x64.dll",
    "eossdk-win64-shipping.dll",
    "galaxy64.dll",
    "gameoverlayrenderer.dll",
    "gameoverlayrenderer64.dll",
    "steam_api.dll",
    "steam_api64.dll",
    "unityplayer.dll",
    "vulkan-1.dll",
];

#[cfg(any(target_os = "windows", test))]
const GRAPHICS_MODULES: &[&str] = &[
    "d3d10.dll",
    "d3d11.dll",
    "d3d12.dll",
    "d3d9.dll",
    "opengl32.dll",
];

#[cfg(any(target_os = "windows", test))]
const INPUT_MODULES: &[&str] = &[
    "dinput8.dll",
    "xinput1_3.dll",
    "xinput1_4.dll",
    "xinput9_1_0.dll",
];

#[cfg(any(target_os = "windows", test))]
pub fn classify(
    process: &ProcessFacts,
    window: &WindowFacts,
    configured: &[String],
    windows_game_paths: &[String],
) -> GameConfidence {
    let executable = process.executable.to_ascii_lowercase();
    let path = process.path.to_ascii_lowercase();
    if configured
        .iter()
        .any(|entry| matches_configured_entry(entry, &executable, &path))
    {
        return GameConfidence::Certain;
    }
    if DENIED_EXECUTABLES.contains(&executable.as_str()) {
        return GameConfidence::None;
    }
    let named = GAME_EXECUTABLES.contains(&executable.as_str())
        || GAME_EXECUTABLE_SUFFIXES
            .iter()
            .any(|suffix| executable.ends_with(suffix));
    let installed_as_a_game = GAME_PATH_MARKERS.iter().any(|marker| path.contains(marker))
        || windows_game_paths
            .iter()
            .any(|known| known.eq_ignore_ascii_case(&path));
    let game_runtime = has_module(&process.modules, GAME_MODULES);
    if named || installed_as_a_game || game_runtime {
        return GameConfidence::Certain;
    }
    let weak = usize::from(has_module(&process.modules, GRAPHICS_MODULES))
        + usize::from(has_module(&process.modules, INPUT_MODULES))
        + usize::from(window.borderless && covers_monitor(window));
    if weak >= 2 {
        GameConfidence::Likely
    } else {
        GameConfidence::None
    }
}

#[cfg(any(target_os = "windows", test))]
fn matches_configured_entry(entry: &str, executable: &str, path: &str) -> bool {
    let entry = entry.trim().to_ascii_lowercase();
    if entry.is_empty() {
        return false;
    }
    entry == executable || entry == path
}

#[cfg(any(target_os = "windows", test))]
fn has_module(modules: &[String], wanted: &[&str]) -> bool {
    modules
        .iter()
        .any(|module| wanted.contains(&module.to_ascii_lowercase().as_str()))
}

#[cfg(any(target_os = "windows", test))]
pub fn covers_monitor(window: &WindowFacts) -> bool {
    if window.monitor_width == 0 || window.monitor_height == 0 {
        return false;
    }
    let wide = u64::from(window.width) * 100 >= u64::from(window.monitor_width) * 95;
    let tall = u64::from(window.height) * 100 >= u64::from(window.monitor_height) * 95;
    wide && tall
}

#[cfg(any(target_os = "windows", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowUsability {
    Capturable,
    TooSmall,
    CoversMonitor,
}

#[cfg(any(target_os = "windows", test))]
const MINIMUM_WINDOW_EDGE: u32 = 320;

#[cfg(any(target_os = "windows", test))]
pub fn window_usability(window: &WindowFacts, confidence: GameConfidence) -> WindowUsability {
    if covers_monitor(window) {
        return WindowUsability::CoversMonitor;
    }
    if confidence != GameConfidence::Certain
        || window.width < MINIMUM_WINDOW_EDGE
        || window.height < MINIMUM_WINDOW_EDGE
    {
        return WindowUsability::TooSmall;
    }
    WindowUsability::Capturable
}

#[cfg(target_os = "windows")]
pub struct GameWindow {
    pub window: windows::Win32::Foundation::HWND,
    pub process_id: u32,
    pub executable: String,
    pub title: String,
    pub confidence: GameConfidence,
    pub facts: WindowFacts,
}

#[cfg(target_os = "windows")]
const REINSPECTION_INTERVAL: std::time::Duration = std::time::Duration::from_secs(10);

#[cfg(target_os = "windows")]
pub struct GameWatch {
    configured: Vec<String>,
    windows_game_paths: Vec<String>,
    tracked: Option<(windows::Win32::Foundation::HWND, u32, String)>,
    rejected: Option<(windows::Win32::Foundation::HWND, std::time::Instant)>,
}

#[cfg(target_os = "windows")]
impl GameWatch {
    pub fn new(configured: &[String]) -> Self {
        Self {
            configured: configured.to_vec(),
            windows_game_paths: windows_game_paths(),
            tracked: None,
            rejected: None,
        }
    }

    pub fn look(&mut self) -> Option<GameWindow> {
        if let Some(game) = self.tracked_game() {
            return Some(game);
        }
        self.tracked = None;
        let foreground = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
        let now = std::time::Instant::now();
        if self.rejected.is_some_and(|(window, seen)| {
            window == foreground && now < seen + REINSPECTION_INTERVAL
        }) {
            return None;
        }
        let Some(game) = self.inspect(foreground) else {
            self.rejected = Some((foreground, now));
            return None;
        };
        self.rejected = None;
        self.tracked = Some((game.window, game.process_id, game.executable.clone()));
        wreath_core::diagnostic!(
            "Wreath capture: {} looks like a game ({:?}), its window is {}x{}",
            game.executable,
            game.confidence,
            game.facts.width,
            game.facts.height
        );
        Some(game)
    }

    fn tracked_game(&self) -> Option<GameWindow> {
        use windows::Win32::UI::WindowsAndMessaging::IsWindow;

        let (window, process_id, executable) = self.tracked.as_ref()?;
        if !unsafe { IsWindow(Some(*window)) }.as_bool() {
            return None;
        }
        if window_process_id(*window) != Some(*process_id) {
            return None;
        }
        let facts = window_facts(*window)?;
        Some(GameWindow {
            window: *window,
            process_id: *process_id,
            executable: executable.clone(),
            title: window_title(*window),
            confidence: GameConfidence::Certain,
            facts,
        })
    }

    fn inspect(&self, window: windows::Win32::Foundation::HWND) -> Option<GameWindow> {
        if window.is_invalid() {
            return None;
        }
        let process_id = window_process_id(window)?;
        if process_id == unsafe { windows::Win32::System::Threading::GetCurrentProcessId() } {
            return None;
        }
        let facts = window_facts(window)?;
        let process = process_facts(process_id)?;
        let confidence = classify(&process, &facts, &self.configured, &self.windows_game_paths);
        if confidence == GameConfidence::None {
            return None;
        }
        Some(GameWindow {
            window,
            process_id,
            executable: process.executable,
            title: window_title(window),
            confidence,
            facts,
        })
    }
}

#[cfg(target_os = "windows")]
fn window_process_id(window: windows::Win32::Foundation::HWND) -> Option<u32> {
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

    let mut process_id = 0_u32;
    let thread = unsafe { GetWindowThreadProcessId(window, Some(&mut process_id)) };
    (thread != 0 && process_id != 0).then_some(process_id)
}

#[cfg(target_os = "windows")]
fn window_facts(window: windows::Win32::Foundation::HWND) -> Option<WindowFacts> {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_STYLE, GetWindowLongPtrW, GetWindowRect, IsIconic, IsWindowVisible, WS_CAPTION,
        WS_THICKFRAME,
    };

    if !unsafe { IsWindowVisible(window) }.as_bool() || unsafe { IsIconic(window) }.as_bool() {
        return None;
    }
    let mut rectangle = RECT::default();
    unsafe { GetWindowRect(window, &mut rectangle) }.ok()?;
    let width = u32::try_from(rectangle.right.saturating_sub(rectangle.left)).ok()?;
    let height = u32::try_from(rectangle.bottom.saturating_sub(rectangle.top)).ok()?;
    if width == 0 || height == 0 {
        return None;
    }
    let monitor = unsafe { MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: u32::try_from(size_of::<MONITORINFO>()).unwrap_or(0),
        ..Default::default()
    };
    let (monitor_width, monitor_height) =
        if unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
            (
                u32::try_from(info.rcMonitor.right.saturating_sub(info.rcMonitor.left))
                    .unwrap_or_default(),
                u32::try_from(info.rcMonitor.bottom.saturating_sub(info.rcMonitor.top))
                    .unwrap_or_default(),
            )
        } else {
            (0, 0)
        };
    let style = unsafe { GetWindowLongPtrW(window, GWL_STYLE) } as u32;
    Some(WindowFacts {
        width,
        height,
        monitor_width,
        monitor_height,
        borderless: style & (WS_CAPTION.0 | WS_THICKFRAME.0) == 0,
    })
}

#[cfg(target_os = "windows")]
pub fn window_monitor(
    window: windows::Win32::Foundation::HWND,
) -> windows::Win32::Graphics::Gdi::HMONITOR {
    use windows::Win32::Graphics::Gdi::{MONITOR_DEFAULTTOPRIMARY, MonitorFromWindow};

    unsafe { MonitorFromWindow(window, MONITOR_DEFAULTTOPRIMARY) }
}

#[cfg(target_os = "windows")]
fn window_title(window: windows::Win32::Foundation::HWND) -> String {
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowTextLengthW, GetWindowTextW};

    let length = unsafe { GetWindowTextLengthW(window) };
    if length <= 0 {
        return String::new();
    }
    let mut text = vec![0_u16; length as usize + 1];
    let written = unsafe { GetWindowTextW(window, &mut text) };
    if written <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&text[..written as usize])
}

#[cfg(target_os = "windows")]
fn process_facts(process_id: u32) -> Option<ProcessFacts> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
    };

    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ,
            false,
            process_id,
        )
    }
    .or_else(|_| unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) })
    .ok()?;
    let path = process_path(handle).unwrap_or_default();
    let modules = process_modules(handle);
    let _ = unsafe { CloseHandle(handle) };
    let executable = path
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or_default()
        .to_owned();
    if executable.is_empty() {
        return None;
    }
    Some(ProcessFacts {
        executable,
        path,
        modules,
    })
}

#[cfg(target_os = "windows")]
fn process_path(handle: windows::Win32::Foundation::HANDLE) -> Option<String> {
    use windows::Win32::System::Threading::{PROCESS_NAME_WIN32, QueryFullProcessImageNameW};
    use windows::core::PWSTR;

    let mut buffer = vec![0_u16; 1024];
    let mut length = u32::try_from(buffer.len()).ok()?;
    unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        )
    }
    .ok()?;
    Some(String::from_utf16_lossy(&buffer[..length as usize]))
}

#[cfg(target_os = "windows")]
fn process_modules(handle: windows::Win32::Foundation::HANDLE) -> Vec<String> {
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::System::ProcessStatus::{
        EnumProcessModulesEx, GetModuleBaseNameW, LIST_MODULES_ALL,
    };

    let mut modules = vec![HMODULE::default(); 512];
    let mut needed = 0_u32;
    let capacity = u32::try_from(std::mem::size_of_val(modules.as_slice())).unwrap_or(0);
    if unsafe {
        EnumProcessModulesEx(
            handle,
            modules.as_mut_ptr(),
            capacity,
            &mut needed,
            LIST_MODULES_ALL,
        )
    }
    .is_err()
    {
        return Vec::new();
    }
    let count = (needed as usize / size_of::<HMODULE>()).min(modules.len());
    let mut names = Vec::with_capacity(count);
    let mut name = [0_u16; 260];
    for module in &modules[..count] {
        let written = unsafe { GetModuleBaseNameW(handle, Some(*module), &mut name) };
        if written > 0 {
            names.push(String::from_utf16_lossy(&name[..written as usize]).to_ascii_lowercase());
        }
    }
    names
}

#[cfg(target_os = "windows")]
fn windows_game_paths() -> Vec<String> {
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_READ, RRF_RT_REG_SZ, RegCloseKey, RegEnumKeyExW, RegGetValueW,
        RegOpenKeyExW,
    };
    use windows::core::{PCWSTR, PWSTR, w};

    let mut root = HKEY::default();
    if unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!("System\\GameConfigStore\\Children"),
            None,
            KEY_READ,
            &mut root,
        )
    } != ERROR_SUCCESS
    {
        return Vec::new();
    }
    let mut paths = Vec::new();
    let mut index = 0_u32;
    loop {
        let mut name = [0_u16; 128];
        let mut length = u32::try_from(name.len()).unwrap_or(0);
        if unsafe {
            RegEnumKeyExW(
                root,
                index,
                Some(PWSTR(name.as_mut_ptr())),
                &mut length,
                None,
                None,
                None,
                None,
            )
        } != ERROR_SUCCESS
        {
            break;
        }
        index = index.saturating_add(1);
        let child = String::from_utf16_lossy(&name[..length as usize]);
        let subkey = format!("System\\GameConfigStore\\Children\\{child}\0")
            .encode_utf16()
            .collect::<Vec<_>>();
        let mut value = [0_u16; 520];
        let mut value_bytes = u32::try_from(std::mem::size_of_val(&value)).unwrap_or(0);
        if unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                w!("MatchedExeFullPath"),
                RRF_RT_REG_SZ,
                None,
                Some(value.as_mut_ptr().cast()),
                Some(&mut value_bytes),
            )
        } == ERROR_SUCCESS
        {
            let units = (value_bytes as usize / size_of::<u16>()).min(value.len());
            let end = value[..units]
                .iter()
                .position(|unit| *unit == 0)
                .unwrap_or(units);
            if end > 0 {
                paths.push(String::from_utf16_lossy(&value[..end]).to_ascii_lowercase());
            }
        }
    }
    let _ = unsafe { RegCloseKey(root) };
    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(executable: &str, path: &str, modules: &[&str]) -> ProcessFacts {
        ProcessFacts {
            executable: executable.into(),
            path: path.into(),
            modules: modules.iter().map(|module| (*module).into()).collect(),
        }
    }

    fn windowed() -> WindowFacts {
        WindowFacts {
            width: 1280,
            height: 720,
            monitor_width: 2560,
            monitor_height: 1440,
            borderless: false,
        }
    }

    fn fullscreen() -> WindowFacts {
        WindowFacts {
            width: 2560,
            height: 1440,
            monitor_width: 2560,
            monitor_height: 1440,
            borderless: true,
        }
    }

    #[test]
    fn unreal_shipping_builds_are_games_wherever_they_live() {
        let rivals = process(
            "Marvel-Win64-Shipping.exe",
            "D:\\MarvelRivals\\Marvel\\Binaries\\Win64\\Marvel-Win64-Shipping.exe",
            &[],
        );
        assert_eq!(
            classify(&rivals, &fullscreen(), &[], &[]),
            GameConfidence::Certain
        );
    }

    #[test]
    fn named_games_are_recognized_without_any_module_evidence() {
        for (executable, path) in [
            (
                "osu!.exe",
                "C:\\Users\\mika\\AppData\\Local\\osu!\\osu!.exe",
            ),
            (
                "RobloxPlayerBeta.exe",
                "C:\\Users\\mika\\AppData\\Local\\Roblox\\Versions\\v1\\RobloxPlayerBeta.exe",
            ),
        ] {
            assert_eq!(
                classify(&process(executable, path, &[]), &windowed(), &[], &[]),
                GameConfidence::Certain,
                "{executable}"
            );
        }
    }

    #[test]
    fn an_install_root_alone_is_enough() {
        let unknown = process(
            "Nightreign.exe",
            "E:\\SteamLibrary\\steamapps\\common\\Nightreign\\Nightreign.exe",
            &[],
        );
        assert_eq!(
            classify(&unknown, &windowed(), &[], &[]),
            GameConfidence::Certain
        );
    }

    #[test]
    fn a_game_runtime_module_is_enough_for_an_unknown_executable() {
        let unity = process("Game.exe", "D:\\itch\\Game.exe", &["UnityPlayer.dll"]);
        assert_eq!(
            classify(&unity, &windowed(), &[], &[]),
            GameConfidence::Certain
        );
    }

    #[test]
    fn windows_own_game_list_is_trusted() {
        let unknown = process("Weird.exe", "D:\\Weird\\Weird.exe", &[]);
        let known = vec!["d:\\weird\\weird.exe".to_owned()];
        assert_eq!(
            classify(&unknown, &windowed(), &[], &known),
            GameConfidence::Certain
        );
    }

    #[test]
    fn browsers_stay_out_even_when_they_render_with_direct3d() {
        let chrome = process(
            "chrome.exe",
            "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
            &["d3d11.dll", "dxgi.dll"],
        );
        assert_eq!(
            classify(&chrome, &fullscreen(), &[], &[]),
            GameConfidence::None
        );
    }

    #[test]
    fn a_configured_entry_outranks_the_denylist() {
        let chrome = process(
            "chrome.exe",
            "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
            &[],
        );
        let configured = vec!["chrome.exe".to_owned()];
        assert_eq!(
            classify(&chrome, &fullscreen(), &configured, &[]),
            GameConfidence::Certain
        );
    }

    #[test]
    fn a_borderless_direct3d_window_is_only_a_likely_game() {
        let unknown = process("Unknown.exe", "D:\\Unknown\\Unknown.exe", &["d3d11.dll"]);
        assert_eq!(
            classify(&unknown, &fullscreen(), &[], &[]),
            GameConfidence::Likely
        );
        assert_eq!(
            classify(&unknown, &windowed(), &[], &[]),
            GameConfidence::None
        );
    }

    #[test]
    fn a_window_that_fills_its_monitor_is_recorded_as_the_monitor() {
        assert_eq!(
            window_usability(&fullscreen(), GameConfidence::Certain),
            WindowUsability::CoversMonitor
        );
    }

    #[test]
    fn only_a_certain_game_in_a_real_window_is_captured_as_a_window() {
        assert_eq!(
            window_usability(&windowed(), GameConfidence::Certain),
            WindowUsability::Capturable
        );
        assert_eq!(
            window_usability(&windowed(), GameConfidence::Likely),
            WindowUsability::TooSmall
        );
        let tiny = WindowFacts {
            width: 240,
            height: 180,
            ..windowed()
        };
        assert_eq!(
            window_usability(&tiny, GameConfidence::Certain),
            WindowUsability::TooSmall
        );
    }

    #[test]
    fn a_window_a_few_pixels_short_of_its_monitor_still_counts_as_full() {
        let almost = WindowFacts {
            width: 2554,
            height: 1436,
            ..fullscreen()
        };
        assert!(covers_monitor(&almost));
    }
}
