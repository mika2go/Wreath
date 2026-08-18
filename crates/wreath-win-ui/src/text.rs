use wreath_core::config::Language;

/// Every visible string of the Windows interface, in one place per language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Strings {
    pub language: Language,

    // shell
    pub clips: &'static str,
    pub collections: &'static str,
    pub settings: &'static str,
    pub open_folder: &'static str,

    // recording toolbar
    pub clip_length_label: &'static str,
    pub display_label: &'static str,
    pub quality_label: &'static str,
    pub audio_label: &'static str,
    pub save_clip: &'static str,
    pub replay_active: &'static str,
    pub replay_starting: &'static str,
    pub replay_paused: &'static str,
    pub replay_error: &'static str,
    pub recorder_offline: &'static str,
    pub replay_running: &'static str,
    pub automatic: &'static str,
    pub audio_system_and_microphone: &'static str,
    pub audio_system: &'static str,
    pub audio_microphone: &'static str,
    pub audio_none: &'static str,

    // status bar
    pub storage_caps: &'static str,
    pub hotkey_caps: &'static str,
    pub microphone_caps: &'static str,
    pub microphone_test: &'static str,
    pub microphone_off: &'static str,
    pub no_signal: &'static str,

    // clips page
    pub tab_all_clips: &'static str,
    pub tab_favorites: &'static str,
    pub search_clips: &'static str,
    pub filter_caps: &'static str,
    pub reset: &'static str,
    pub filter_time: &'static str,
    pub filter_game: &'static str,
    pub filter_type: &'static str,
    pub filter_size: &'static str,
    pub filter_sort: &'static str,
    pub empty_no_clips: &'static str,
    pub empty_no_match: &'static str,
    pub empty_no_favorites: &'static str,
    pub empty_no_filter_match: &'static str,
    pub select_all: &'static str,
    pub cancel: &'static str,

    // filter values
    pub all: &'static str,
    pub all_time: &'static str,
    pub today: &'static str,
    pub yesterday: &'static str,
    pub this_week: &'static str,
    pub this_month: &'static str,
    pub type_replays: &'static str,
    pub type_cuts: &'static str,
    pub size_small: &'static str,
    pub size_medium: &'static str,
    pub size_large: &'static str,
    pub sort_newest: &'static str,
    pub sort_oldest: &'static str,

    // clip context menu
    pub clip_actions: &'static str,
    pub favorite_add: &'static str,
    pub favorite_remove: &'static str,
    pub open_in_explorer: &'static str,
    pub edit_clip: &'static str,
    pub rename: &'static str,
    pub select_multiple: &'static str,
    pub move_to_collection: &'static str,
    pub delete_clip: &'static str,

    // collections page
    pub collections_subtitle: &'static str,
    pub new_collection_button: &'static str,
    pub search_collections: &'static str,
    pub folders_caps: &'static str,
    pub all_clips: &'static str,
    pub delete: &'static str,
    pub empty_collection: &'static str,
    pub no_collections: &'static str,
    pub sort_ascending: &'static str,
    pub sort_descending: &'static str,
    pub move_selected_to: &'static str,

    // settings page
    pub settings_subtitle: &'static str,
    pub save: &'static str,
    pub panel_general: &'static str,
    pub panel_capture: &'static str,
    pub panel_audio: &'static str,
    pub panel_storage: &'static str,
    pub autostart: &'static str,
    pub autostart_hint: &'static str,
    pub replay_hotkey: &'static str,
    pub replay_hotkey_hint: &'static str,
    pub theme_row: &'static str,
    pub theme_hint: &'static str,
    pub hover_row: &'static str,
    pub hover_hint: &'static str,
    pub hover_strength_row: &'static str,
    pub hover_strength_hint: &'static str,
    pub language_row: &'static str,
    pub language_hint: &'static str,
    pub display_row: &'static str,
    pub display_hint: &'static str,
    pub clip_duration: &'static str,
    pub clip_duration_hint: &'static str,
    pub frame_rate: &'static str,
    pub frame_rate_hint: &'static str,
    pub video_quality: &'static str,
    pub video_quality_hint: &'static str,
    pub codec: &'static str,
    pub codec_hint: &'static str,
    pub capture_cursor: &'static str,
    pub capture_cursor_hint: &'static str,
    pub system_audio: &'static str,
    pub system_audio_hint: &'static str,
    pub output_device: &'static str,
    pub output_device_hint: &'static str,
    pub system_level: &'static str,
    pub system_level_hint: &'static str,
    pub microphone: &'static str,
    pub microphone_hint: &'static str,
    pub microphone_device: &'static str,
    pub microphone_device_hint: &'static str,
    pub microphone_level: &'static str,
    pub microphone_level_hint: &'static str,
    pub storage_location: &'static str,
    pub storage_location_hint: &'static str,
    pub storage_limit: &'static str,
    pub storage_limit_hint: &'static str,
    pub on: &'static str,
    pub off: &'static str,
    pub windows_default: &'static str,
    pub primary_display: &'static str,
    pub hotkey_activating: &'static str,
    pub hotkey_prompt: &'static str,

    // appearance values
    pub theme_dark: &'static str,
    pub theme_light: &'static str,
    pub theme_cafe: &'static str,
    pub hover_surface: &'static str,
    pub hover_outline: &'static str,
    pub hover_both: &'static str,
    pub strength_off: &'static str,
    pub strength_subtle: &'static str,
    pub strength_normal: &'static str,
    pub strength_strong: &'static str,
    pub language_system: &'static str,
    pub language_german: &'static str,
    pub language_english: &'static str,

    // player and editor
    pub preview_title: &'static str,
    pub preview_subtitle: &'static str,
    pub back: &'static str,
    pub clip_information: &'static str,
    pub field_title: &'static str,
    pub field_created: &'static str,
    pub field_duration: &'static str,
    pub field_size: &'static str,
    pub field_resolution: &'static str,
    pub loading: &'static str,
    pub editor_title: &'static str,
    pub editor_subtitle: &'static str,
    pub trimmed_duration: &'static str,
    pub save_as_new: &'static str,
    pub replace_original: &'static str,
    pub discard: &'static str,
    pub saving: &'static str,
    pub original_size_hint: &'static str,
    pub back_to_preview: &'static str,
    pub clip_unavailable: &'static str,
    pub new_clip: &'static str,
    pub prompt_hint: &'static str,
    pub codec_auto: &'static str,

    // notification area
    pub tray_open_app: &'static str,
    pub tray_save_replay: &'static str,
    pub tray_pause: &'static str,
    pub tray_resume: &'static str,
    pub tray_open_clips: &'static str,
    pub tray_open_config: &'static str,
    pub tray_reload_config: &'static str,
    pub tray_autostart_enable: &'static str,
    pub tray_autostart_disable: &'static str,
    pub tray_exit: &'static str,
    pub tray_starting_up: &'static str,
    pub tray_state_starting: &'static str,
    pub tray_state_recording: &'static str,
    pub tray_state_paused: &'static str,
    pub tray_state_error: &'static str,
    pub tray_error_title: &'static str,
    pub tray_recovering: &'static str,
    pub tray_unavailable: &'static str,

    // modals
    pub delete_clip_question: &'static str,
    pub delete_collection_question: &'static str,
    pub this_clip: &'static str,
    pub this_collection: &'static str,
    pub rename_clip_title: &'static str,
    pub rename_collection_title: &'static str,
    pub new_collection_title: &'static str,
    pub clip_name_label: &'static str,
    pub collection_name_label: &'static str,
    pub create: &'static str,

    // notices
    pub notice_settings_saved: &'static str,
    pub notice_setting_applied: &'static str,
    pub notice_appearance_failed: &'static str,
    pub notice_library_refreshed: &'static str,
    pub notice_clip_renamed: &'static str,
    pub notice_clip_deleted: &'static str,
    pub notice_collection_created: &'static str,
    pub notice_collection_renamed: &'static str,
    pub notice_collection_deleted: &'static str,
    pub notice_clip_gone: &'static str,
    pub notice_collection_gone: &'static str,
    pub notice_no_clip_loaded: &'static str,
    pub notice_already_in_collection: &'static str,
    pub notice_microphone_fallback: &'static str,
    pub notice_microphone_test: &'static str,
    pub notice_cannot_rename_clip: &'static str,
    pub notice_cannot_delete_clip: &'static str,
    pub notice_cannot_create_collection: &'static str,
    pub notice_cannot_rename_collection: &'static str,
    pub notice_cannot_delete_collection: &'static str,
    pub notice_cannot_save_settings: &'static str,
    pub notice_cannot_move_clips: &'static str,
    pub notice_cannot_play: &'static str,
    pub notice_render_failed: &'static str,
    pub notice_folder_picker_failed: &'static str,
    pub notice_shortcut_failed: &'static str,
    pub notice_shortcut_unsafe: &'static str,
    pub notice_cut_running: &'static str,
    pub notice_replace_running: &'static str,
    pub notice_cut_lossless: &'static str,
    pub notice_cut_reencoded: &'static str,
    pub notice_replaced_lossless: &'static str,
    pub notice_replaced_reencoded: &'static str,
    pub notice_cannot_cut: &'static str,
    pub notice_cannot_open_editor: &'static str,
    pub notice_player_unavailable: &'static str,

    // units
    pub seconds_word: &'static str,
    pub clip_singular: &'static str,
    pub clip_plural: &'static str,
    pub months: [&'static str; 12],
}

