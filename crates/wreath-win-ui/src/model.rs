use std::path::PathBuf;

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
    OpenClip(usize),
    Back,
    Refresh,
    SaveReplay,
    OpenClipsFolder,
    Search,
    ClearSearch,
    ToggleCursor,
    ToggleDesktopAudio,
    ToggleMicrophone,
    CycleDuration,
    CycleFrameRate,
    CycleCodec,
    CycleQuality,
    CycleDisplay,
    CycleMicrophone,
    CycleMicrophoneGain,
    CaptureHotkey,
    ChooseStorage,
    SaveSettings,
    CreateCollection,
    DeleteActiveCollection,
    PlayPause,
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
        if page != Page::Player {
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
}
