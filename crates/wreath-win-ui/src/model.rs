use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use wreath_core::clips::{self, Clip, Collection};
use wreath_core::config::Config;
use wreath_core::favorites::Favorites;
use wreath_core::ipc::DaemonState;
use wreath_core::paths::AppPaths;

use crate::clock::{self, Civil};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Library,
    Collections,
    Settings,
    Player,
    Editor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipTab {
    All,
    Favorites,
}

impl ClipTab {
    pub const fn label(self) -> &'static str {
        match self {
            Self::All => "Alle Clips",
            Self::Favorites => "Favoriten",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeFilter {
    All,
    Today,
    Week,
    Month,
}

impl TimeFilter {
    pub const OPTIONS: [Self; 4] = [Self::All, Self::Today, Self::Week, Self::Month];

    pub const fn label(self) -> &'static str {
        match self {
            Self::All => "Alle Zeit",
            Self::Today => "Heute",
            Self::Week => "Diese Woche",
            Self::Month => "Dieser Monat",
        }
    }

    fn keeps(self, clip: Civil, today: Civil) -> bool {
        match self {
            Self::All => true,
            Self::Today => clock::within_days(clip, today, 1),
            Self::Week => clock::within_days(clip, today, 7),
            Self::Month => clock::same_month(clip, today),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeFilter {
    All,
    Replay,
    Cut,
}

impl TypeFilter {
    pub const OPTIONS: [Self; 3] = [Self::All, Self::Replay, Self::Cut];

    pub const fn label(self) -> &'static str {
        match self {
            Self::All => "Alle",
            Self::Replay => "Replays",
            Self::Cut => "Zuschnitte",
        }
    }

    fn keeps(self, clip: &Clip) -> bool {
        let is_cut = clip.title.contains("(cut)");
        match self {
            Self::All => true,
            Self::Replay => !is_cut,
            Self::Cut => is_cut,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeFilter {
    All,
    Small,
    Medium,
    Large,
}

impl SizeFilter {
    pub const OPTIONS: [Self; 4] = [Self::All, Self::Small, Self::Medium, Self::Large];

    pub const fn label(self) -> &'static str {
        match self {
            Self::All => "Alle",
            Self::Small => "Bis 25 MB",
            Self::Medium => "25 bis 100 MB",
            Self::Large => "Über 100 MB",
        }
    }

    fn keeps(self, size_bytes: u64) -> bool {
        const SMALL: u64 = 25 * 1_048_576;
        const LARGE: u64 = 100 * 1_048_576;
        match self {
            Self::All => true,
            Self::Small => size_bytes < SMALL,
            Self::Medium => (SMALL..=LARGE).contains(&size_bytes),
            Self::Large => size_bytes > LARGE,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DaemonSnapshot {
    pub state: Option<DaemonState>,
    pub buffered_seconds: u16,
    pub error: Option<String>,
}

impl DaemonSnapshot {
    pub fn is_recording(&self) -> bool {
        self.state == Some(DaemonState::Recording)
    }

    pub const fn toolbar_headline(&self) -> &'static str {
        match self.state {
            Some(DaemonState::Recording) => "REPLAY AKTIV",
            Some(DaemonState::Starting) => "REPLAY STARTET",
            Some(DaemonState::Paused) => "REPLAY PAUSIERT",
            Some(DaemonState::Error) => "REPLAY GESTÖRT",
            None => "RECORDER OFFLINE",
        }
    }

    pub const fn status_headline(&self) -> &'static str {
        match self.state {
            Some(DaemonState::Recording) => "REPLAY LÄUFT",
            Some(DaemonState::Starting) => "REPLAY STARTET",
            Some(DaemonState::Paused) => "REPLAY PAUSIERT",
            Some(DaemonState::Error) => "REPLAY GESTÖRT",
            None => "RECORDER OFFLINE",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipGroup {
    pub label: String,
    pub indices: Vec<usize>,
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
    OpenClipMenu(usize),
    Back,
    Refresh,
    SetCollectionCardsPage(usize),
    SetCollectionClipsPage(usize),
    SetLibraryGrid(bool),
    SetClipTab(ClipTab),
    ToggleFilterPanel,
    ChooseTimeFilter,
    ChooseCollectionFilter,
    ChooseTypeFilter,
    ChooseSizeFilter,
    ChooseClipSort,
    ResetFilters,
    ToggleCollectionSort,
    SetCollectionsGrid(bool),
    SaveReplay,
    OpenClipsFolder,
    Search,
    ClearSearch,
    PlaceSearchCaret(usize),
    PlacePromptCaret(usize),
    DismissContextMenu,
    EditClip(usize),
    RenameClip(usize),
    ToggleFavorite(usize),
    OpenClipExternally(usize),
    RenameActiveClip,
    MoveClipToCollection { clip: usize, collection: usize },
    ToggleSelectionMode,
    ToggleClipSelection(usize),
    SelectAllVisibleClips,
    ToggleCollectionPicker,
    MoveSelectedToCollection(usize),
    DeleteClip(usize),
    DismissNotice,
    MinimizeWindow,
    ToggleMaximizeWindow,
    CloseWindow,
    ToggleAutostart,
    ToggleCursor,
    ToggleDesktopAudio,
    ChooseDesktopDevice,
    ChooseDesktopGain,
    DragDesktopGain,
    ChooseAudioMode,
    ToggleMicrophone,
    ChooseDuration,
    ChooseFrameRate,
    ChooseCodec,
    ChooseQuality,
    ChooseDisplay,
    ChooseMicrophone,
    ChooseMicrophoneGain,
    DragMicrophoneGain,
    ChooseStorageLimit,
    DismissSettingsMenu,
    SelectSettingsOption(usize),
    CaptureHotkey,
    ClearHotkey,
    ChooseStorage,
    SaveSettings,
    CreateCollection,
    DeleteActiveCollection,
    RenameActiveCollection,
    CancelDelete,
    ConfirmDelete,
    SelectCollection(Option<usize>),
    PreviousClip,
    NextClip,
    PlayPause,
    DragPlayerSeek,
    DragPlayerVolume,
    ToggleMute,
    ToggleFullscreen,
    EditActiveClip,
    SetTrimReplace(bool),
    UndoEditorTrim,
    RedoEditorTrim,
    ResetEditorTrim,
    SetEditorStartToPlayhead,
    SetEditorEndToPlayhead,
    DragEditorStart,
    DragEditorEnd,
    DragEditorPlayhead,
    SaveCut,
    ReplaceCut,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeleteTarget {
    Clip(usize),
    Collection(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptKind {
    RenameClip(usize),
    RenameCollection(PathBuf),
    NewCollection,
}

pub const PROMPT_MAX_CHARACTERS: usize = 80;

#[derive(Debug, Default)]
pub struct NoticeExpiry {
    last: Option<String>,
    deadline: Option<Instant>,
}

impl NoticeExpiry {
    pub fn tick(&mut self, notice: &mut Option<String>, now: Instant, lifetime: Duration) -> bool {
        if *notice != self.last {
            self.last = notice.clone();
            self.deadline = notice.as_ref().map(|_| now + lifetime);
            return true;
        }
        if self.deadline.is_some_and(|deadline| now >= deadline) {
            *notice = None;
            self.last = None;
            self.deadline = None;
            return true;
        }
        false
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextInput {
    pub value: String,
    pub caret: usize,
    pub anchor: usize,
    max_characters: usize,
}

impl TextInput {
    pub fn new(value: String, max_characters: usize) -> Self {
        let caret = value.chars().count();
        Self {
            value,
            caret,
            anchor: caret,
            max_characters,
        }
    }

    pub fn characters(&self) -> usize {
        self.value.chars().count()
    }

    pub fn selection(&self) -> (usize, usize) {
        (self.anchor.min(self.caret), self.anchor.max(self.caret))
    }

    pub fn has_selection(&self) -> bool {
        self.anchor != self.caret
    }

    pub fn selected_text(&self) -> String {
        let (start, end) = self.selection();
        self.value.chars().skip(start).take(end - start).collect()
    }

    fn byte_index(&self, character: usize) -> usize {
        self.value
            .char_indices()
            .nth(character)
            .map_or(self.value.len(), |(index, _)| index)
    }

    fn delete_selection(&mut self) -> bool {
        let (start, end) = self.selection();
        if start == end {
            return false;
        }
        let from = self.byte_index(start);
        let to = self.byte_index(end);
        self.value.replace_range(from..to, "");
        self.caret = start;
        self.anchor = start;
        true
    }

    pub fn insert(&mut self, character: char) {
        let mut buffer = [0_u8; 4];
        self.insert_text(character.encode_utf8(&mut buffer));
    }

    pub fn insert_text(&mut self, text: &str) {
        self.delete_selection();
        let available = self.max_characters.saturating_sub(self.characters());
        if available == 0 {
            return;
        }
        let clean = text
            .chars()
            .filter(|character| !character.is_control())
            .take(available)
            .collect::<String>();
        if clean.is_empty() {
            return;
        }
        let inserted = clean.chars().count();
        let at = self.byte_index(self.caret);
        self.value.insert_str(at, &clean);
        self.caret += inserted;
        self.anchor = self.caret;
    }

    pub fn backspace(&mut self) {
        if self.delete_selection() || self.caret == 0 {
            return;
        }
        let from = self.byte_index(self.caret - 1);
        let to = self.byte_index(self.caret);
        self.value.replace_range(from..to, "");
        self.caret -= 1;
        self.anchor = self.caret;
    }

    pub fn delete(&mut self) {
        if self.delete_selection() || self.caret >= self.characters() {
            return;
        }
        let from = self.byte_index(self.caret);
        let to = self.byte_index(self.caret + 1);
        self.value.replace_range(from..to, "");
        self.anchor = self.caret;
    }

    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.caret = self.characters();
    }

    pub fn move_caret(&mut self, to: usize, extend: bool) {
        self.caret = to.min(self.characters());
        if !extend {
            self.anchor = self.caret;
        }
    }

    pub fn caret_left(&mut self, extend: bool) {
        if !extend && self.has_selection() {
            let (start, _) = self.selection();
            self.move_caret(start, false);
            return;
        }
        self.move_caret(self.caret.saturating_sub(1), extend);
    }

    pub fn caret_right(&mut self, extend: bool) {
        if !extend && self.has_selection() {
            let (_, end) = self.selection();
            self.move_caret(end, false);
            return;
        }
        self.move_caret(self.caret.saturating_add(1), extend);
    }

    pub fn caret_home(&mut self, extend: bool) {
        self.move_caret(0, extend);
    }

    pub fn caret_end(&mut self, extend: bool) {
        self.move_caret(self.characters(), extend);
    }

    pub fn clear(&mut self) {
        self.value.clear();
        self.caret = 0;
        self.anchor = 0;
    }
}

pub const QUALITY_PRESETS: [(u8, &str); 5] = [
    (50, "Low"),
    (65, "Medium"),
    (75, "High"),
    (85, "Ultra"),
    (100, "Insane"),
];

pub fn quality_label(quality: u8) -> String {
    QUALITY_PRESETS
        .iter()
        .find(|(value, _)| *value == quality)
        .map_or_else(|| format!("{quality}%"), |(_, name)| (*name).to_owned())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsMenuKind {
    TimeFilter,
    CollectionFilter,
    TypeFilter,
    SizeFilter,
    ClipSort,
    Display,
    FrameRate,
    Duration,
    Codec,
    Quality,
    AudioMode,
    DesktopDevice,
    DesktopGain,
    Microphone,
    MicrophoneGain,
    StorageLimit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsMenuItem {
    pub label: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsMenu {
    pub kind: SettingsMenuKind,
    pub items: Vec<SettingsMenuItem>,
    pub selected: Option<usize>,
    pub highlighted: usize,
}

impl SettingsMenu {
    pub fn new(
        kind: SettingsMenuKind,
        items: Vec<SettingsMenuItem>,
        selected: Option<usize>,
    ) -> Self {
        let highlighted = selected.unwrap_or(0).min(items.len().saturating_sub(1));
        Self {
            kind,
            items,
            selected,
            highlighted,
        }
    }

    pub fn move_highlight(&mut self, direction: i32) {
        if self.items.is_empty() {
            return;
        }
        self.highlighted = if direction < 0 {
            self.highlighted
                .checked_sub(1)
                .unwrap_or(self.items.len() - 1)
        } else {
            (self.highlighted + 1) % self.items.len()
        };
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualityOption {
    pub value: u8,
    pub label: String,
    pub megabytes: u64,
    pub seconds: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub kind: PromptKind,
    pub input: TextInput,
}

impl Prompt {
    pub fn new(kind: PromptKind, value: String) -> Self {
        Self {
            kind,
            input: TextInput::new(value, PROMPT_MAX_CHARACTERS),
        }
    }

    pub fn title(&self) -> &'static str {
        match self.kind {
            PromptKind::RenameClip(_) => "Clip umbenennen",
            PromptKind::RenameCollection(_) => "Sammlung umbenennen",
            PromptKind::NewCollection => "Neue Sammlung",
        }
    }

    pub fn label(&self) -> &'static str {
        match self.kind {
            PromptKind::RenameClip(_) => "Clip-Name",
            PromptKind::RenameCollection(_) => "Name der Sammlung",
            PromptKind::NewCollection => "Name der Sammlung",
        }
    }

    pub fn confirm(&self) -> &'static str {
        match self.kind {
            PromptKind::RenameClip(_) | PromptKind::RenameCollection(_) => "Umbenennen",
            PromptKind::NewCollection => "Erstellen",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DisplayOption {
    pub name: String,
    pub label: String,
    pub short_label: String,
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
    pub settings_menu: Option<SettingsMenu>,
    pub search: TextInput,
    pub search_focused: bool,
    pub collection_cards_page: usize,
    pub collection_clips_page: usize,
    pub clips_oldest_first: bool,
    pub library_grid: bool,
    pub clip_tab: ClipTab,
    pub filter_panel_open: bool,
    pub filter_time: TimeFilter,
    pub filter_collection: Option<PathBuf>,
    pub filter_type: TypeFilter,
    pub filter_size: SizeFilter,
    pub library_scroll: f32,
    pub favorites: Favorites,
    pub daemon: DaemonSnapshot,
    pub microphone_level: u8,
    pub collections_descending: bool,
    pub collections_grid: bool,
    pub context_menu: Option<ClipContextMenu>,
    pub active_collection: Option<PathBuf>,
    pub active_clip: Option<usize>,
    pub selection_mode: bool,
    pub selected_clips: HashSet<PathBuf>,
    pub collection_picker_open: bool,
    pub clip_drag_preview: Option<ClipDragPreview>,
    pub notice: Option<String>,
    pub autostart_enabled: bool,
    pub hotkey_capture: bool,
    pub hotkey_modifiers: Vec<String>,
    pub hotkey_pending: bool,
    pub hotkey_deferred: bool,
    pub hotkey_error: Option<String>,
    pub displays: Vec<DisplayOption>,
    pub microphone_names: Vec<(String, String)>,
    pub output_names: Vec<(String, String)>,
    pub player_ready: bool,
    pub player_playing: bool,
    pub player_position_seconds: f64,
    pub player_duration_seconds: f64,
    pub player_aspect_ratio: f32,
    pub player_video_width: u32,
    pub player_video_height: u32,
    pub player_volume_percent: u8,
    pub player_last_audible_percent: u8,
    pub pending_delete: Option<DeleteTarget>,
    pub prompt: Option<Prompt>,
    pub editor_timing: Option<wreath_core::trim::ClipTiming>,
    pub editor_source: Option<PathBuf>,
    pub editor_start: Duration,
    pub editor_end: Duration,
    editor_undo: Vec<(Duration, Duration)>,
    editor_redo: Vec<(Duration, Duration)>,
    pub editor_loading: bool,
    pub editor_working: bool,
    pub trim_replace_original: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClipDragPreview {
    pub clip: usize,
    pub count: usize,
    pub x: f32,
    pub y: f32,
    pub target_collection: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClipContextMenu {
    pub clip: usize,
    pub x: f32,
    pub y: f32,
}

impl UiModel {
    pub fn load() -> Result<Self, String> {
        let paths = AppPaths::discover();
        let config = Config::load(&paths).map_err(|error| error.to_string())?;
        let favorites = Favorites::load(&paths.favorites_file, &config.storage.directory);
        let mut model = Self {
            paths,
            config,
            clips: Vec::new(),
            collections: Vec::new(),
            page: Page::Library,
            previous_page: Page::Library,
            settings_section: SettingsSection::Display,
            settings_menu: None,
            search: TextInput::new(String::new(), PROMPT_MAX_CHARACTERS),
            search_focused: false,
            collection_cards_page: 0,
            collection_clips_page: 0,
            clips_oldest_first: false,
            library_grid: true,
            clip_tab: ClipTab::All,
            filter_panel_open: true,
            filter_time: TimeFilter::All,
            filter_collection: None,
            filter_type: TypeFilter::All,
            filter_size: SizeFilter::All,
            library_scroll: 0.0,
            favorites,
            daemon: DaemonSnapshot::default(),
            microphone_level: 0,
            collections_descending: false,
            collections_grid: true,
            context_menu: None,
            active_collection: None,
            active_clip: None,
            selection_mode: false,
            selected_clips: HashSet::new(),
            collection_picker_open: false,
            clip_drag_preview: None,
            notice: None,
            autostart_enabled: false,
            hotkey_capture: false,
            hotkey_modifiers: Vec::new(),
            hotkey_pending: false,
            hotkey_deferred: false,
            hotkey_error: None,
            displays: Vec::new(),
            microphone_names: Vec::new(),
            output_names: Vec::new(),
            player_ready: false,
            player_playing: false,
            player_position_seconds: 0.0,
            player_duration_seconds: 0.0,
            player_aspect_ratio: 16.0 / 9.0,
            player_video_width: 0,
            player_video_height: 0,
            player_volume_percent: 100,
            player_last_audible_percent: 100,
            pending_delete: None,
            prompt: None,
            editor_timing: None,
            editor_source: None,
            editor_start: Duration::ZERO,
            editor_end: Duration::ZERO,
            editor_undo: Vec::new(),
            editor_redo: Vec::new(),
            editor_loading: false,
            editor_working: false,
            trim_replace_original: false,
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
        self.selected_clips
            .retain(|path| self.clips.iter().any(|clip| &clip.path == path));
        self.favorites.set_root(&self.config.storage.directory);
        let stored = self.favorites.len();
        let paths = self
            .clips
            .iter()
            .map(|clip| clip.path.clone())
            .collect::<Vec<_>>();
        self.favorites.retain_existing(&paths);
        if self.favorites.len() != stored {
            let _ = self.favorites.save();
        }
        if self
            .filter_collection
            .as_ref()
            .is_some_and(|filter| !self.collections.iter().any(|item| &item.path == filter))
        {
            self.filter_collection = None;
        }
        Ok(())
    }

    pub fn navigate(&mut self, page: Page) {
        if matches!(self.page, Page::Library | Page::Collections)
            && matches!(page, Page::Library | Page::Collections)
            && self.page != page
        {
            self.search.clear();
            self.library_scroll = 0.0;
            self.collection_cards_page = 0;
            self.collection_clips_page = 0;
        }
        self.search_focused = false;
        self.context_menu = None;
        self.settings_menu = None;
        self.hotkey_capture = false;
        self.hotkey_modifiers.clear();
        self.hotkey_error = None;
        self.clear_clip_selection();
        if !matches!(page, Page::Player | Page::Editor) {
            self.active_clip = None;
        }
        self.page = page;
    }

    pub fn clear_clip_selection(&mut self) {
        self.selection_mode = false;
        self.selected_clips.clear();
        self.collection_picker_open = false;
        self.clip_drag_preview = None;
    }

    pub fn toggle_selection_mode(&mut self) {
        if self.selection_mode {
            self.clear_clip_selection();
        } else {
            self.selection_mode = true;
            self.collection_picker_open = false;
        }
    }

    pub fn toggle_clip_selection(&mut self, index: usize) -> bool {
        let Some(path) = self.clips.get(index).map(|clip| clip.path.clone()) else {
            return false;
        };
        self.selection_mode = true;
        if !self.selected_clips.remove(&path) {
            self.selected_clips.insert(path);
        }
        if self.selected_clips.is_empty() {
            self.collection_picker_open = false;
        }
        true
    }

    pub fn select_all_visible_clips(&mut self) {
        self.selection_mode = true;
        for index in self.visible_clip_indices(usize::MAX) {
            if let Some(clip) = self.clips.get(index) {
                self.selected_clips.insert(clip.path.clone());
            }
        }
    }

    pub fn clip_is_selected(&self, index: usize) -> bool {
        self.clips
            .get(index)
            .is_some_and(|clip| self.selected_clips.contains(&clip.path))
    }

    pub fn selected_clip_indices(&self) -> Vec<usize> {
        self.clips
            .iter()
            .enumerate()
            .filter_map(|(index, clip)| self.selected_clips.contains(&clip.path).then_some(index))
            .collect()
    }

    pub fn open_clip(&mut self, index: usize) {
        if index < self.clips.len() {
            self.previous_page = self.page;
            self.active_clip = Some(index);
            self.page = Page::Player;
        }
    }

    pub fn adjacent_clip(&self, offset: isize) -> Option<usize> {
        let active = self.active_clip?;
        let visible = self.visible_clip_indices(usize::MAX);
        let position = visible.iter().position(|index| *index == active)?;
        position
            .checked_add_signed(offset)
            .and_then(|position| visible.get(position))
            .copied()
    }

    pub fn select_adjacent_clip(&mut self, offset: isize) -> bool {
        let Some(index) = self.adjacent_clip(offset) else {
            return false;
        };
        self.active_clip = Some(index);
        self.page = Page::Player;
        true
    }

    pub fn reset_player_state(&mut self) {
        self.player_ready = false;
        self.player_playing = false;
        self.player_position_seconds = 0.0;
        self.player_duration_seconds = 0.0;
        self.player_aspect_ratio = 16.0 / 9.0;
        self.player_video_width = 0;
        self.player_video_height = 0;
    }

    pub fn set_player_volume(&mut self, percent: u8) {
        self.player_volume_percent = percent.min(100);
        if self.player_volume_percent > 0 {
            self.player_last_audible_percent = self.player_volume_percent;
        }
    }

    pub fn toggle_player_mute(&mut self) {
        if self.player_volume_percent == 0 {
            self.player_volume_percent = self.player_last_audible_percent.max(1);
        } else {
            self.player_last_audible_percent = self.player_volume_percent;
            self.player_volume_percent = 0;
        }
    }

    pub fn begin_rename(&mut self, index: usize) -> bool {
        let Some(clip) = self.clips.get(index) else {
            return false;
        };
        let mut prompt = Prompt::new(PromptKind::RenameClip(index), clip.title.clone());
        prompt.input.select_all();
        self.prompt = Some(prompt);
        true
    }

    pub fn begin_new_collection(&mut self) {
        self.prompt = Some(Prompt::new(PromptKind::NewCollection, String::new()));
    }

    pub fn begin_rename_collection(&mut self) -> bool {
        let Some(path) = self.active_collection.clone() else {
            return false;
        };
        let Some(collection) = self.collections.iter().find(|item| item.path == path) else {
            return false;
        };
        let mut prompt = Prompt::new(PromptKind::RenameCollection(path), collection.name.clone());
        prompt.input.select_all();
        self.prompt = Some(prompt);
        true
    }

    pub fn edit_active_clip(&mut self) -> bool {
        let Some(source) = self.active_clip().map(|clip| clip.path.clone()) else {
            return false;
        };
        if !matches!(self.page, Page::Player | Page::Editor) {
            self.previous_page = self.page;
        }
        self.page = Page::Editor;
        self.editor_source = Some(source);
        self.editor_timing = None;
        self.editor_start = Duration::ZERO;
        self.editor_end = Duration::ZERO;
        self.editor_undo.clear();
        self.editor_redo.clear();
        self.editor_loading = true;
        self.editor_working = false;
        self.trim_replace_original = false;
        true
    }

    pub fn reset_editor_trim(&mut self) {
        let previous = (self.editor_start, self.editor_end);
        self.editor_start = Duration::ZERO;
        if let Some(timing) = &self.editor_timing {
            self.editor_end = timing.duration;
        }
        self.commit_editor_trim_change(previous);
    }

    pub fn set_editor_start_to_playhead(&mut self) {
        let previous = (self.editor_start, self.editor_end);
        let Some(timing) = &self.editor_timing else {
            return;
        };
        let selected = Duration::from_secs_f64(
            self.player_position_seconds
                .clamp(0.0, timing.duration.as_secs_f64()),
        );
        let latest = self
            .editor_end
            .saturating_sub(wreath_core::trim::MINIMUM_LENGTH);
        self.editor_start = snap(timing, selected).min(latest);
        self.commit_editor_trim_change(previous);
    }

    pub fn set_editor_end_to_playhead(&mut self) {
        let previous = (self.editor_start, self.editor_end);
        let Some(timing) = &self.editor_timing else {
            return;
        };
        let selected = Duration::from_secs_f64(
            self.player_position_seconds
                .clamp(0.0, timing.duration.as_secs_f64()),
        );
        let earliest = self
            .editor_start
            .saturating_add(wreath_core::trim::MINIMUM_LENGTH);
        self.editor_end = snap(timing, selected).max(earliest).min(timing.duration);
        self.commit_editor_trim_change(previous);
    }

    pub fn apply_editor_timing(&mut self, timing: wreath_core::trim::ClipTiming) {
        self.editor_start = Duration::ZERO;
        self.editor_end = timing.duration;
        self.editor_undo.clear();
        self.editor_redo.clear();
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

    pub fn can_undo_editor_trim(&self) -> bool {
        !self.editor_undo.is_empty()
    }

    pub fn can_redo_editor_trim(&self) -> bool {
        !self.editor_redo.is_empty()
    }

    pub fn commit_editor_trim_change(&mut self, previous: (Duration, Duration)) {
        let current = (self.editor_start, self.editor_end);
        if current == previous {
            return;
        }
        if self.editor_undo.last().copied() != Some(previous) {
            self.editor_undo.push(previous);
            if self.editor_undo.len() > 64 {
                self.editor_undo.remove(0);
            }
        }
        self.editor_redo.clear();
    }

    pub fn undo_editor_trim(&mut self) -> bool {
        let Some(previous) = self.editor_undo.pop() else {
            return false;
        };
        self.editor_redo.push((self.editor_start, self.editor_end));
        (self.editor_start, self.editor_end) = previous;
        true
    }

    pub fn redo_editor_trim(&mut self) -> bool {
        let Some(next) = self.editor_redo.pop() else {
            return false;
        };
        self.editor_undo.push((self.editor_start, self.editor_end));
        (self.editor_start, self.editor_end) = next;
        true
    }

    pub fn visible_clip_indices(&self, limit: usize) -> Vec<usize> {
        self.visible_clip_indices_at(limit, clock::now())
    }

    pub fn visible_clip_indices_at(&self, limit: usize, today: Civil) -> Vec<usize> {
        let library_scope = self.page == Page::Library
            || (matches!(self.page, Page::Player | Page::Editor)
                && self.previous_page == Page::Library);
        let collection_scope = self.page == Page::Collections
            || (matches!(self.page, Page::Player | Page::Editor)
                && self.previous_page == Page::Collections);
        let query = if library_scope {
            self.search.value.trim().to_ascii_lowercase()
        } else {
            String::new()
        };
        let mut visible =
            self.clips
                .iter()
                .enumerate()
                .filter(|(_, clip)| {
                    let in_collection = !collection_scope
                        || self.active_collection.as_ref().is_none_or(|collection| {
                            clip.path.parent() == Some(collection.as_path())
                        });
                    let matches_query = query.is_empty()
                        || clip.title.to_ascii_lowercase().contains(&query)
                        || clip.path.file_name().is_some_and(|name| {
                            name.to_string_lossy().to_ascii_lowercase().contains(&query)
                        });
                    let matches_filters = !library_scope || self.passes_clip_filters(clip, today);
                    in_collection && matches_query && matches_filters
                })
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
        if self.clips_oldest_first {
            visible.reverse();
        }
        visible.truncate(limit);
        visible
    }

    pub fn visible_collection_indices(&self) -> Vec<usize> {
        let query = self.search.value.trim().to_ascii_lowercase();
        let mut visible = self
            .collections
            .iter()
            .enumerate()
            .filter(|(_, collection)| {
                query.is_empty() || collection.name.to_ascii_lowercase().contains(&query)
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        visible.sort_by(|left, right| {
            self.collections[*left]
                .name
                .to_ascii_lowercase()
                .cmp(&self.collections[*right].name.to_ascii_lowercase())
        });
        if self.collections_descending {
            visible.reverse();
        }
        visible
    }

    fn passes_clip_filters(&self, clip: &Clip, today: Civil) -> bool {
        if self.clip_tab == ClipTab::Favorites && !self.favorites.contains(&clip.path) {
            return false;
        }
        if let Some(collection) = &self.filter_collection
            && clip.path.parent() != Some(collection.as_path())
        {
            return false;
        }
        self.filter_time.keeps(clock::local(clip.modified), today)
            && self.filter_type.keeps(clip)
            && self.filter_size.keeps(clip.size_bytes)
    }

    pub fn clip_day_groups(&self, indices: &[usize], today: Civil) -> Vec<ClipGroup> {
        let mut groups: Vec<ClipGroup> = Vec::new();
        let mut current_day = None;
        for index in indices {
            let Some(clip) = self.clips.get(*index) else {
                continue;
            };
            let civil = clock::local(clip.modified);
            let day = civil.day_index();
            if current_day != Some(day) {
                groups.push(ClipGroup {
                    label: clock::day_label(civil, today),
                    indices: Vec::new(),
                });
                current_day = Some(day);
            }
            if let Some(group) = groups.last_mut() {
                group.indices.push(*index);
            }
        }
        groups
    }

    pub fn set_clip_tab(&mut self, tab: ClipTab) {
        if self.clip_tab == tab {
            return;
        }
        self.clip_tab = tab;
        self.library_scroll = 0.0;
        self.clear_clip_selection();
    }

    pub fn filters_are_active(&self) -> bool {
        self.filter_time != TimeFilter::All
            || self.filter_type != TypeFilter::All
            || self.filter_size != SizeFilter::All
            || self.filter_collection.is_some()
            || self.clips_oldest_first
    }

    pub fn reset_filters(&mut self) {
        self.filter_time = TimeFilter::All;
        self.filter_type = TypeFilter::All;
        self.filter_size = SizeFilter::All;
        self.filter_collection = None;
        self.clips_oldest_first = false;
        self.library_scroll = 0.0;
    }

    pub fn filter_collection_label(&self) -> &str {
        self.filter_collection
            .as_ref()
            .and_then(|path| {
                self.collections
                    .iter()
                    .find(|collection| &collection.path == path)
            })
            .map_or("Alle", |collection| collection.name.as_str())
    }

    pub const fn sort_label(&self) -> &'static str {
        if self.clips_oldest_first {
            "Älteste zuerst"
        } else {
            "Neueste zuerst"
        }
    }

    pub fn is_favorite(&self, index: usize) -> bool {
        self.clips
            .get(index)
            .is_some_and(|clip| self.favorites.contains(&clip.path))
    }

    pub fn toggle_favorite(&mut self, index: usize) -> Result<(), String> {
        let Some(path) = self.clips.get(index).map(|clip| clip.path.clone()) else {
            return Err("Clip is no longer available".into());
        };
        self.favorites.toggle(&path);
        self.favorites.save().map_err(|error| error.to_string())
    }

    pub fn scroll_library_by(&mut self, delta: f32, overflow: f32) -> bool {
        let target = (self.library_scroll + delta).clamp(0.0, overflow.max(0.0));
        if (target - self.library_scroll).abs() < 0.5 {
            return false;
        }
        self.library_scroll = target;
        true
    }

    pub fn clamp_library_scroll(&mut self, overflow: f32) {
        self.library_scroll = self.library_scroll.clamp(0.0, overflow.max(0.0));
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

    pub fn quality_options(&self) -> Vec<QualityOption> {
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
        let mut values = QUALITY_PRESETS
            .iter()
            .map(|(value, _)| *value)
            .collect::<Vec<_>>();
        values.push(self.config.capture.quality.min(100));
        values.sort_unstable();
        values.dedup();
        values
            .into_iter()
            .map(|quality| {
                let mut spec = wreath_core::replay::ReplaySpec::from_config(&self.config, &monitor);
                spec.quality = quality;
                let video_bytes = spec.estimated_buffer_bytes();
                let audio_bytes = if spec.desktop_audio || spec.microphone_audio {
                    24_000_u64.saturating_mul(u64::from(seconds))
                } else {
                    0
                };
                let encoded_bytes = video_bytes.saturating_add(audio_bytes);
                let container_allowance = encoded_bytes.div_ceil(50);
                let megabytes = encoded_bytes
                    .saturating_add(container_allowance)
                    .div_ceil(1_048_576);
                QualityOption {
                    value: quality,
                    label: quality_label(quality),
                    megabytes,
                    seconds,
                }
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
            previous_page: Page::Library,
            settings_section: SettingsSection::Display,
            settings_menu: None,
            search: TextInput::new(String::new(), PROMPT_MAX_CHARACTERS),
            search_focused: false,
            collection_cards_page: 0,
            collection_clips_page: 0,
            clips_oldest_first: false,
            library_grid: true,
            clip_tab: ClipTab::All,
            filter_panel_open: true,
            filter_time: TimeFilter::All,
            filter_collection: None,
            filter_type: TypeFilter::All,
            filter_size: SizeFilter::All,
            library_scroll: 0.0,
            favorites: Favorites::load(
                std::env::temp_dir().join("wreath-model-test-favorites.json"),
                PathBuf::from("/clips"),
            ),
            daemon: DaemonSnapshot::default(),
            microphone_level: 0,
            collections_descending: false,
            collections_grid: true,
            context_menu: None,
            active_collection: None,
            active_clip: None,
            selection_mode: false,
            selected_clips: HashSet::new(),
            collection_picker_open: false,
            clip_drag_preview: None,
            notice: None,
            autostart_enabled: false,
            hotkey_capture: false,
            hotkey_modifiers: Vec::new(),
            hotkey_pending: false,
            hotkey_deferred: false,
            hotkey_error: None,
            displays: Vec::new(),
            microphone_names: Vec::new(),
            output_names: Vec::new(),
            player_ready: false,
            player_playing: false,
            player_position_seconds: 0.0,
            player_duration_seconds: 0.0,
            player_aspect_ratio: 16.0 / 9.0,
            player_video_width: 0,
            player_video_height: 0,
            player_volume_percent: 100,
            player_last_audible_percent: 100,
            pending_delete: None,
            prompt: None,
            editor_timing: None,
            editor_source: None,
            editor_start: Duration::ZERO,
            editor_end: Duration::ZERO,
            editor_undo: Vec::new(),
            editor_redo: Vec::new(),
            editor_loading: false,
            editor_working: false,
            trim_replace_original: false,
        }
    }

    #[test]
    fn search_is_case_insensitive_and_bounded() {
        let mut model = model();
        model.search.value = "RANKED".into();
        assert_eq!(model.visible_clip_indices(200), vec![0]);
        model.search.clear();
        assert_eq!(model.visible_clip_indices(1), vec![0]);
    }

    #[test]
    fn library_always_shows_collection_clips_while_collections_filter_them() {
        let mut model = model();
        model.clips.push(Clip {
            path: PathBuf::from("/clips/Favorites/Collected.mp4"),
            title: "Collected".into(),
            size_bytes: 20,
            modified: SystemTime::UNIX_EPOCH,
        });
        model.active_collection = Some(PathBuf::from("/clips/Favorites"));

        model.page = Page::Library;
        assert_eq!(model.visible_clip_indices(10), vec![0, 1, 2]);

        model.page = Page::Collections;
        assert_eq!(model.visible_clip_indices(10), vec![2]);

        model.active_collection = None;
        assert_eq!(model.visible_clip_indices(10), vec![0, 1, 2]);
    }

    #[test]
    fn clip_selection_tracks_paths_and_can_be_cancelled_as_one_mode() {
        let mut model = model();
        model.toggle_selection_mode();
        assert!(model.selection_mode);
        assert!(model.toggle_clip_selection(1));
        assert!(model.clip_is_selected(1));
        assert_eq!(model.selected_clip_indices(), vec![1]);

        model.toggle_selection_mode();
        assert!(!model.selection_mode);
        assert!(model.selected_clips.is_empty());
        assert!(!model.collection_picker_open);
    }

    #[test]
    fn select_all_uses_the_current_visible_filter() {
        let mut model = model();
        model.search.value = "ranked".into();
        model.select_all_visible_clips();

        assert!(model.selection_mode);
        assert_eq!(model.selected_clip_indices(), vec![0]);
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
    fn player_moves_through_the_visible_library_order_without_wrapping() {
        let mut model = model();
        model.open_clip(0);

        assert_eq!(model.adjacent_clip(-1), None);
        assert_eq!(model.adjacent_clip(1), Some(1));
        assert!(model.select_adjacent_clip(1));
        assert_eq!(model.active_clip().unwrap().title, "Other");
        assert!(!model.select_adjacent_clip(1));
    }

    #[test]
    fn player_navigation_respects_the_current_search() {
        let mut model = model();
        model.search.value = "ranked".into();
        model.open_clip(0);

        assert_eq!(model.adjacent_clip(-1), None);
        assert_eq!(model.adjacent_clip(1), None);
    }

    #[test]
    fn reopening_a_clip_clears_stale_timeline_state() {
        let mut model = model();
        model.player_ready = true;
        model.player_playing = true;
        model.player_position_seconds = 12.0;
        model.player_duration_seconds = 30.0;

        model.reset_player_state();

        assert!(!model.player_ready);
        assert!(!model.player_playing);
        assert_eq!(model.player_position_seconds, 0.0);
        assert_eq!(model.player_duration_seconds, 0.0);
    }

    #[test]
    fn playback_volume_accepts_a_vertical_slider_percentage() {
        let mut model = model();
        model.set_player_volume(60);
        assert_eq!(model.player_volume_percent, 60);
        model.set_player_volume(200);
        assert_eq!(model.player_volume_percent, 100);
    }

    #[test]
    fn mute_restores_the_last_audible_playback_level() {
        let mut model = model();
        model.set_player_volume(64);
        model.toggle_player_mute();
        assert_eq!(model.player_volume_percent, 0);
        model.toggle_player_mute();
        assert_eq!(model.player_volume_percent, 64);
    }

    #[test]
    fn notices_expire_exactly_after_their_lifetime() {
        let mut expiry = NoticeExpiry::default();
        let now = Instant::now();
        let mut notice = Some("Clip saved".to_owned());

        assert!(expiry.tick(&mut notice, now, Duration::from_secs(3)));
        assert!(!expiry.tick(
            &mut notice,
            now + Duration::from_millis(2_999),
            Duration::from_secs(3)
        ));
        assert!(notice.is_some());
        assert!(expiry.tick(
            &mut notice,
            now + Duration::from_secs(3),
            Duration::from_secs(3)
        ));
        assert!(notice.is_none());
    }

    #[test]
    fn reloading_editor_after_replacing_original_keeps_back_destination() {
        let mut model = model();
        model.open_clip(1);
        assert!(model.edit_active_clip());
        assert_eq!(model.page, Page::Editor);
        assert_eq!(model.previous_page, Page::Library);

        assert!(model.edit_active_clip());
        assert_eq!(model.previous_page, Page::Library);

        model.navigate(model.previous_page);
        assert_eq!(model.page, Page::Library);
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
            short_label: "Display 1".into(),
            refresh_rate: 144.0,
            width: 2560,
            height: 1440,
        });
        model.config.capture.monitor = Some("DISPLAY1".into());
        model.config.capture.frames_per_second = 50;

        assert_eq!(model.frame_rate_options(), vec![30, 48, 50, 60]);
    }

    #[test]
    fn quality_choices_are_named_and_carry_their_clip_size() {
        let mut model = model();
        model.displays.push(DisplayOption {
            name: "DISPLAY1".into(),
            label: "DISPLAY1 · 2560×1440 · 60 Hz".into(),
            short_label: "Display 1".into(),
            refresh_rate: 60.0,
            width: 2560,
            height: 1440,
        });
        model.config.capture.monitor = Some("DISPLAY1".into());
        model.config.capture.frames_per_second = 60;
        model.config.capture.duration_seconds = 30;
        model.config.capture.quality = 75;

        let options = model.quality_options();
        let option = options
            .iter()
            .find(|option| option.value == 75)
            .expect("the configured quality is always offered");

        assert_eq!(option.value, 75);
        assert_eq!(option.label, "High");
        assert_eq!(option.megabytes, 98);
        assert_eq!(option.seconds, 30);

        let cheaper = options
            .iter()
            .find(|option| option.value == 50)
            .expect("50 is offered");
        assert_eq!(cheaper.label, "Low");
        assert_eq!(cheaper.megabytes, 79);
    }

    #[test]
    fn a_quality_outside_the_steps_keeps_its_percentage() {
        let mut model = model();
        model.config.capture.quality = 62;

        let options = model.quality_options();
        let labels = options
            .iter()
            .map(|option| option.label.as_str())
            .collect::<Vec<_>>();

        assert!(options.iter().any(|option| option.value == 62));
        assert!(labels.contains(&"62%"));
        assert_eq!(options.len(), QUALITY_PRESETS.len() + 1);
    }

    #[test]
    fn settings_menu_keyboard_navigation_wraps_at_both_ends() {
        let items = ["Low", "Medium", "High"]
            .into_iter()
            .map(|label| SettingsMenuItem {
                label: label.into(),
                detail: None,
            })
            .collect();
        let mut menu = SettingsMenu::new(SettingsMenuKind::Quality, items, Some(0));

        menu.move_highlight(-1);
        assert_eq!(menu.highlighted, 2);
        menu.move_highlight(1);
        assert_eq!(menu.highlighted, 0);
    }

    #[test]
    fn a_slower_monitor_still_caps_the_frame_rate_choices() {
        let mut model = model();
        model.displays.push(DisplayOption {
            name: "DISPLAY1".into(),
            label: "DISPLAY1 · 1920×1080 · 30 Hz".into(),
            short_label: "Display 1".into(),
            refresh_rate: 30.0,
            width: 1920,
            height: 1080,
        });
        model.config.capture.monitor = Some("DISPLAY1".into());
        model.config.capture.frames_per_second = 30;

        assert_eq!(model.frame_rate_options(), vec![30]);
    }

    fn prompt(value: &str) -> TextInput {
        TextInput::new(value.to_owned(), PROMPT_MAX_CHARACTERS)
    }

    #[test]
    fn a_rename_starts_with_the_whole_name_selected() {
        let mut prompt = prompt("Old name");
        prompt.select_all();

        assert_eq!(prompt.selection(), (0, 8));
        assert!(prompt.has_selection());
    }

    #[test]
    fn typing_over_a_selection_replaces_it() {
        let mut prompt = prompt("Old name");
        prompt.select_all();

        prompt.insert('N');

        assert_eq!(prompt.value, "N");
        assert_eq!(prompt.caret, 1);
        assert!(!prompt.has_selection());
    }

    #[test]
    fn backspace_clears_a_selection_before_it_removes_characters() {
        let mut prompt = prompt("Old name");
        prompt.select_all();

        prompt.backspace();

        assert_eq!(prompt.value, "");
        assert_eq!(prompt.caret, 0);

        let mut prompt = prompt_at("Clip", 4);
        prompt.backspace();
        assert_eq!(prompt.value, "Cli");
    }

    #[test]
    fn a_partial_selection_is_replaced_in_place() {
        let mut prompt = prompt("abcdef");
        prompt.move_caret(1, false);
        prompt.move_caret(4, true);

        assert_eq!(prompt.selection(), (1, 4));
        prompt.insert('X');

        assert_eq!(prompt.value, "aXef");
        assert_eq!(prompt.caret, 2);
    }

    #[test]
    fn a_backwards_selection_deletes_the_same_range() {
        let mut prompt = prompt("abcdef");
        prompt.move_caret(4, false);
        prompt.move_caret(1, true);

        assert_eq!(prompt.selection(), (1, 4));
        prompt.delete();

        assert_eq!(prompt.value, "aef");
        assert_eq!(prompt.caret, 1);
    }

    #[test]
    fn arrows_collapse_a_selection_to_its_edges() {
        let mut prompt = prompt("abcdef");
        prompt.select_all();
        prompt.caret_left(false);
        assert_eq!(prompt.caret, 0);

        prompt.select_all();
        prompt.caret_right(false);
        assert_eq!(prompt.caret, 6);
        assert!(!prompt.has_selection());
    }

    #[test]
    fn home_and_end_extend_the_selection_when_asked() {
        let mut prompt = prompt_at("abcdef", 3);

        prompt.caret_end(true);
        assert_eq!(prompt.selection(), (3, 6));

        prompt.caret_home(true);
        assert_eq!(prompt.selection(), (0, 3));

        prompt.caret_home(false);
        assert!(!prompt.has_selection());
    }

    #[test]
    fn forward_delete_removes_the_character_after_the_caret() {
        let mut prompt = prompt_at("abc", 1);

        prompt.delete();

        assert_eq!(prompt.value, "ac");
        assert_eq!(prompt.caret, 1);
    }

    #[test]
    fn editing_stays_on_character_boundaries_for_wide_names() {
        let mut prompt = prompt("Grüße 🎬");
        prompt.select_all();
        assert_eq!(prompt.selection(), (0, 7));

        prompt.caret_home(false);
        prompt.caret_right(false);
        prompt.caret_right(true);
        prompt.insert('x');

        assert_eq!(prompt.value, "Gxüße 🎬");

        prompt.caret_end(false);
        prompt.backspace();
        assert_eq!(prompt.value, "Gxüße ");
    }

    #[test]
    fn the_length_limit_still_holds_but_replacing_a_selection_is_allowed() {
        let mut prompt = prompt(&"a".repeat(PROMPT_MAX_CHARACTERS));

        prompt.caret_end(false);
        prompt.insert('b');
        assert_eq!(prompt.characters(), PROMPT_MAX_CHARACTERS);

        prompt.select_all();
        prompt.insert('b');
        assert_eq!(prompt.value, "b");
    }

    #[test]
    fn pasted_text_replaces_the_selection_and_drops_control_characters() {
        let mut input = prompt("old value");
        input.select_all();

        input.insert_text("new\r\nvalue");

        assert_eq!(input.value, "newvalue");
        assert_eq!(input.caret, 8);
        assert!(!input.has_selection());
    }

    fn prompt_at(value: &str, caret: usize) -> TextInput {
        let mut prompt = TextInput::new(value.to_owned(), PROMPT_MAX_CHARACTERS);
        prompt.move_caret(caret, false);
        prompt
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

    #[test]
    fn editor_trim_history_undoes_and_redoes_a_drag_as_one_step() {
        let mut model = model();
        model.apply_editor_timing(wreath_core::trim::ClipTiming {
            duration: Duration::from_secs(10),
            keyframes: Vec::new(),
        });
        let original = (model.editor_start, model.editor_end);
        model.set_editor_start(200);
        model.set_editor_end(800);
        model.commit_editor_trim_change(original);

        assert!(model.can_undo_editor_trim());
        assert!(model.undo_editor_trim());
        assert_eq!((model.editor_start, model.editor_end), original);
        assert!(model.can_redo_editor_trim());
        assert!(model.redo_editor_trim());
        assert_eq!(model.editor_start, Duration::from_secs(2));
        assert_eq!(model.editor_end, Duration::from_secs(8));
    }

    fn library_model() -> UiModel {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(1_755_527_700);
        let mut model = model();
        model.clips = vec![
            Clip {
                path: PathBuf::from("/clips/Night Drive.mp4"),
                title: "Night Drive".into(),
                size_bytes: 12 * 1_048_576,
                modified: base,
            },
            Clip {
                path: PathBuf::from("/clips/Mountain Clutch (cut).mp4"),
                title: "Mountain Clutch (cut)".into(),
                size_bytes: 140 * 1_048_576,
                modified: base - Duration::from_secs(86_400),
            },
            Clip {
                path: PathBuf::from("/clips/Valorant/ACE.mp4"),
                title: "ACE".into(),
                size_bytes: 60 * 1_048_576,
                modified: base - Duration::from_secs(10 * 86_400),
            },
        ];
        model.collections = vec![Collection {
            path: PathBuf::from("/clips/Valorant"),
            name: "Valorant".into(),
            clip_count: 1,
        }];
        model
    }

    fn today_of(model: &UiModel) -> Civil {
        clock::local(model.clips[0].modified)
    }

    #[test]
    fn the_favourites_tab_lists_only_starred_clips() {
        let mut model = library_model();
        let today = today_of(&model);
        model.favorites = Favorites::load(
            std::env::temp_dir().join("wreath-model-favourites-tab.json"),
            PathBuf::from("/clips"),
        );
        model.favorites.toggle(&model.clips[1].path.clone());

        model.set_clip_tab(ClipTab::Favorites);
        assert_eq!(model.visible_clip_indices_at(usize::MAX, today), vec![1]);
        assert!(model.is_favorite(1));

        model.set_clip_tab(ClipTab::All);
        assert_eq!(
            model.visible_clip_indices_at(usize::MAX, today),
            vec![0, 1, 2]
        );
    }

    #[test]
    fn each_filter_narrows_the_library_on_its_own_axis() {
        let mut model = library_model();
        let today = today_of(&model);

        model.filter_time = TimeFilter::Today;
        assert_eq!(model.visible_clip_indices_at(usize::MAX, today), vec![0]);

        model.filter_time = TimeFilter::Week;
        assert_eq!(model.visible_clip_indices_at(usize::MAX, today), vec![0, 1]);

        model.filter_time = TimeFilter::All;
        model.filter_type = TypeFilter::Cut;
        assert_eq!(model.visible_clip_indices_at(usize::MAX, today), vec![1]);

        model.filter_type = TypeFilter::Replay;
        assert_eq!(model.visible_clip_indices_at(usize::MAX, today), vec![0, 2]);

        model.filter_type = TypeFilter::All;
        model.filter_size = SizeFilter::Small;
        assert_eq!(model.visible_clip_indices_at(usize::MAX, today), vec![0]);

        model.filter_size = SizeFilter::Large;
        assert_eq!(model.visible_clip_indices_at(usize::MAX, today), vec![1]);

        model.filter_size = SizeFilter::All;
        model.filter_collection = Some(PathBuf::from("/clips/Valorant"));
        assert_eq!(model.visible_clip_indices_at(usize::MAX, today), vec![2]);
        assert_eq!(model.filter_collection_label(), "Valorant");
    }

    #[test]
    fn resetting_the_filters_restores_the_whole_library() {
        let mut model = library_model();
        let today = today_of(&model);
        model.filter_time = TimeFilter::Today;
        model.filter_size = SizeFilter::Large;
        model.clips_oldest_first = true;
        assert!(model.filters_are_active());

        model.reset_filters();

        assert!(!model.filters_are_active());
        assert_eq!(model.filter_collection_label(), "Alle");
        assert_eq!(
            model.visible_clip_indices_at(usize::MAX, today),
            vec![0, 1, 2]
        );
    }

    #[test]
    fn the_library_is_grouped_by_local_day_in_view_order() {
        let mut model = library_model();
        let today = today_of(&model);

        let indices = model.visible_clip_indices_at(usize::MAX, today);
        let groups = model.clip_day_groups(&indices, today);
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].label, "Heute");
        assert_eq!(groups[0].indices, vec![0]);
        assert_eq!(groups[1].label, "Gestern");
        assert_eq!(groups[1].indices, vec![1]);
        assert_ne!(groups[2].label, "Gestern");

        model.clips_oldest_first = true;
        let reversed = model.visible_clip_indices_at(usize::MAX, today);
        let groups = model.clip_day_groups(&reversed, today);
        assert_eq!(groups[0].indices, vec![2]);
        assert_eq!(groups[2].label, "Heute");
    }

    #[test]
    fn clips_of_the_same_day_share_one_section() {
        let mut model = library_model();
        let today = today_of(&model);
        model.clips.insert(
            1,
            Clip {
                path: PathBuf::from("/clips/Sunset.mp4"),
                title: "Sunset".into(),
                size_bytes: 8 * 1_048_576,
                modified: model.clips[0].modified - Duration::from_secs(600),
            },
        );

        let indices = model.visible_clip_indices_at(usize::MAX, today);
        let groups = model.clip_day_groups(&indices, today);

        assert_eq!(groups[0].label, "Heute");
        assert_eq!(groups[0].indices, vec![0, 1]);
    }

    #[test]
    fn clip_filters_stay_out_of_the_collections_page() {
        let mut model = library_model();
        let today = today_of(&model);
        model.filter_size = SizeFilter::Small;
        model.page = Page::Collections;
        model.active_collection = Some(PathBuf::from("/clips/Valorant"));

        assert_eq!(model.visible_clip_indices_at(usize::MAX, today), vec![2]);
    }

    #[test]
    fn library_scrolling_is_clamped_to_the_overflow() {
        let mut model = library_model();

        assert!(model.scroll_library_by(120.0, 300.0));
        assert_eq!(model.library_scroll, 120.0);
        assert!(model.scroll_library_by(400.0, 300.0));
        assert_eq!(model.library_scroll, 300.0);
        assert!(!model.scroll_library_by(50.0, 300.0));
        assert!(model.scroll_library_by(-1_000.0, 300.0));
        assert_eq!(model.library_scroll, 0.0);

        model.library_scroll = 250.0;
        model.clamp_library_scroll(0.0);
        assert_eq!(model.library_scroll, 0.0);
    }

    #[test]
    fn the_recorder_state_drives_both_replay_labels() {
        let mut model = library_model();
        assert_eq!(model.daemon.toolbar_headline(), "RECORDER OFFLINE");

        model.daemon.state = Some(DaemonState::Recording);
        model.daemon.buffered_seconds = 30;
        assert!(model.daemon.is_recording());
        assert_eq!(model.daemon.toolbar_headline(), "REPLAY AKTIV");
        assert_eq!(model.daemon.status_headline(), "REPLAY LÄUFT");

        model.daemon.state = Some(DaemonState::Paused);
        assert!(!model.daemon.is_recording());
        assert_eq!(model.daemon.status_headline(), "REPLAY PAUSIERT");
    }
}