impl Strings {
    pub fn seconds(&self, value: u16) -> String {
        format!("{value} {}", self.seconds_word)
    }

    pub fn saves_last_seconds(&self, value: u16) -> String {
        match self.language {
            Language::English => format!("Keeps the last {value} seconds"),
            _ => format!("Speichert die letzten {value} Sekunden"),
        }
    }

    pub fn buffered_seconds(&self, value: u16) -> String {
        match self.language {
            Language::English => format!("{value} seconds buffered"),
            _ => format!("{value} Sekunden Puffer"),
        }
    }

    pub fn buffer_short(&self, value: u16) -> String {
        match self.language {
            Language::English => format!("{value} s buffer"),
            _ => format!("{value} s Puffer"),
        }
    }

    pub fn clip_count(&self, count: usize) -> String {
        if count == 1 {
            format!("1 {}", self.clip_singular)
        } else {
            format!("{count} {}", self.clip_plural)
        }
    }

    pub fn selected_count(&self, count: usize) -> String {
        match self.language {
            Language::English => format!("{count} selected"),
            _ => format!("{count} ausgewählt"),
        }
    }

    pub fn move_button(&self, count: usize) -> String {
        match self.language {
            Language::English => format!("MOVE ({count})"),
            _ => format!("VERSCHIEBEN ({count})"),
        }
    }

