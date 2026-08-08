use std::path::PathBuf;
use std::time::Duration;

use wreath_core::clips::{self, Clip, Collection};
use wreath_core::config::Config;
use wreath_core::paths::AppPaths;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Home,
    Library,
    Collections,
    Settings,
    Player,
    Editor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsSection {
    Display,
    Quality,
    Audio,
    Controls,
    Storage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Navigate(Page),
    SettingsSection(SettingsSection),
    CancelPrompt,
    ConfirmPrompt,
    OpenClip(usize),
    Back,
    Refresh,
    SaveReplay,
    OpenClipsFolder,
    Search,
    ClearSearch,
    DismissNotice,
    ToggleSidebar,
    ToggleCursor,
    ToggleDesktopAudio,
    ChooseDesktopGain,
    ToggleMicrophone,
    ChooseDuration,
    ChooseFrameRate,
    ChooseCodec,
    ChooseQuality,
    ChooseDisplay,
    ChooseMicrophone,
    ChooseMicrophoneGain,
    ChooseStorageLimit,
    CaptureHotkey,
    ClearHotkey,
    ChooseStorage,
    SaveSettings,
    CreateCollection,
    DeleteActiveCollection,
    CancelDelete,
    ConfirmDelete,
    SelectCollection(Option<usize>),
    PlayPause,
    SeekPercent(u8),
    EditActiveClip,
    DragEditorStart,
    DragEditorEnd,
    SaveCut,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeleteTarget {
    Clip(usize),
    Collection(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptKind {
    RenameClip(usize),
    NewCollection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub kind: PromptKind,
    pub value: String,
}

impl Prompt {
    pub fn title(&self) -> &'static str {
        match self.kind {
            PromptKind::RenameClip(_) => "Rename clip",
            PromptKind::NewCollection => "New collection",
        }
    }

    pub fn label(&self) -> &'static str {
        match self.kind {
            PromptKind::RenameClip(_) => "Clip name",
            PromptKind::NewCollection => "Collection name",
        }
    }

    pub fn confirm(&self) -> &'static str {
        match self.kind {
            PromptKind::RenameClip(_) => "Rename",
            PromptKind::NewCollection => "Create",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DisplayOption {
    pub name: String,
    pub label: String,
    pub refresh_rate: f64,
    pub width: u32,
    pub height: u32,
}

pub struct UiModel {
    pub paths: AppPaths,
    pub config: Config,
    pub clips: Vec<Clip>,
    pub collections: Vec<Collection>,
    pub page: Page,
    pub previous_page: Page,
    pub settings_section: SettingsSection,
    pub query: String,
    pub search_focused: bool,
    pub active_collection: Option<PathBuf>,
    pub active_clip: Option<usize>,
    pub notice: Option<String>,
    pub hotkey_capture: bool,
    pub displays: Vec<DisplayOption>,
    pub microphone_names: Vec<(String, String)>,
    pub player_ready: bool,
    pub player_playing: bool,
    pub player_position_seconds: f64,
    pub player_duration_seconds: f64,
    pub player_aspect_ratio: f32,
    pub pending_delete: Option<DeleteTarget>,
    pub prompt: Option<Prompt>,
    pub sidebar_expanded: bool,
    pub editor_timing: Option<wreath_core::trim::ClipTiming>,
    pub editor_source: Option<PathBuf>,
    pub editor_start: Duration,
    pub editor_end: Duration,
    pub editor_loading: bool,
    pub editor_working: bool,
}

impl UiModel {
    pub fn load() -> Result<Self, String> {
        let paths = AppPaths::discover();
        let config = Config::load(&paths).map_err(|error| error.to_string())?;
        let mut model = Self {
            paths,
            config,
            clips: Vec::new(),
            collections: Vec::new(),
            page: Page::Home,
            previous_page: Page::Library,
            settings_section: SettingsSection::Display,
            query: String::new(),
            search_focused: false,
            active_collection: None,
            active_clip: None,
            notice: None,
            hotkey_capture: false,
            displays: Vec::new(),
            microphone_names: Vec::new(),
            player_ready: false,
            player_playing: false,
            player_position_seconds: 0.0,
            player_duration_seconds: 0.0,
            player_aspect_ratio: 16.0 / 9.0,
            pending_delete: None,
            prompt: None,
            sidebar_expanded: true,
            editor_timing: None,
            editor_source: None,
            editor_start: Duration::ZERO,
            editor_end: Duration::ZERO,
            editor_loading: false,
            editor_working: false,
        };
        model.refresh()?;
        Ok(model)
    }

    pub fn refresh(&mut self) -> Result<(), String> {
        self.clips =
            clips::scan(&self.config.storage.directory).map_err(|error| error.to_string())?;
        self.collections = clips::collections(&self.config.storage.directory)
            .map_err(|error| error.to_string())?;
        if let Some(active) = &self.active_collection
            && !self.collections.iter().any(|item| &item.path == active)
        {
            self.active_collection = None;
        }
        Ok(())
    }

    pub fn navigate(&mut self, page: Page) {
        self.search_focused = false;
        self.hotkey_capture = false;
        if !matches!(page, Page::Player | Page::Editor) {
            self.active_clip = None;
        }
        self.page = page;
    }

    pub fn open_clip(&mut self, index: usize) {
        if index < self.clips.len() {
            self.previous_page = self.page;
            self.active_clip = Some(index);
            self.page = Page::Player;
        }
    }

    pub fn begin_rename(&mut self, index: usize) -> bool {
        let Some(clip) = self.clips.get(index) else {
            return false;
        };
        self.prompt = Some(Prompt {
            kind: PromptKind::RenameClip(index),
            value: clip.title.clone(),
        });
        true
    }

    pub fn begin_new_collection(&mut self) {
        self.prompt = Some(Prompt {
            kind: PromptKind::NewCollection,
            value: String::new(),
        });
    }

    pub fn prompt_push(&mut self, character: char) {
        if let Some(prompt) = &mut self.prompt
            && !character.is_control()
            && prompt.value.chars().count() < 80
        {
            prompt.value.push(character);
        }
    }

    pub fn prompt_backspace(&mut self) {
        if let Some(prompt) = &mut self.prompt {
            prompt.value.pop();
        }
    }

    pub fn edit_active_clip(&mut self) -> bool {
        let Some(source) = self.active_clip().map(|clip| clip.path.clone()) else {
            return false;
        };
        if self.page != Page::Player {
            self.previous_page = self.page;
        }
        self.page = Page::Editor;
        self.editor_source = Some(source);
        self.editor_timing = None;
        self.editor_start = Duration::ZERO;
        self.editor_end = Duration::ZERO;
        self.editor_loading = true;
        self.editor_working = false;
        true
    }

    pub fn apply_editor_timing(&mut self, timing: wreath_core::trim::ClipTiming) {
        self.editor_start = Duration::ZERO;
        self.editor_end = timing.duration;
        self.editor_timing = Some(timing);
        self.editor_loading = false;
    }

    pub fn set_editor_start(&mut self, thousandths: u16) {
        let Some(timing) = &self.editor_timing else {
            return;
        };
        let requested = fraction(timing.duration, thousandths);
        let snapped = snap(timing, requested);
        let latest = self
            .editor_end
            .saturating_sub(wreath_core::trim::MINIMUM_LENGTH);
        self.editor_start = snapped.min(latest);
    }

    pub fn set_editor_end(&mut self, thousandths: u16) {
        let Some(timing) = &self.editor_timing else {
            return;
        };
        let requested = fraction(timing.duration, thousandths);
        let snapped = snap(timing, requested);
        let earliest = self
            .editor_start
            .saturating_add(wreath_core::trim::MINIMUM_LENGTH);
        self.editor_end = snapped.max(earliest).min(timing.duration);
    }

    pub fn editor_selected_duration(&self) -> Duration {
        self.editor_end.saturating_sub(self.editor_start)
    }

    pub fn visible_clip_indices(&self, limit: usize) -> Vec<usize> {
        let query = self.query.trim().to_ascii_lowercase();
        self.clips
            .iter()
            .enumerate()
            .filter(|(_, clip)| {
                let in_collection = self
                    .active_collection
                    .as_ref()
                    .is_none_or(|collection| clip.path.parent() == Some(collection.as_path()));
                let matches_query = query.is_empty()
                    || clip.title.to_ascii_lowercase().contains(&query)
                    || clip.path.file_name().is_some_and(|name| {
                        name.to_string_lossy().to_ascii_lowercase().contains(&query)
                    });
                in_collection && matches_query
            })
            .map(|(index, _)| index)
            .take(limit)
            .collect()
    }

    pub fn total_size_bytes(&self) -> u64 {
        self.clips.iter().map(|clip| clip.size_bytes).sum()
    }

    pub fn active_clip(&self) -> Option<&Clip> {
        self.active_clip.and_then(|index| self.clips.get(index))
    }

    pub fn selected_display(&self) -> Option<&DisplayOption> {
        let configured = self.config.capture.monitor.as_deref();
        configured
            .and_then(|name| {
                self.displays
                    .iter()
                    .find(|display| display.name.eq_ignore_ascii_case(name))
            })
            .or_else(|| self.displays.first())
    }

    /// Quality choices labelled with what they actually cost.
    ///
    /// The menu used to list bare percentages, which say nothing about the
    /// thing people care about — how large the clip ends up. Each choice now
    /// carries the bitrate it aims for and the size a full replay reaches on
    /// the selected monitor, at the configured frame rate, codec and duration.
    pub fn quality_options(&self) -> Vec<(u8, String)> {
        let (width, height) = self
            .selected_display()
            .map_or((1920, 1080), |display| (display.width, display.height));
        let monitor = wreath_core::display::Monitor {
            id: 0,
            name: String::new(),
            description: String::new(),
            make: String::new(),
            model: String::new(),
            serial: String::new(),
            width,
            height,
            refresh_rate: f64::from(self.config.capture.frames_per_second),
            focused: true,
            disabled: false,
        };
        let seconds = self.config.capture.duration_seconds;
        let mut values = vec![50, 65, 75, 85, 95, 100];
        values.push(self.config.capture.quality.min(100));
        values.sort_unstable();
        values.dedup();
        values
            .into_iter()
            .map(|quality| {
                let mut spec = wreath_core::replay::ReplaySpec::from_config(&self.config, &monitor);
                spec.quality = quality;
                let megabits = spec.target_bitrate_kbps().saturating_add(500) / 1_000;
                let megabytes = spec.estimated_buffer_megabytes();
                (
                    quality,
                    format!(
                        "{quality}% · {megabits} Mbit/s · about {megabytes} MB per {seconds} s"
                    ),
                )
            })
            .collect()
    }

    pub fn frame_rate_options(&self) -> Vec<u16> {
        let native_rate = self
            .selected_display()
            .map_or(60, |display| display.refresh_rate.round() as u16)
            .clamp(15, wreath_core::config::MAX_FRAMES_PER_SECOND);
        let mut rates = [30, 48, 60]
            .into_iter()
            .filter(|rate| *rate <= native_rate)
            .collect::<Vec<_>>();
        rates.push(native_rate);
        rates.push(
            self.config
                .capture
                .frames_per_second
                .clamp(15, wreath_core::config::MAX_FRAMES_PER_SECOND),
        );
        rates.sort_unstable();
        rates.dedup();
        rates
    }
}

fn fraction(duration: Duration, thousandths: u16) -> Duration {
    duration.mul_f64(f64::from(thousandths.min(1_000)) / 1_000.0)
}

fn snap(timing: &wreath_core::trim::ClipTiming, position: Duration) -> Duration {
    timing
        .nearest_keyframe(position)
        .filter(|keyframe| keyframe.abs_diff(position) <= wreath_core::trim::SNAP_TOLERANCE)
        .unwrap_or(position)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    fn model() -> UiModel {
        UiModel {
            paths: AppPaths::discover(),
            config: Config::default(),
            clips: vec![
                Clip {
                    path: PathBuf::from("/clips/Ranked Match.mp4"),
                    title: "Ranked Match".into(),
                    size_bytes: 10,
                    modified: SystemTime::UNIX_EPOCH,
                },
                Clip {
                    path: PathBuf::from("/clips/other.mp4"),
                    title: "Other".into(),
                    size_bytes: 15,
                    modified: SystemTime::UNIX_EPOCH,
                },
            ],
            collections: Vec::new(),
            page: Page::Library,
            previous_page: Page::Home,
            settings_section: SettingsSection::Display,
            query: String::new(),
            search_focused: false,
            active_collection: None,
            active_clip: None,
            notice: None,
            hotkey_capture: false,
            displays: Vec::new(),
            microphone_names: Vec::new(),
            player_ready: false,
            player_playing: false,
            player_position_seconds: 0.0,
            player_duration_seconds: 0.0,
            player_aspect_ratio: 16.0 / 9.0,
            pending_delete: None,
            prompt: None,
            sidebar_expanded: true,
            editor_timing: None,
            editor_source: None,
            editor_start: Duration::ZERO,
            editor_end: Duration::ZERO,
            editor_loading: false,
            editor_working: false,
        }
    }

    #[test]
    fn search_is_case_insensitive_and_bounded() {
        let mut model = model();
        model.query = "RANKED".into();
        assert_eq!(model.visible_clip_indices(200), vec![0]);
        model.query.clear();
        assert_eq!(model.visible_clip_indices(1), vec![0]);
    }

    #[test]
    fn player_remembers_its_origin() {
        let mut model = model();
        model.open_clip(1);
        assert_eq!(model.page, Page::Player);
        assert_eq!(model.previous_page, Page::Library);
        assert_eq!(model.active_clip().unwrap().title, "Other");
    }

    #[test]
    fn total_size_sums_all_clips() {
        assert_eq!(model().total_size_bytes(), 25);
    }

    #[test]
    fn frame_rates_follow_the_selected_monitor_and_keep_the_current_value() {
        let mut model = model();
        model.displays.push(DisplayOption {
            name: "DISPLAY1".into(),
            label: "DISPLAY1 · 2560×1440 · 144 Hz".into(),
            refresh_rate: 144.0,
            width: 2560,
            height: 1440,
        });
        model.config.capture.monitor = Some("DISPLAY1".into());
        model.config.capture.frames_per_second = 50;

        // A 144 Hz monitor no longer offers 144: hardware encoders could not
        // sustain it, so the choice only ever produced dropped frames.
        assert_eq!(model.frame_rate_options(), vec![30, 48, 50, 60]);
    }

    /// A bare percentage says nothing about what a setting costs, which is the
    /// one thing people want to know before changing it.
    #[test]
    fn quality_choices_carry_their_bitrate_and_clip_size() {
        let mut model = model();
        model.displays.push(DisplayOption {
            name: "DISPLAY1".into(),
            label: "DISPLAY1 · 2560×1440 · 60 Hz".into(),
            refresh_rate: 60.0,
            width: 2560,
            height: 1440,
        });
        model.config.capture.monitor = Some("DISPLAY1".into());
        model.config.capture.frames_per_second = 60;
        model.config.capture.duration_seconds = 30;
        model.config.capture.quality = 75;

        let options = model.quality_options();
        let (value, label) = options
            .iter()
            .find(|(value, _)| *value == 75)
            .expect("the configured quality is always offered");

        assert_eq!(*value, 75);
        assert_eq!(label, "75% · 27 Mbit/s · about 94 MB per 30 s");

        // A lower setting has to read as visibly cheaper.
        let cheaper = options
            .iter()
            .find(|(value, _)| *value == 50)
            .expect("50 is offered");
        assert_eq!(cheaper.1, "50% · 21 Mbit/s · about 75 MB per 30 s");
    }

    #[test]
    fn a_slower_monitor_still_caps_the_frame_rate_choices() {
        let mut model = model();
        model.displays.push(DisplayOption {
            name: "DISPLAY1".into(),
            label: "DISPLAY1 · 1920×1080 · 30 Hz".into(),
            refresh_rate: 30.0,
            width: 1920,
            height: 1080,
        });
        model.config.capture.monitor = Some("DISPLAY1".into());
        model.config.capture.frames_per_second = 30;

        assert_eq!(model.frame_rate_options(), vec![30]);
    }

    #[test]
    fn editor_handles_snap_to_nearby_keyframes_and_keep_a_valid_range() {
        let mut model = model();
        model.active_clip = Some(0);
        assert!(model.edit_active_clip());
        model.apply_editor_timing(wreath_core::trim::ClipTiming {
            duration: Duration::from_secs(10),
            keyframes: vec![
                Duration::ZERO,
                Duration::from_secs(2),
                Duration::from_secs(8),
            ],
        });

        model.set_editor_start(201);
        model.set_editor_end(799);

        assert_eq!(model.editor_start, Duration::from_secs(2));
        assert_eq!(model.editor_end, Duration::from_secs(8));
        assert_eq!(model.editor_selected_duration(), Duration::from_secs(6));
    }
}