    pub fn move_drag(&self, count: usize) -> String {
        match (self.language, count) {
            (Language::English, 1) => "Move 1 clip".to_owned(),
            (Language::English, _) => format!("Move {count} clips"),
            (_, 1) => "1 Clip verschieben".to_owned(),
            (_, _) => format!("{count} Clips verschieben"),
        }
    }

    pub fn frames_per_second(&self, value: u16) -> String {
        format!("{value} fps")
    }

    pub fn resolution_line(&self, height: u32, fps: u16) -> String {
        format!("{height}p · {fps} FPS")
    }

    pub fn version_line(&self, version: &str) -> String {
        match self.language {
            Language::English => {
                format!("wreath {version} · local, no account and no upload")
            }
            _ => format!("wreath {version} · lokal, ohne Konto und ohne Upload"),
        }
    }

    pub fn moved_clips(&self, count: usize, collection: &str) -> String {
        match (self.language, count) {
            (Language::English, 1) => format!("Clip moved to {collection}"),
            (Language::English, _) => format!("{count} clips moved to {collection}"),
            (_, 1) => format!("Clip nach {collection} verschoben"),
            (_, _) => format!("{count} Clips nach {collection} verschoben"),
        }
    }
}

pub const GERMAN: Strings = Strings {
    language: Language::German,
    clips: "Clips",
    collections: "Collections",
    settings: "Einstellungen",
    open_folder: "Ordner öffnen",

    clip_length_label: "CLIP-LÄNGE",
    display_label: "BILDSCHIRM",
    quality_label: "QUALITÄT",
    audio_label: "AUDIO",
    save_clip: "CLIP SPEICHERN",
    replay_active: "REPLAY AKTIV",
    replay_starting: "REPLAY STARTET",
    replay_paused: "REPLAY PAUSIERT",
    replay_error: "REPLAY GESTÖRT",
    recorder_offline: "RECORDER OFFLINE",
    replay_running: "REPLAY LÄUFT",
    automatic: "Automatisch",
    audio_system_and_microphone: "System + Mikrofon",
    audio_system: "Systemaudio",
    audio_microphone: "Mikrofon",
    audio_none: "Kein Audio",

    storage_caps: "SPEICHER",
    hotkey_caps: "HOTKEY",
    microphone_caps: "MIKROFON",
    microphone_test: "Testen",
    microphone_off: "Mikrofon aus",
    no_signal: "Kein Signal",

    tab_all_clips: "Alle Clips",
    tab_favorites: "Favoriten",
    search_clips: "Clips suchen...",
    filter_caps: "FILTER",
    reset: "Zurücksetzen",
    filter_time: "Zeitraum",
    filter_game: "Spiel",
    filter_type: "Typ",
    filter_size: "Größe",
    filter_sort: "Sortierung",
    empty_no_clips: "Noch keine Clips",
    empty_no_match: "Keine passenden Clips",
    empty_no_favorites: "Noch keine Favoriten",
    empty_no_filter_match: "Keine Clips in dieser Auswahl",
    select_all: "Alle auswählen",
    cancel: "Abbrechen",

    all: "Alle",
    all_time: "Alle Zeit",
    today: "Heute",
    yesterday: "Gestern",
    this_week: "Diese Woche",
    this_month: "Dieser Monat",
    type_replays: "Replays",
    type_cuts: "Zuschnitte",
    size_small: "Bis 25 MB",
    size_medium: "25 bis 100 MB",
    size_large: "Über 100 MB",
    sort_newest: "Neueste zuerst",
    sort_oldest: "Älteste zuerst",

    clip_actions: "Clip-Aktionen",
    favorite_add: "Als Favorit merken",
    favorite_remove: "Favorit entfernen",
    open_in_explorer: "Im Explorer öffnen",
    edit_clip: "Clip bearbeiten",
    rename: "Umbenennen",
    select_multiple: "Mehrere auswählen",
    move_to_collection: "In Sammlung verschieben",
    delete_clip: "Clip löschen",

    collections_subtitle: "Ordne deine Clips in lokalen Ordnern.",
    new_collection_button: "NEUE SAMMLUNG",
    search_collections: "Sammlungen suchen...",
    folders_caps: "ORDNER",
    all_clips: "Alle Clips",
    delete: "Löschen",
    empty_collection: "Diese Sammlung ist leer",
    no_collections: "Noch keine Sammlungen",
    sort_ascending: "A–Z",
    sort_descending: "Z–A",
    move_selected_to: "Ausgewählte Clips verschieben nach",

    settings_subtitle: "Passe wreath nach deinen Wünschen an.",
    save: "Speichern",
    panel_general: "Allgemein",
    panel_capture: "Aufnahmen",
    panel_audio: "Audio",
    panel_storage: "Speicher",
    autostart: "Programmstart",
    autostart_hint: "wreath automatisch mit Windows starten",
    replay_hotkey: "Replay-Hotkey",
    replay_hotkey_hint: "Speichert den aktuellen Replay",
    theme_row: "Design",
    theme_hint: "Farbstimmung der Oberfläche",
    hover_row: "Hover-Effekt",
    hover_hint: "Fläche, Kontur oder beides",
    hover_strength_row: "Hover-Stärke",
    hover_strength_hint: "Wie deutlich Elemente reagieren",
    language_row: "Sprache",
    language_hint: "Sprache der Oberfläche",
    display_row: "Bildschirm",
    display_hint: "Aufnahmequelle",
    clip_duration: "Clip-Dauer",
    clip_duration_hint: "Länge des Replay-Fensters",
    frame_rate: "Bildrate",
    frame_rate_hint: "Maximal 60 Bilder pro Sekunde",
    video_quality: "Videoqualität",
    video_quality_hint: "Detailgrad und Speicherbedarf",
    codec: "Codec",
    codec_hint: "Hardware-Encoder",
    capture_cursor: "Mauszeiger aufnehmen",
    capture_cursor_hint: "Zeiger in Aufnahmen anzeigen",
    system_audio: "Systemaudio",
    system_audio_hint: "Spiel- und Desktop-Ton aufnehmen",
    output_device: "Ausgabegerät",
    output_device_hint: "Quelle für Systemaudio",
    system_level: "Systemaudio-Pegel",
    system_level_hint: "Balance der Desktop-Aufnahme",
    microphone: "Mikrofon",
    microphone_hint: "Eingabegerät mit aufnehmen",
    microphone_device: "Mikrofon-Gerät",
    microphone_device_hint: "Aktives Windows-Eingabegerät",
    microphone_level: "Mikrofon-Pegel",
    microphone_level_hint: "Lautstärke der Stimme",
    storage_location: "Speicherort",
    storage_location_hint: "Lokaler Ordner für Clips",
    storage_limit: "Speicherlimit",
    storage_limit_hint: "Maximaler Platz für Clips",
    on: "An",
    off: "Aus",
    windows_default: "Windows-Standard",
    primary_display: "Primärer Bildschirm",
    hotkey_activating: "Aktivieren...",
    hotkey_prompt: "Tastenkombination drücken…",

    theme_dark: "Dunkel",
    theme_light: "Hell",
    theme_cafe: "Café",
    hover_surface: "Fläche",
    hover_outline: "Kontur",
    hover_both: "Fläche und Kontur",
    strength_off: "Aus",
    strength_subtle: "Zart",
    strength_normal: "Normal",
    strength_strong: "Deutlich",
    language_system: "System",
    language_german: "Deutsch",
    language_english: "English",

    preview_title: "Clip-Preview",
    preview_subtitle: "Schau dir deinen Clip an und prüfe die besten Momente.",
    back: "Zurück",
    clip_information: "Clip-Informationen",
    field_title: "Titel",
    field_created: "Erstellt",
    field_duration: "Dauer (Original)",
    field_size: "Größe (Original)",
    field_resolution: "Auflösung",
    loading: "Wird geladen",
    editor_title: "Clip bearbeiten",
    editor_subtitle: "Schneide deinen Clip und speichere nur die besten Momente.",
    trimmed_duration: "Geschnittene Dauer",
    save_as_new: "Speichern als",
    replace_original: "Original ersetzen",
    discard: "Verwerfen",
    saving: "Speichert…",
    original_size_hint: "Originalgröße     ESC",
    back_to_preview: "Zurück zur Preview",
    clip_unavailable: "Clip nicht verfügbar",
    new_clip: "Neuer Clip",
    prompt_hint: "Strg+A alles wählen · Strg+C/X/V · Enter bestätigen · Esc abbrechen",
    codec_auto: "Auto (empfohlen)",

    tray_open_app: "wreath öffnen",
    tray_save_replay: "Replay speichern",
    tray_pause: "Aufnahme pausieren",
    tray_resume: "Aufnahme fortsetzen",
    tray_open_clips: "Clips öffnen",
    tray_open_config: "Konfigurationsdatei öffnen",
    tray_reload_config: "Einstellungen neu laden",
    tray_autostart_enable: "Mit Windows starten",
    tray_autostart_disable: "Nicht mit Windows starten",
    tray_exit: "wreath beenden",
    tray_starting_up: "wreath startet",
    tray_state_starting: "Startet",
    tray_state_recording: "Nimmt auf",
    tray_state_paused: "Pausiert",
    tray_state_error: "Fehler",
    tray_error_title: "wreath-Fehler",
    tray_recovering: "wreath — Aufnahme wird neu gestartet",
    tray_unavailable: "wreath — Recorder nicht erreichbar",

    delete_clip_question: "Clip löschen?",
    delete_collection_question: "Sammlung löschen?",
    this_clip: "diesen Clip",
    this_collection: "diese Sammlung",
    rename_clip_title: "Clip umbenennen",
    rename_collection_title: "Sammlung umbenennen",
    new_collection_title: "Neue Sammlung",
    clip_name_label: "Clip-Name",
    collection_name_label: "Name der Sammlung",
    create: "Erstellen",

    notice_settings_saved: "Einstellungen gespeichert und Aufnahme neu geladen",
    notice_setting_applied: "Einstellung gespeichert und Aufnahme neu geladen",
    notice_appearance_failed: "Darstellung nicht gespeichert",
    notice_library_refreshed: "Bibliothek aktualisiert",
    notice_clip_renamed: "Clip umbenannt",
    notice_clip_deleted: "Clip gelöscht",
    notice_collection_created: "Sammlung erstellt",
    notice_collection_renamed: "Sammlung umbenannt",
    notice_collection_deleted: "Sammlung gelöscht; Clips liegen wieder in der Bibliothek",
    notice_clip_gone: "Clip ist nicht mehr verfügbar",
    notice_collection_gone: "Sammlung ist nicht mehr verfügbar",
    notice_no_clip_loaded: "Kein Clip geladen",
    notice_already_in_collection: "Die gewählten Clips liegen schon in dieser Sammlung",
    notice_microphone_fallback: "Gespeichertes Mikrofon nicht verfügbar; Windows-Standard wird getestet",
    notice_microphone_test: "Mikrofontest",
    notice_cannot_rename_clip: "Clip konnte nicht umbenannt werden",
    notice_cannot_delete_clip: "Clip konnte nicht gelöscht werden",
    notice_cannot_create_collection: "Sammlung konnte nicht erstellt werden",
    notice_cannot_rename_collection: "Sammlung konnte nicht umbenannt werden",
    notice_cannot_delete_collection: "Sammlung konnte nicht gelöscht werden",
    notice_cannot_save_settings: "Einstellungen konnten nicht gespeichert werden",
    notice_cannot_move_clips: "Clips konnten nicht verschoben werden",
    notice_cannot_play: "Dieser Clip lässt sich nicht abspielen",
    notice_render_failed: "Darstellung fehlgeschlagen",
    notice_folder_picker_failed: "Ordnerauswahl fehlgeschlagen",
    notice_shortcut_failed: "Tastenkombination konnte nicht geändert werden",
    notice_shortcut_unsafe: "Wähle eine andere Tastenkombination",
    notice_cut_running: "Zuschnitt läuft im Hintergrund…",
    notice_replace_running: "Original wird im Hintergrund ersetzt…",
    notice_cut_lossless: "Verlustfrei geschnitten",
    notice_cut_reencoded: "Für den exakten Start neu kodiert",
    notice_replaced_lossless: "Original verlustfrei ersetzt",
    notice_replaced_reencoded: "Original ersetzt und für den exakten Start neu kodiert",
    notice_cannot_cut: "Clip konnte nicht geschnitten werden",
    notice_cannot_open_editor: "Editor konnte nicht geöffnet werden",
    notice_player_unavailable: "Wiedergabe nicht verfügbar",

    seconds_word: "Sekunden",
    clip_singular: "Clip",
    clip_plural: "Clips",
    months: [
        "Januar",
        "Februar",
        "März",
        "April",
        "Mai",
        "Juni",
        "Juli",
        "August",
        "September",
        "Oktober",
        "November",
        "Dezember",
    ],
};

pub const ENGLISH: Strings = Strings {
    language: Language::English,
    clips: "Clips",
    collections: "Collections",
    settings: "Settings",
    open_folder: "Open folder",

    clip_length_label: "CLIP LENGTH",
    display_label: "DISPLAY",
    quality_label: "QUALITY",
    audio_label: "AUDIO",
    save_clip: "SAVE CLIP",
    replay_active: "REPLAY ON",
    replay_starting: "REPLAY STARTING",
    replay_paused: "REPLAY PAUSED",
    replay_error: "REPLAY STOPPED",
    recorder_offline: "RECORDER OFFLINE",
    replay_running: "REPLAY RUNNING",
    automatic: "Automatic",
    audio_system_and_microphone: "System + mic",
    audio_system: "System audio",
    audio_microphone: "Microphone",
    audio_none: "No audio",

    storage_caps: "STORAGE",
    hotkey_caps: "HOTKEY",
    microphone_caps: "MICROPHONE",
    microphone_test: "Test",
    microphone_off: "Microphone off",
    no_signal: "No signal",

    tab_all_clips: "All clips",
    tab_favorites: "Favourites",
    search_clips: "Search clips...",
    filter_caps: "FILTER",
    reset: "Reset",
    filter_time: "Time range",
    filter_game: "Game",
    filter_type: "Type",
    filter_size: "Size",
    filter_sort: "Sort",
    empty_no_clips: "No clips yet",
    empty_no_match: "No matching clips",
    empty_no_favorites: "No favourites yet",
    empty_no_filter_match: "No clips in this selection",
    select_all: "Select all",
    cancel: "Cancel",

    all: "All",
    all_time: "All time",
    today: "Today",
    yesterday: "Yesterday",
    this_week: "This week",
    this_month: "This month",
    type_replays: "Replays",
    type_cuts: "Cuts",
    size_small: "Up to 25 MB",
    size_medium: "25 to 100 MB",
    size_large: "Over 100 MB",
    sort_newest: "Newest first",
    sort_oldest: "Oldest first",

    clip_actions: "Clip actions",
    favorite_add: "Add to favourites",
    favorite_remove: "Remove from favourites",
    open_in_explorer: "Show in Explorer",
    edit_clip: "Trim clip",
    rename: "Rename",
    select_multiple: "Select multiple",
    move_to_collection: "Move to collection",
    delete_clip: "Delete clip",

    collections_subtitle: "Keep your clips in local folders.",
    new_collection_button: "NEW COLLECTION",
    search_collections: "Search collections...",
    folders_caps: "FOLDERS",
    all_clips: "All clips",
    delete: "Delete",
    empty_collection: "This collection is empty",
    no_collections: "No collections yet",
    sort_ascending: "A–Z",
    sort_descending: "Z–A",
    move_selected_to: "Move selected clips to",

    settings_subtitle: "Set wreath up the way you work.",
    save: "Save",
    panel_general: "General",
    panel_capture: "Recording",
    panel_audio: "Audio",
    panel_storage: "Storage",
    autostart: "Start with Windows",
    autostart_hint: "Launch wreath when you sign in",
    replay_hotkey: "Replay shortcut",
    replay_hotkey_hint: "Saves the current replay",
    theme_row: "Theme",
    theme_hint: "Colour mood of the interface",
    hover_row: "Hover effect",
    hover_hint: "Surface, outline or both",
    hover_strength_row: "Hover strength",
    hover_strength_hint: "How strongly elements respond",
    language_row: "Language",
    language_hint: "Language of the interface",
    display_row: "Display",
    display_hint: "Recording source",
    clip_duration: "Clip length",
    clip_duration_hint: "Length of the replay window",
    frame_rate: "Frame rate",
    frame_rate_hint: "Up to 60 frames per second",
    video_quality: "Video quality",
    video_quality_hint: "Detail and disk space",
    codec: "Codec",
    codec_hint: "Hardware encoder",
    capture_cursor: "Record the pointer",
    capture_cursor_hint: "Show the cursor in recordings",
    system_audio: "System audio",
    system_audio_hint: "Record game and desktop sound",
    output_device: "Output device",
    output_device_hint: "Source for system audio",
    system_level: "System level",
    system_level_hint: "Balance of the desktop recording",
    microphone: "Microphone",
    microphone_hint: "Record your input device",
    microphone_device: "Microphone device",
    microphone_device_hint: "Active Windows input device",
    microphone_level: "Microphone level",
    microphone_level_hint: "Loudness of your voice",
    storage_location: "Clip folder",
    storage_location_hint: "Local folder for clips",
    storage_limit: "Storage limit",
    storage_limit_hint: "Maximum space for clips",
    on: "On",
    off: "Off",
    windows_default: "Windows default",
    primary_display: "Primary display",
    hotkey_activating: "Activating...",
    hotkey_prompt: "Press a shortcut…",

    theme_dark: "Dark",
    theme_light: "Light",
    theme_cafe: "Café",
    hover_surface: "Surface",
    hover_outline: "Outline",
    hover_both: "Surface and outline",
    strength_off: "Off",
    strength_subtle: "Subtle",
    strength_normal: "Normal",
    strength_strong: "Strong",
    language_system: "System",
    language_german: "Deutsch",
    language_english: "English",

    preview_title: "Clip preview",
    preview_subtitle: "Watch your clip and find the best moment.",
    back: "Back",
    clip_information: "Clip details",
    field_title: "Title",
    field_created: "Created",
    field_duration: "Length (original)",
    field_size: "Size (original)",
    field_resolution: "Resolution",
    loading: "Loading",
    editor_title: "Trim clip",
    editor_subtitle: "Cut your clip down to the best moment.",
    trimmed_duration: "Trimmed length",
    save_as_new: "Save as new",
    replace_original: "Replace original",
    discard: "Discard",
    saving: "Saving…",
    original_size_hint: "Original size     ESC",
    back_to_preview: "Back to preview",
    clip_unavailable: "Clip unavailable",
    new_clip: "New clip",
    prompt_hint: "Ctrl+A select all · Ctrl+C/X/V · Enter to confirm · Esc to cancel",
    codec_auto: "Auto (recommended)",

    tray_open_app: "Open wreath",
    tray_save_replay: "Save replay",
    tray_pause: "Pause recording",
    tray_resume: "Resume recording",
    tray_open_clips: "Open clips",
    tray_open_config: "Open configuration file",
    tray_reload_config: "Reload settings",
    tray_autostart_enable: "Start with Windows",
    tray_autostart_disable: "Do not start with Windows",
    tray_exit: "Exit wreath",
    tray_starting_up: "wreath is starting",
    tray_state_starting: "Starting",
    tray_state_recording: "Recording",
    tray_state_paused: "Paused",
    tray_state_error: "Error",
    tray_error_title: "wreath error",
    tray_recovering: "wreath — restarting capture",
    tray_unavailable: "wreath — recorder unavailable",

    delete_clip_question: "Delete clip?",
    delete_collection_question: "Delete collection?",
    this_clip: "this clip",
    this_collection: "this collection",
    rename_clip_title: "Rename clip",
    rename_collection_title: "Rename collection",
    new_collection_title: "New collection",
    clip_name_label: "Clip name",
    collection_name_label: "Collection name",
    create: "Create",

    notice_settings_saved: "Settings saved and recording reloaded",
    notice_setting_applied: "Setting saved and recording reloaded",
    notice_appearance_failed: "Appearance not saved",
    notice_library_refreshed: "Library refreshed",
    notice_clip_renamed: "Clip renamed",
    notice_clip_deleted: "Clip deleted",
    notice_collection_created: "Collection created",
    notice_collection_renamed: "Collection renamed",
    notice_collection_deleted: "Collection deleted; clips are back in the library",
    notice_clip_gone: "Clip is no longer available",
    notice_collection_gone: "Collection is no longer available",
    notice_no_clip_loaded: "No clip is loaded",
    notice_already_in_collection: "The selected clips are already in this collection",
    notice_microphone_fallback: "Saved microphone unavailable; testing the Windows default",
    notice_microphone_test: "Microphone test",
    notice_cannot_rename_clip: "Cannot rename clip",
    notice_cannot_delete_clip: "Cannot delete clip",
    notice_cannot_create_collection: "Cannot create collection",
    notice_cannot_rename_collection: "Cannot rename collection",
    notice_cannot_delete_collection: "Cannot delete collection",
    notice_cannot_save_settings: "Cannot save settings",
    notice_cannot_move_clips: "Cannot move clips",
    notice_cannot_play: "Cannot play this clip",
    notice_render_failed: "Rendering failed",
    notice_folder_picker_failed: "Folder picker failed",
    notice_shortcut_failed: "Cannot change the shortcut",
    notice_shortcut_unsafe: "Choose a different shortcut",
    notice_cut_running: "Cutting on a background worker…",
    notice_replace_running: "Replacing the original on a background worker…",
    notice_cut_lossless: "Cut without re-encoding",
    notice_cut_reencoded: "Re-encoded for an exact start",
    notice_replaced_lossless: "Original replaced without re-encoding",
    notice_replaced_reencoded: "Original replaced and re-encoded for an exact start",
    notice_cannot_cut: "Cannot cut clip",
    notice_cannot_open_editor: "Cannot open the editor",
    notice_player_unavailable: "Playback unavailable",

    seconds_word: "seconds",
    clip_singular: "clip",
    clip_plural: "clips",
    months: [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ],
};

pub const fn strings(language: Language) -> &'static Strings {
    match language {
        Language::English => &ENGLISH,
        _ => &GERMAN,
    }
}

/// Resolves `System` to the Windows display language.
pub fn resolve(language: Language) -> Language {
    match language {
        Language::System => system_language(),
        chosen => chosen,
    }
}

#[cfg(target_os = "windows")]
fn system_language() -> Language {
    use windows::Win32::Globalization::GetUserDefaultUILanguage;

    // 0x07 is the primary language id of German in every regional variant
    let identifier = unsafe { GetUserDefaultUILanguage() };
    if identifier & 0x3ff == 0x07 {
        Language::German
    } else {
        Language::English
    }
}

#[cfg(not(target_os = "windows"))]
fn system_language() -> Language {
    Language::English
}

#[cfg(test)]
mod tests {
    use super::{ENGLISH, GERMAN, Language, resolve, strings};

    #[test]
    fn both_catalogues_are_complete_and_distinct() {
        assert_eq!(strings(Language::German).language, Language::German);
        assert_eq!(strings(Language::English).language, Language::English);
        assert_ne!(GERMAN.save_clip, ENGLISH.save_clip);
        assert_ne!(GERMAN.settings, ENGLISH.settings);
        assert_ne!(GERMAN.months[0], ENGLISH.months[0]);
    }

    #[test]
    fn counted_phrases_follow_the_language_and_the_number() {
        assert_eq!(GERMAN.clip_count(1), "1 Clip");
        assert_eq!(GERMAN.clip_count(12), "12 Clips");
        assert_eq!(ENGLISH.clip_count(1), "1 clip");
        assert_eq!(ENGLISH.clip_count(12), "12 clips");
        assert_eq!(GERMAN.seconds(30), "30 Sekunden");
        assert_eq!(ENGLISH.seconds(30), "30 seconds");
        assert_eq!(ENGLISH.move_drag(1), "Move 1 clip");
        assert_eq!(GERMAN.move_drag(3), "3 Clips verschieben");
        assert_eq!(ENGLISH.moved_clips(1, "Valorant"), "Clip moved to Valorant");
    }

    #[test]
    fn an_explicit_language_is_never_overridden_by_the_system() {
        assert_eq!(resolve(Language::German), Language::German);
        assert_eq!(resolve(Language::English), Language::English);
        assert!(matches!(
            resolve(Language::System),
            Language::German | Language::English
        ));
    }
}
