use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::time::Duration;

use gdk_pixbuf::PixbufLoader;
use gdk_pixbuf::prelude::PixbufLoaderExt;
use gtk::glib;
use gtk::prelude::*;
use gtk::{
    Align, Box as GtkBox, Button, CheckButton, ContentFit, DropDown, Entry, EventControllerFocus,
    EventControllerKey, FileDialog, GestureClick, Grid, Label, Orientation, Overlay, Picture,
    ScrolledWindow, SignalListItemFactory, Stack, StringList, StringObject,
};
use wreath_core::audio::{self, DesktopOutput, Microphone};
use wreath_core::config::{Codec, Config, HotkeyConfig, MAX_FRAMES_PER_SECOND};
use wreath_core::display::{self, Monitor};
use wreath_core::paths::AppPaths;
use wreath_core::replay::ReplaySpec;
use wreath_core::shortcuts;

#[derive(Clone)]
pub struct SettingsView {
    pub page: ScrolledWindow,
    rows: Vec<SettingsRow>,
    grids: Vec<Grid>,
    footer: GtkBox,
    feedback: Label,
    apply: Button,
    panels: Vec<GtkBox>,
    audio_columns: GtkBox,
    sticker: GtkBox,
}

#[derive(Clone)]
struct SettingsRow {
    container: GtkBox,
    copy: GtkBox,
    control: gtk::Widget,
    audio_column: bool,
}

const QUALITY_PRESETS: [(u8, &str); 5] = [
    (50, "Low"),
    (65, "Medium"),
    (75, "High"),
    (85, "Ultra"),
    (100, "Insane"),
];
const STORAGE_LIMITS: [(u32, &str); 6] = [
    (1_024, "1 GB"),
    (5_120, "5 GB"),
    (10_240, "10 GB"),
    (25_600, "25 GB"),
    (51_200, "50 GB"),
    (102_400, "100 GB"),
];
const DURATION_PRESETS: [u16; 6] = [15, 30, 45, 60, 90, 120];
const DESKTOP_GAIN_PRESETS: [u16; 9] = [0, 25, 50, 75, 100, 125, 150, 175, 200];
const MICROPHONE_GAIN_PRESETS: [u16; 4] = [25, 50, 75, 100];
const SETTINGS_STICKER_PNG: &[u8] = include_bytes!("../../../assets/wreath-settings-67.png");

#[derive(Clone)]
struct HotkeyCapture {
    entry: Entry,
    confirmed: Rc<RefCell<HotkeyConfig>>,
}

impl HotkeyCapture {
    fn new(hotkey: &HotkeyConfig) -> Self {
        let entry = Entry::new();
        entry.set_text(&hotkey.to_string());
        entry.set_editable(false);
        entry.set_hexpand(true);
        entry.set_tooltip_text(Some("Click, then press the new shortcut"));

        let confirmed = Rc::new(RefCell::new(hotkey.clone()));
        let recording = Rc::new(Cell::new(false));

        let focus_controller = EventControllerFocus::new();
        let focus_entry = entry.clone();
        let enter_recording = recording.clone();
        focus_controller.connect_enter(move |_| {
            begin_hotkey_capture(&focus_entry, &enter_recording);
        });

        let leave_entry = entry.clone();
        let leave_confirmed = confirmed.clone();
        let focus_recording = recording.clone();
        focus_controller.connect_leave(move |_| {
            restore_system_shortcuts(&leave_entry);
            if focus_recording.replace(false) {
                leave_entry.remove_css_class("recording");
                leave_entry.set_text(&leave_confirmed.borrow().to_string());
                leave_entry.set_tooltip_text(Some("Click to record a different shortcut"));
            }
        });
        entry.add_controller(focus_controller);

        let click_controller = GestureClick::new();
        let click_entry = entry.clone();
        let click_recording = recording.clone();
        click_controller.connect_pressed(move |_, _, _, _| {
            begin_hotkey_capture(&click_entry, &click_recording);
        });
        entry.add_controller(click_controller);

        let controller = EventControllerKey::new();
        let key_entry = entry.clone();
        let key_confirmed = confirmed.clone();
        let key_recording = recording.clone();
        controller.connect_key_pressed(move |_, key, _, modifiers| {
            if !key_recording.get() {
                return glib::Propagation::Proceed;
            }

            match hotkey_capture_action(key, modifiers) {
                HotkeyCaptureAction::Cancel => {
                    key_entry.set_text(&key_confirmed.borrow().to_string());
                    key_recording.set(false);
                    key_entry.remove_css_class("recording");
                    key_entry.set_tooltip_text(Some("Click to record a different shortcut"));
                    clear_hotkey_focus(&key_entry);
                }
                HotkeyCaptureAction::Preview(preview) => key_entry.set_text(&preview),
                HotkeyCaptureAction::Complete(hotkey) => {
                    key_entry.set_text(&hotkey.to_string());
                    *key_confirmed.borrow_mut() = hotkey;
                    key_recording.set(false);
                    key_entry.remove_css_class("recording");
                    key_entry.set_tooltip_text(Some(
                        "Shortcut captured. Click to record a different shortcut",
                    ));
                    clear_hotkey_focus(&key_entry);
                }
                HotkeyCaptureAction::Invalid => {
                    key_entry.set_text("Unsupported key · try another shortcut");
                }
            }
            glib::Propagation::Stop
        });
        entry.add_controller(controller);

        Self { entry, confirmed }
    }

    fn value(&self) -> Result<HotkeyConfig, String> {
        Ok(self.confirmed.borrow().clone())
    }
}

fn begin_hotkey_capture(entry: &Entry, recording: &Cell<bool>) {
    recording.set(true);
    inhibit_system_shortcuts(entry);
    entry.set_text("Press shortcut…");
    entry.add_css_class("recording");
    entry.set_tooltip_text(Some(
        "Hold any modifiers, then press one other key. Escape cancels.",
    ));
}

fn clear_hotkey_focus(entry: &Entry) {
    restore_system_shortcuts(entry);
    if let Some(window) = entry
        .root()
        .and_then(|root| root.downcast::<gtk::Window>().ok())
    {
        gtk::prelude::GtkWindowExt::set_focus(&window, None::<&gtk::Widget>);
    }
}

fn inhibit_system_shortcuts(entry: &Entry) {
    if let Some(toplevel) = hotkey_toplevel(entry) {
        toplevel.inhibit_system_shortcuts(None::<gtk::gdk::Event>);
    }
}

fn restore_system_shortcuts(entry: &Entry) {
    if let Some(toplevel) = hotkey_toplevel(entry) {
        toplevel.restore_system_shortcuts();
    }
}

fn hotkey_toplevel(entry: &Entry) -> Option<gtk::gdk::Toplevel> {
    entry
        .native()
        .and_then(|native| native.surface())
        .and_then(|surface| surface.downcast::<gtk::gdk::Toplevel>().ok())
}

fn is_modifier_key(key: gtk::gdk::Key) -> bool {
    matches!(
        key,
        gtk::gdk::Key::Super_L
            | gtk::gdk::Key::Super_R
            | gtk::gdk::Key::Shift_L
            | gtk::gdk::Key::Shift_R
            | gtk::gdk::Key::Control_L
            | gtk::gdk::Key::Control_R
            | gtk::gdk::Key::Alt_L
            | gtk::gdk::Key::Alt_R
            | gtk::gdk::Key::Meta_L
            | gtk::gdk::Key::Meta_R
    )
}

#[derive(Debug, PartialEq, Eq)]
enum HotkeyCaptureAction {
    Cancel,
    Preview(String),
    Complete(HotkeyConfig),
    Invalid,
}

fn hotkey_capture_action(
    key: gtk::gdk::Key,
    modifiers: gtk::gdk::ModifierType,
) -> HotkeyCaptureAction {
    if key == gtk::gdk::Key::Escape {
        return HotkeyCaptureAction::Cancel;
    }
    if is_modifier_key(key) {
        let modifiers = hotkey_modifiers(key, modifiers);
        return HotkeyCaptureAction::Preview(if modifiers.is_empty() {
            "Press shortcut…".to_owned()
        } else {
            format!("{}+…", modifiers.join("+"))
        });
    }
    hotkey_from_key(key, modifiers)
        .map(HotkeyCaptureAction::Complete)
        .unwrap_or(HotkeyCaptureAction::Invalid)
}

fn hotkey_modifiers(key: gtk::gdk::Key, modifiers: gtk::gdk::ModifierType) -> Vec<&'static str> {
    let super_pressed = modifiers.intersects(
        gtk::gdk::ModifierType::SUPER_MASK
            | gtk::gdk::ModifierType::HYPER_MASK
            | gtk::gdk::ModifierType::META_MASK,
    ) || matches!(
        key,
        gtk::gdk::Key::Super_L
            | gtk::gdk::Key::Super_R
            | gtk::gdk::Key::Meta_L
            | gtk::gdk::Key::Meta_R
    );
    let shift_pressed = modifiers.contains(gtk::gdk::ModifierType::SHIFT_MASK)
        || matches!(key, gtk::gdk::Key::Shift_L | gtk::gdk::Key::Shift_R);
    let control_pressed = modifiers.contains(gtk::gdk::ModifierType::CONTROL_MASK)
        || matches!(key, gtk::gdk::Key::Control_L | gtk::gdk::Key::Control_R);
    let alt_pressed = modifiers.contains(gtk::gdk::ModifierType::ALT_MASK)
        || matches!(key, gtk::gdk::Key::Alt_L | gtk::gdk::Key::Alt_R);

    [
        (super_pressed, "SUPER"),
        (shift_pressed, "SHIFT"),
        (control_pressed, "CTRL"),
        (alt_pressed, "ALT"),
    ]
    .into_iter()
    .filter_map(|(pressed, name)| pressed.then_some(name))
    .collect()
}

fn hotkey_from_key(
    key: gtk::gdk::Key,
    modifiers: gtk::gdk::ModifierType,
) -> Result<HotkeyConfig, String> {
    let key_name = key
        .to_upper()
        .name()
        .ok_or_else(|| "key has no XKB name".to_owned())?;
    let expression = hotkey_modifiers(key, modifiers)
        .iter()
        .copied()
        .chain(std::iter::once(key_name.as_str()))
        .collect::<Vec<_>>()
        .join("+");
    HotkeyConfig::parse(&expression).map_err(|error| error.to_string())
}

impl SettingsView {
    pub fn set_layout(&self, window_width: i32, window_height: i32) {
        for grid in &self.grids {
            grid.set_row_spacing(12);
        }
        for row in &self.rows {
            row.container.set_orientation(Orientation::Horizontal);
            row.container.set_spacing(18);
            row.copy.set_size_request(-1, -1);
            row.control
                .set_size_request(settings_control_width(window_width, row.audio_column), 42);
            row.control.set_halign(Align::End);
        }
        self.footer.set_orientation(Orientation::Horizontal);
        self.footer.set_spacing(18);
        self.feedback.set_wrap(false);
        self.apply.set_halign(Align::End);
        self.apply.set_size_request(132, 42);
        for panel in &self.panels {
            panel.set_size_request(-1, -1);
            panel.set_hexpand(true);
        }
        self.audio_columns.set_orientation(Orientation::Horizontal);
        self.audio_columns.set_spacing(12);
        let (sticker_width, sticker_height) = settings_sticker_size(window_width, window_height);
        self.sticker
            .set_visible(sticker_width > 0 && sticker_height > 0);
        self.sticker.set_size_request(sticker_width, sticker_height);
        self.sticker
            .set_margin_end(settings_page_padding(window_width).round() as i32);
        self.sticker.set_margin_bottom(16);
    }
}

pub fn build() -> SettingsView {
    let paths = AppPaths::discover();
    let config = Config::load(&paths).unwrap_or_default();
    let monitors = display::monitors().unwrap_or_default();
    let desktop_outputs = configured_desktop_outputs(
        audio::desktop_outputs().unwrap_or_default(),
        config.audio.desktop_device.as_deref(),
    );
    let microphones = configured_microphones(
        audio::microphones().unwrap_or_default(),
        config.audio.microphone_device.as_deref(),
    );
    let initial_autostart = autostart_enabled().unwrap_or(false);

    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("settings-page");

    let title = Label::new(Some("Settings"));
    title.add_css_class("page-title");
    title.set_halign(Align::Start);
    let subtitle = Label::new(Some("Tune capture without leaving Wreath"));
    subtitle.add_css_class("page-subtitle");
    subtitle.set_halign(Align::Start);
    subtitle.set_wrap(true);
    title.set_margin_top(10);
    root.append(&subtitle);
    title.set_margin_bottom(24);
    root.append(&title);

    let tabs = GtkBox::new(Orientation::Horizontal, 4);
    tabs.add_css_class("settings-tabs");
    tabs.set_halign(Align::Start);
    tabs.set_hexpand(true);
    let settings_stack = Stack::new();
    settings_stack.add_css_class("settings-stack");
    settings_stack.set_hhomogeneous(false);
    settings_stack.set_vhomogeneous(false);
    settings_stack.set_transition_type(gtk::StackTransitionType::None);
    settings_stack.set_transition_duration(0);

    let display_page = settings_panel(
        "Display",
        "Choose the display or desktop portal Wreath records.",
    );
    let quality_page = settings_panel("Quality", "Balance detail, file size and encoder load.");
    let audio_page = settings_panel(
        "Audio",
        "Mix desktop sound and your microphone into each clip.",
    );
    let controls_description = match shortcuts::backend() {
        shortcuts::ShortcutBackend::Hyprland => {
            "Set the shortcut used to save the replay buffer. Hyprland updates immediately."
        }
        shortcuts::ShortcutBackend::Plasma => {
            "Record the shortcut here, then assign wreathctl save in Plasma System Settings."
        }
        shortcuts::ShortcutBackend::Manual(_) => {
            "Record the shortcut here, then assign wreathctl save in your desktop settings."
        }
    };
    let controls_page = settings_panel("Controls", controls_description);
    let storage_page = settings_panel(
        "Storage",
        "Choose where clips and collection folders are kept.",
    );
    let panels = vec![
        display_page.clone(),
        quality_page.clone(),
        audio_page.clone(),
        controls_page.clone(),
        storage_page.clone(),
    ];
    settings_stack.add_named(&display_page, Some("display"));
    settings_stack.add_named(&quality_page, Some("quality"));
    settings_stack.add_named(&audio_page, Some("audio"));
    settings_stack.add_named(&controls_page, Some("controls"));
    settings_stack.add_named(&storage_page, Some("storage"));
    let tab_buttons = [
        settings_tab("Display", "display"),
        settings_tab("Quality", "quality"),
        settings_tab("Audio", "audio"),
        settings_tab("Controls", "controls"),
        settings_tab("Storage", "storage"),
    ];
    tab_buttons[0].0.add_css_class("active");
    tab_buttons[0]
        .0
        .update_state(&[gtk::accessible::State::Selected(Some(true))]);
    for (button, _) in &tab_buttons {
        tabs.append(button);
    }
    for (button, page_name) in &tab_buttons {
        let stack = settings_stack.clone();
        let buttons = tab_buttons
            .iter()
            .map(|(button, _)| button.clone())
            .collect::<Vec<_>>();
        let page_name = *page_name;
        let active = button.clone();
        button.connect_clicked(move |_| {
            stack.set_visible_child_name(page_name);
            for button in &buttons {
                button.remove_css_class("active");
                button.update_state(&[gtk::accessible::State::Selected(Some(false))]);
            }
            active.add_css_class("active");
            active.update_state(&[gtk::accessible::State::Selected(Some(true))]);
        });
    }
    let mut rows = Vec::new();
    let mut grids = Vec::new();
    let display_grid = settings_grid();
    grids.push(display_grid.clone());
    let monitor_model = monitor_model(&monitors);
    let monitor_dropdown = DropDown::new(Some(monitor_model.clone()), None::<gtk::Expression>);
    monitor_dropdown.set_hexpand(true);
    monitor_dropdown.set_selected(selected_monitor_index(&monitors, &config));
    rows.push(attach_row(
        &display_grid,
        0,
        "Capture display",
        "Choose a monitor and use its current Linux refresh rate.",
        &monitor_dropdown,
    ));
    let fps_values = Rc::new(RefCell::new(frame_rate_values(
        &monitors,
        monitor_dropdown.selected(),
        config.capture.frames_per_second,
    )));
    let fps = DropDown::new(
        Some(numeric_model(&fps_values.borrow(), " fps")),
        None::<gtk::Expression>,
    );
    fps.set_selected(selected_value_index(
        &fps_values.borrow(),
        config.capture.frames_per_second,
    ));
    let changed_fps = fps.clone();
    let changed_fps_values = fps_values.clone();
    let changed_monitors = monitors.clone();
    monitor_dropdown.connect_selected_notify(move |dropdown| {
        let current =
            selected_value(&changed_fps_values.borrow(), changed_fps.selected()).unwrap_or(60);
        let native = selected_monitor_refresh_rate(&changed_monitors, dropdown.selected());
        let next = current.min(native);
        let values = frame_rate_values(&changed_monitors, dropdown.selected(), next);
        changed_fps.set_model(Some(&numeric_model(&values, " fps")));
        changed_fps.set_selected(selected_value_index(&values, next));
        *changed_fps_values.borrow_mut() = values;
    });
    rows.push(attach_row(
        &display_grid,
        1,
        "Frame rate",
        "Available rates follow the selected monitor.",
        &fps,
    ));
    let capture_cursor = settings_toggle(config.capture.cursor);
    rows.push(attach_row(
        &display_grid,
        2,
        "Capture cursor",
        "Include the pointer in saved clips.",
        &capture_cursor,
    ));
    display_page.append(&display_grid);

    let capture_grid = settings_grid();
    grids.push(capture_grid.clone());
    let duration_values = preset_values(&DURATION_PRESETS, config.capture.duration_seconds);
    let duration = DropDown::new(
        Some(numeric_model(&duration_values, " seconds")),
        None::<gtk::Expression>,
    );
    duration.set_selected(selected_value_index(
        &duration_values,
        config.capture.duration_seconds,
    ));
    duration.set_tooltip_text(Some("Seconds retained in the encoded replay buffer"));
    rows.push(attach_row(
        &capture_grid,
        0,
        "Clip length",
        "How much encoded video stays in memory.",
        &duration,
    ));
    let codec_model = StringList::new(&["Auto (recommended)", "H.264", "HEVC", "AV1"]);
    let codec = DropDown::new(Some(codec_model), None::<gtk::Expression>);
    codec.set_selected(match config.capture.codec {
        Codec::Auto => 0,
        Codec::H264 => 1,
        Codec::Hevc => 2,
        Codec::Av1 => 3,
    });
    rows.push(attach_row(
        &capture_grid,
        1,
        "Codec",
        "Hardware encoder selection; Auto is recommended.",
        &codec,
    ));
    let quality_values = quality_values(config.capture.quality);
    let quality_model = quality_choice_model(
        &quality_values,
        &config,
        selected_monitor_for_estimate(&monitors, monitor_dropdown.selected()),
    );
    let quality = DropDown::new(Some(quality_model), None::<gtk::Expression>);
    install_quality_factories(&quality);
    quality.set_hexpand(true);
    quality.set_selected(
        quality_values
            .iter()
            .position(|value| *value == config.capture.quality)
            .and_then(|index| u32::try_from(index).ok())
            .unwrap_or(0),
    );
    rows.push(attach_row(
        &capture_grid,
        2,
        "Quality",
        "Balances image detail and replay memory.",
        &quality,
    ));
    quality_page.append(&capture_grid);

    let control_grid = settings_grid();
    grids.push(control_grid.clone());
    let hotkey = HotkeyCapture::new(&config.hotkey);
    rows.push(attach_row(
        &control_grid,
        0,
        "Save replay",
        controls_description,
        &hotkey.entry,
    ));
    let autostart = settings_toggle(initial_autostart);
    rows.push(attach_row(
        &control_grid,
        1,
        "Start with Linux",
        "Start Wreath automatically when your user session begins.",
        &autostart,
    ));
    controls_page.append(&control_grid);

    let audio_columns = GtkBox::new(Orientation::Horizontal, 12);
    audio_columns.set_hexpand(true);
    let game_audio_grid = settings_grid();
    game_audio_grid.set_hexpand(true);
    let microphone_grid = settings_grid();
    microphone_grid.set_hexpand(true);
    grids.push(game_audio_grid.clone());
    grids.push(microphone_grid.clone());
    let desktop_audio = settings_toggle(config.audio.desktop);
    rows.push(attach_audio_row(
        &game_audio_grid,
        0,
        "Game audio",
        "Record game and system sound.",
        &desktop_audio,
    ));
    let desktop_gain_values =
        preset_values(&DESKTOP_GAIN_PRESETS, config.audio.desktop_gain_percent);
    let desktop_gain = DropDown::new(
        Some(numeric_model(&desktop_gain_values, "%")),
        None::<gtk::Expression>,
    );
    desktop_gain.set_selected(selected_value_index(
        &desktop_gain_values,
        config.audio.desktop_gain_percent,
    ));
    desktop_gain.set_hexpand(true);
    desktop_gain.set_sensitive(config.audio.desktop);
    desktop_gain.set_tooltip_text(Some(
        "Only changes desktop loudness in Wreath recordings, never the system volume",
    ));
    rows.push(attach_audio_row(
        &game_audio_grid,
        1,
        "Game audio level",
        "Recording level; Linux system volume stays unchanged.",
        &desktop_gain,
    ));
    let desktop_output_model = desktop_output_model(&desktop_outputs);
    let desktop_output_dropdown =
        DropDown::new(Some(desktop_output_model), None::<gtk::Expression>);
    desktop_output_dropdown.set_hexpand(true);
    desktop_output_dropdown.set_selected(selected_desktop_output_index(&desktop_outputs, &config));
    desktop_output_dropdown.set_sensitive(config.audio.desktop);
    rows.push(attach_audio_row(
        &game_audio_grid,
        2,
        "Output device",
        "Capture this output instead of following the Linux default.",
        &desktop_output_dropdown,
    ));
    let microphone = settings_toggle(config.audio.microphone);
    rows.push(attach_audio_row(
        &microphone_grid,
        0,
        "Microphone",
        "Capture your selected input with its own level.",
        &microphone,
    ));
    let microphone_model = microphone_model(&microphones);
    let microphone_dropdown = DropDown::new(Some(microphone_model), None::<gtk::Expression>);
    microphone_dropdown.set_hexpand(true);
    microphone_dropdown.set_selected(selected_microphone_index(&microphones, &config));
    microphone_dropdown.set_sensitive(config.audio.microphone);
    microphone_dropdown.set_tooltip_text(Some("PipeWire microphone used in new clips"));
    rows.push(attach_audio_row(
        &microphone_grid,
        1,
        "Input device",
        "Use this PipeWire input for new clips.",
        &microphone_dropdown,
    ));
    let microphone_gain_values = preset_values(
        &MICROPHONE_GAIN_PRESETS,
        config.audio.microphone_gain_percent,
    );
    let microphone_gain = DropDown::new(
        Some(numeric_model(&microphone_gain_values, "%")),
        None::<gtk::Expression>,
    );
    microphone_gain.set_selected(selected_value_index(
        &microphone_gain_values,
        config.audio.microphone_gain_percent,
    ));
    microphone_gain.set_hexpand(true);
    microphone_gain.set_sensitive(config.audio.microphone);
    microphone_gain.set_tooltip_text(Some(
        "Only changes microphone loudness in Wreath recordings, never the system microphone volume",
    ));
    rows.push(attach_audio_row(
        &microphone_grid,
        2,
        "Microphone level",
        "Recording level; Linux microphone volume stays unchanged.",
        &microphone_gain,
    ));
    let microphone_toggle_dropdown = microphone_dropdown.clone();
    let microphone_toggle_gain = microphone_gain.clone();
    microphone.connect_toggled(move |toggle| {
        microphone_toggle_dropdown.set_sensitive(toggle.is_active());
        microphone_toggle_gain.set_sensitive(toggle.is_active());
    });
    let desktop_toggle_gain = desktop_gain.clone();
    let desktop_toggle_output = desktop_output_dropdown.clone();
    desktop_audio.connect_toggled(move |toggle| {
        desktop_toggle_gain.set_sensitive(toggle.is_active());
        desktop_toggle_output.set_sensitive(toggle.is_active());
    });

    let refresh_quality: Rc<dyn Fn()> = Rc::new({
        let base_config = config.clone();
        let monitors = monitors.clone();
        let monitor_dropdown = monitor_dropdown.clone();
        let fps_values = fps_values.clone();
        let fps = fps.clone();
        let duration_values = duration_values.clone();
        let duration = duration.clone();
        let codec = codec.clone();
        let quality_values = quality_values.clone();
        let quality = quality.clone();
        let desktop_audio = desktop_audio.clone();
        let microphone = microphone.clone();
        move || {
            let mut preview = base_config.clone();
            preview.capture.frames_per_second =
                selected_value(&fps_values.borrow(), fps.selected()).unwrap_or(60);
            preview.capture.duration_seconds =
                selected_value(&duration_values, duration.selected()).unwrap_or(30);
            preview.capture.codec = codec_from_selected(codec.selected());
            preview.audio.desktop = desktop_audio.is_active();
            preview.audio.microphone = microphone.is_active();
            let selected = quality.selected();
            let monitor = selected_monitor_for_estimate(&monitors, monitor_dropdown.selected());
            quality.set_model(Some(&quality_choice_model(
                &quality_values,
                &preview,
                monitor,
            )));
            quality.set_selected(selected);
        }
    });
    for dropdown in [&monitor_dropdown, &fps, &duration, &codec] {
        let refresh_quality = refresh_quality.clone();
        dropdown.connect_selected_notify(move |_| refresh_quality());
    }
    for toggle in [&desktop_audio, &microphone] {
        let refresh_quality = refresh_quality.clone();
        toggle.connect_toggled(move |_| refresh_quality());
    }
    audio_columns.append(&game_audio_grid);
    audio_columns.append(&microphone_grid);
    audio_page.append(&audio_columns);

    let storage_grid = settings_grid();
    grids.push(storage_grid.clone());
    let output = Entry::new();
    output.set_text(&config.storage.directory.to_string_lossy());
    output.set_hexpand(true);
    let choose_output = Button::with_label("Browse…");
    choose_output.add_css_class("quiet-action");
    let output_control = GtkBox::new(Orientation::Horizontal, 12);
    output_control.set_hexpand(true);
    output_control.append(&output);
    output_control.append(&choose_output);
    let selected_output = output.clone();
    choose_output.connect_clicked(move |button| {
        let current = PathBuf::from(selected_output.text().as_str());
        let mut chooser = FileDialog::builder()
            .title("Choose Wreath save location")
            .accept_label("Choose")
            .modal(true);
        if current.is_absolute() {
            chooser = chooser.initial_folder(&gtk::gio::File::for_path(current));
        }
        let chooser = chooser.build();
        let chosen_entry = selected_output.clone();
        let parent = button
            .root()
            .and_then(|root| root.downcast::<gtk::Window>().ok());
        chooser.select_folder(
            parent.as_ref(),
            None::<&gtk::gio::Cancellable>,
            move |result| {
                if let Ok(file) = result
                    && let Some(path) = file.path()
                {
                    chosen_entry.set_text(&path.to_string_lossy());
                }
            },
        );
    });
    rows.push(attach_row(
        &storage_grid,
        0,
        "Save location",
        "Store clips and collection folders in this local directory.",
        &output_control,
    ));
    let storage_values = storage_values(config.storage.max_megabytes);
    let storage_labels = storage_values
        .iter()
        .map(|value| storage_label(*value))
        .collect::<Vec<_>>();
    let storage_model = StringList::new(
        &storage_labels
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    );
    let storage_limit = DropDown::new(Some(storage_model), None::<gtk::Expression>);
    storage_limit.set_hexpand(true);
    storage_limit.set_selected(
        storage_values
            .iter()
            .position(|value| *value == config.storage.max_megabytes)
            .and_then(|index| u32::try_from(index).ok())
            .unwrap_or(0),
    );
    rows.push(attach_row(
        &storage_grid,
        1,
        "Storage limit",
        "Old clips are never uploaded.",
        &storage_limit,
    ));
    storage_page.append(&storage_grid);

    let footer = GtkBox::new(Orientation::Horizontal, 18);
    footer.set_margin_top(32);
    footer.set_visible(false);
    let feedback = Label::new(None);
    feedback.add_css_class("feedback");
    feedback.set_halign(Align::Start);
    feedback.set_hexpand(true);
    let autostart_feedback = feedback.clone();
    let autostart_footer = footer.clone();
    let autostart_reverting = Rc::new(Cell::new(false));
    let toggled_reverting = autostart_reverting.clone();
    autostart.connect_toggled(move |toggle| {
        if toggled_reverting.get() {
            return;
        }
        let enabled = toggle.is_active();
        autostart_footer.set_visible(true);
        match set_autostart_enabled(enabled) {
            Ok(()) => {
                autostart_feedback.set_text(if enabled {
                    "Wreath will start after sign-in."
                } else {
                    "Wreath will no longer start after sign-in."
                });
                autostart_feedback.remove_css_class("error");
                autostart_feedback.add_css_class("success");
            }
            Err(error) => {
                toggled_reverting.set(true);
                toggle.set_active(!enabled);
                toggled_reverting.set(false);
                autostart_feedback.set_text(&error);
                autostart_feedback.remove_css_class("success");
                autostart_feedback.add_css_class("error");
            }
        }
    });
    let apply_label = Label::new(Some("Save settings"));
    let apply = Button::new();
    apply.add_css_class("settings-save-action");
    apply.update_property(&[gtk::accessible::Property::Label("Save settings")]);
    apply.set_child(Some(&apply_label));
    apply.set_size_request(132, 42);
    footer.append(&feedback);
    let toolbar = GtkBox::new(Orientation::Horizontal, 18);
    toolbar.add_css_class("settings-toolbar");
    toolbar.set_margin_bottom(22);
    toolbar.append(&tabs);
    toolbar.append(&apply);
    root.append(&toolbar);
    root.append(&settings_stack);
    root.append(&footer);
    let sticker = GtkBox::new(Orientation::Vertical, 0);
    sticker.add_css_class("settings-sticker");
    sticker.set_halign(Align::End);
    sticker.set_valign(Align::End);
    sticker.set_size_request(253, 190);
    sticker.set_overflow(gtk::Overflow::Hidden);
    let sticker_picture = embedded_picture(SETTINGS_STICKER_PNG);
    sticker_picture.set_can_shrink(true);
    sticker_picture.set_content_fit(ContentFit::Contain);
    sticker_picture.set_hexpand(true);
    sticker_picture.set_vexpand(true);
    sticker_picture.set_size_request(1, 1);
    sticker.append(&sticker_picture);

    let save_paths = paths.clone();
    let saved_feedback = feedback.clone();
    let saved_footer = footer.clone();
    let saved_apply = apply.clone();
    let saved_apply_label = apply_label.clone();
    let save_generation = Rc::new(Cell::new(0_u64));
    apply.connect_clicked(move |_| {
        let generation = save_generation.get().wrapping_add(1);
        save_generation.set(generation);
        saved_footer.set_visible(true);
        match collect_and_save(
            &save_paths,
            &monitors,
            &monitor_dropdown,
            &fps_values,
            &duration,
            &duration_values,
            &fps,
            &codec,
            &quality_values,
            &quality,
            &capture_cursor,
            &hotkey,
            &desktop_audio,
            &desktop_gain_values,
            &desktop_gain,
            &desktop_outputs,
            &desktop_output_dropdown,
            &microphone,
            &microphones,
            &microphone_dropdown,
            &microphone_gain_values,
            &microphone_gain,
            &output,
            &storage_values,
            &storage_limit,
        ) {
            Ok(()) => {
                saved_feedback.set_text("✓ Changes saved locally. Recorder updated.");
                saved_feedback.remove_css_class("error");
                saved_feedback.add_css_class("success");
                saved_apply_label.set_text("Saved");
                saved_apply.add_css_class("saved");

                let reset_apply = saved_apply.clone();
                let reset_apply_label = saved_apply_label.clone();
                let reset_generation = save_generation.clone();
                glib::timeout_add_local_once(Duration::from_millis(2200), move || {
                    if reset_generation.get() == generation {
                        reset_apply_label.set_text("Save settings");
                        reset_apply.remove_css_class("saved");
                    }
                });

                let reset_feedback = saved_feedback.clone();
                let reset_footer = saved_footer.clone();
                let reset_generation = save_generation.clone();
                glib::timeout_add_local_once(Duration::from_secs(5), move || {
                    if reset_generation.get() == generation {
                        reset_feedback.set_text("");
                        reset_feedback.remove_css_class("success");
                        reset_footer.set_visible(false);
                    }
                });
            }
            Err(error) => {
                saved_feedback.set_text(&error);
                saved_feedback.remove_css_class("success");
                saved_feedback.add_css_class("error");
                saved_apply_label.set_text("Save settings");
                saved_apply.remove_css_class("saved");
            }
        }
    });

    let scroll = ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    let overlay = Overlay::new();
    overlay.set_child(Some(&root));
    overlay.add_overlay(&sticker);
    scroll.set_child(Some(&overlay));
    SettingsView {
        page: scroll,
        rows,
        grids,
        footer,
        feedback,
        apply,
        panels,
        audio_columns,
        sticker,
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_and_save(
    paths: &AppPaths,
    monitors: &[Monitor],
    monitor_dropdown: &DropDown,
    fps_values: &RefCell<Vec<u16>>,
    duration: &DropDown,
    duration_values: &[u16],
    fps: &DropDown,
    codec: &DropDown,
    quality_values: &[u8],
    quality: &DropDown,
    capture_cursor: &CheckButton,
    hotkey: &HotkeyCapture,
    desktop_audio: &CheckButton,
    desktop_gain_values: &[u16],
    desktop_gain: &DropDown,
    desktop_outputs: &[DesktopOutput],
    desktop_output_dropdown: &DropDown,
    microphone: &CheckButton,
    microphones: &[Microphone],
    microphone_dropdown: &DropDown,
    microphone_gain_values: &[u16],
    microphone_gain: &DropDown,
    output: &Entry,
    storage_values: &[u32],
    storage_limit: &DropDown,
) -> Result<(), String> {
    let mut config = Config::load(paths).unwrap_or_default();
    let previous_hotkey = config.hotkey.clone();
    let selected = usize::try_from(monitor_dropdown.selected()).unwrap_or(usize::MAX);
    let monitor = monitors
        .get(selected)
        .ok_or_else(|| "Select an available monitor.".to_owned())?;
    config.capture.monitor = Some(monitor.description.clone());
    config.capture.duration_seconds = selected_value(duration_values, duration.selected())
        .ok_or_else(|| "Select a clip length.".to_owned())?;
    config.capture.frames_per_second = selected_value(&fps_values.borrow(), fps.selected())
        .ok_or_else(|| "Select a frame rate.".to_owned())?;
    config.capture.codec = match codec.selected() {
        0 => Codec::Auto,
        1 => Codec::H264,
        2 => Codec::Hevc,
        3 => Codec::Av1,
        _ => return Err("Select a codec.".into()),
    };
    let quality_index = usize::try_from(quality.selected()).unwrap_or(usize::MAX);
    config.capture.quality = *quality_values
        .get(quality_index)
        .ok_or_else(|| "Select a quality preset.".to_owned())?;
    config.capture.cursor = capture_cursor.is_active();
    config.hotkey = hotkey.value()?;
    config.audio.desktop = desktop_audio.is_active();
    config.audio.desktop_gain_percent =
        selected_value(desktop_gain_values, desktop_gain.selected())
            .ok_or_else(|| "Select a game audio level.".to_owned())?;
    let desktop_output_index =
        usize::try_from(desktop_output_dropdown.selected()).unwrap_or(usize::MAX);
    config.audio.desktop_device = if desktop_output_index == 0 {
        None
    } else {
        desktop_outputs
            .get(desktop_output_index - 1)
            .map(|output| output.name.clone())
    };
    if config.audio.desktop && desktop_output_index != 0 && config.audio.desktop_device.is_none() {
        return Err("Select an available output device.".into());
    }
    config.audio.microphone = microphone.is_active();
    let microphone_index = usize::try_from(microphone_dropdown.selected()).unwrap_or(usize::MAX);
    config.audio.microphone_device = microphone_index
        .checked_sub(1)
        .and_then(|index| microphones.get(index))
        .map(|microphone| microphone.name.clone());
    config.audio.microphone_gain_percent =
        selected_value(microphone_gain_values, microphone_gain.selected())
            .ok_or_else(|| "Select a microphone level.".to_owned())?;
    let output_path = PathBuf::from(output.text().as_str());
    if !output_path.is_absolute() {
        return Err("Save location must be an absolute local path.".into());
    }
    config.storage.directory = output_path;
    let storage_index = usize::try_from(storage_limit.selected()).unwrap_or(usize::MAX);
    config.storage.max_megabytes = *storage_values
        .get(storage_index)
        .ok_or_else(|| "Select a storage limit.".to_owned())?;
    config.validate().map_err(|error| error.to_string())?;
    config.save(paths).map_err(|error| error.to_string())?;

    let control = sibling_control_executable();
    shortcuts::replace(Some(&previous_hotkey), &config.hotkey, &control)
        .map_err(|error| error.to_string())?;
    reload_capture(&control)?;
    Ok(())
}

fn sibling_control_executable() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .map(|directory| directory.join("wreathctl"))
        .filter(|path| path.exists())
        .unwrap_or_else(|| PathBuf::from("wreathctl"))
}

fn reload_capture(control: &Path) -> Result<(), String> {
    let run = || {
        Command::new(control)
            .arg("reload")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .map_err(|error| {
                format!("Settings were saved, but the recorder could not reload: {error}")
            })
    };
    let first = run()?;
    if first.status.success() {
        return Ok(());
    }

    let started = Command::new("systemctl")
        .args(["--user", "start", "wreathd.service"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output();
    if started.as_ref().is_ok_and(|output| output.status.success()) {
        let retried = run()?;
        if retried.status.success() {
            return Ok(());
        }
        return Err(reload_error(&retried.stderr));
    }
    Err(reload_error(&first.stderr))
}

fn reload_error(stderr: &[u8]) -> String {
    let detail = String::from_utf8_lossy(stderr).trim().to_owned();
    if detail.is_empty() {
        "Settings were saved, but the recorder could not reload.".to_owned()
    } else {
        format!("Settings were saved, but the recorder could not reload: {detail}")
    }
}

fn settings_toggle(active: bool) -> CheckButton {
    let toggle = CheckButton::new();
    let state = Label::new(Some(if active { "On" } else { "Off" }));
    state.set_halign(Align::Center);
    state.set_hexpand(true);
    toggle.set_child(Some(&state));
    toggle.set_active(active);
    toggle.set_halign(Align::Fill);
    toggle.set_hexpand(true);
    toggle.connect_toggled(move |toggle| {
        state.set_text(if toggle.is_active() { "On" } else { "Off" });
    });
    toggle
}

fn numeric_model(values: &[u16], suffix: &str) -> StringList {
    let labels = values
        .iter()
        .map(|value| format!("{value}{suffix}"))
        .collect::<Vec<_>>();
    StringList::new(&labels.iter().map(String::as_str).collect::<Vec<_>>())
}

fn preset_values(presets: &[u16], current: u16) -> Vec<u16> {
    let mut values = presets.to_vec();
    if !values.contains(&current) {
        values.push(current);
        values.sort_unstable();
    }
    values
}

fn selected_value(values: &[u16], selected: u32) -> Option<u16> {
    usize::try_from(selected)
        .ok()
        .and_then(|index| values.get(index))
        .copied()
}

fn selected_value_index(values: &[u16], selected: u16) -> u32 {
    values
        .iter()
        .position(|value| *value == selected)
        .and_then(|index| u32::try_from(index).ok())
        .unwrap_or(gtk::INVALID_LIST_POSITION)
}

fn selected_monitor_refresh_rate(monitors: &[Monitor], selected: u32) -> u16 {
    usize::try_from(selected)
        .ok()
        .and_then(|index| monitors.get(index))
        .map_or(60, |monitor| monitor.refresh_rate.round() as u16)
        .clamp(15, MAX_FRAMES_PER_SECOND)
}

fn frame_rate_values(monitors: &[Monitor], selected: u32, current: u16) -> Vec<u16> {
    let native_rate = selected_monitor_refresh_rate(monitors, selected);
    let mut rates = [30, 48, 60]
        .into_iter()
        .filter(|rate| *rate <= native_rate)
        .collect::<Vec<_>>();
    rates.push(native_rate);
    rates.push(current.clamp(15, MAX_FRAMES_PER_SECOND));
    rates.sort_unstable();
    rates.dedup();
    rates
}

fn monitor_model(monitors: &[Monitor]) -> StringList {
    let labels = monitors
        .iter()
        .map(|monitor| {
            if monitor.uses_portal() {
                "Desktop portal · choose a screen securely".to_owned()
            } else {
                format!(
                    "{} · {} × {} · {:.0} Hz",
                    monitor.name, monitor.width, monitor.height, monitor.refresh_rate
                )
            }
        })
        .collect::<Vec<_>>();
    StringList::new(&labels.iter().map(String::as_str).collect::<Vec<&str>>())
}

fn selected_monitor_index(monitors: &[Monitor], config: &Config) -> u32 {
    monitors
        .iter()
        .position(|monitor| {
            config.capture.monitor.as_deref() == Some(monitor.description.as_str())
                || config.capture.monitor.as_deref() == Some(monitor.name.as_str())
        })
        .or_else(|| monitors.iter().position(|monitor| monitor.focused))
        .and_then(|index| u32::try_from(index).ok())
        .unwrap_or(gtk::INVALID_LIST_POSITION)
}

fn configured_desktop_outputs(
    mut outputs: Vec<DesktopOutput>,
    configured: Option<&str>,
) -> Vec<DesktopOutput> {
    if let Some(configured) = configured
        && !outputs.iter().any(|output| output.name == configured)
    {
        outputs.push(DesktopOutput {
            name: configured.to_owned(),
            label: "Configured output · unavailable".to_owned(),
            is_default: false,
        });
    }
    outputs
}

fn configured_microphones(
    mut microphones: Vec<Microphone>,
    configured: Option<&str>,
) -> Vec<Microphone> {
    if let Some(configured) = configured
        && !microphones
            .iter()
            .any(|microphone| microphone.name == configured)
    {
        microphones.push(Microphone {
            name: configured.to_owned(),
            label: "Configured input · unavailable".to_owned(),
            is_default: false,
        });
    }
    microphones
}

fn desktop_output_model(outputs: &[DesktopOutput]) -> StringList {
    let mut labels = vec!["Follow Linux default".to_owned()];
    labels.extend(outputs.iter().map(|output| output.label.clone()));
    StringList::new(&labels.iter().map(String::as_str).collect::<Vec<_>>())
}

fn selected_desktop_output_index(outputs: &[DesktopOutput], config: &Config) -> u32 {
    config
        .audio
        .desktop_device
        .as_deref()
        .and_then(|selected| outputs.iter().position(|output| output.name == selected))
        .and_then(|index| u32::try_from(index + 1).ok())
        .unwrap_or(0)
}

fn quality_values(current: u8) -> Vec<u8> {
    let current = current.min(100);
    let mut values = QUALITY_PRESETS
        .iter()
        .map(|(value, _)| *value)
        .collect::<Vec<_>>();
    if !values.contains(&current) {
        values.push(current);
        values.sort_unstable();
    }
    values
}

fn quality_label(value: u8) -> String {
    QUALITY_PRESETS
        .iter()
        .find(|(preset, _)| *preset == value)
        .map_or_else(|| format!("{value}%"), |(_, label)| (*label).to_owned())
}

fn codec_from_selected(selected: u32) -> Codec {
    match selected {
        1 => Codec::H264,
        2 => Codec::Hevc,
        3 => Codec::Av1,
        _ => Codec::Auto,
    }
}

fn selected_monitor_for_estimate(monitors: &[Monitor], selected: u32) -> Monitor {
    usize::try_from(selected)
        .ok()
        .and_then(|index| monitors.get(index))
        .cloned()
        .unwrap_or_else(|| Monitor {
            id: 0,
            name: String::new(),
            description: String::new(),
            make: String::new(),
            model: String::new(),
            serial: String::new(),
            width: 1_920,
            height: 1_080,
            refresh_rate: 60.0,
            focused: true,
            disabled: false,
        })
}

fn quality_estimated_megabytes(config: &Config, monitor: &Monitor, quality: u8) -> u64 {
    let mut spec = ReplaySpec::from_config(config, monitor);
    spec.quality = quality;
    let audio_bytes = if spec.desktop_audio || spec.microphone_audio {
        24_000_u64.saturating_mul(u64::from(spec.duration_seconds))
    } else {
        0
    };
    let encoded_bytes = spec.estimated_buffer_bytes().saturating_add(audio_bytes);
    encoded_bytes
        .saturating_add(encoded_bytes.div_ceil(50))
        .div_ceil(1_048_576)
}

fn quality_choice_model(values: &[u8], config: &Config, monitor: Monitor) -> StringList {
    let rows = values
        .iter()
        .map(|value| {
            format!(
                "{}\u{1f}≈ {} MB total · {} s",
                quality_label(*value),
                quality_estimated_megabytes(config, &monitor, *value),
                config.capture.duration_seconds
            )
        })
        .collect::<Vec<_>>();
    StringList::new(&rows.iter().map(String::as_str).collect::<Vec<_>>())
}

fn quality_choice_parts(value: &str) -> (&str, &str) {
    value.split_once('\u{1f}').unwrap_or((value, ""))
}

fn install_quality_factories(dropdown: &DropDown) {
    let selected_factory = SignalListItemFactory::new();
    selected_factory.connect_setup(|_, item| {
        let Ok(item) = item.clone().downcast::<gtk::ListItem>() else {
            return;
        };
        let label = Label::new(None);
        label.set_halign(Align::Start);
        item.set_child(Some(&label));
    });
    selected_factory.connect_bind(|_, item| {
        let Ok(item) = item.clone().downcast::<gtk::ListItem>() else {
            return;
        };
        let Some(label) = item.child().and_downcast::<Label>() else {
            return;
        };
        let Some(value) = item.item().and_downcast::<StringObject>() else {
            return;
        };
        label.set_text(quality_choice_parts(&value.string()).0);
    });
    dropdown.set_factory(Some(&selected_factory));

    let list_factory = SignalListItemFactory::new();
    list_factory.connect_setup(|_, item| {
        let Ok(item) = item.clone().downcast::<gtk::ListItem>() else {
            return;
        };
        let option = GtkBox::new(Orientation::Vertical, 2);
        option.add_css_class("quality-choice-option");
        let label = Label::new(None);
        label.add_css_class("quality-choice-label");
        label.set_halign(Align::Start);
        let detail = Label::new(None);
        detail.add_css_class("quality-choice-detail");
        detail.set_halign(Align::Start);
        option.append(&label);
        option.append(&detail);
        item.set_child(Some(&option));
    });
    list_factory.connect_bind(|_, item| {
        let Ok(item) = item.clone().downcast::<gtk::ListItem>() else {
            return;
        };
        let Some(option) = item.child().and_downcast::<GtkBox>() else {
            return;
        };
        let Some(label) = option.first_child().and_downcast::<Label>() else {
            return;
        };
        let Some(detail) = option.last_child().and_downcast::<Label>() else {
            return;
        };
        let Some(value) = item.item().and_downcast::<StringObject>() else {
            return;
        };
        let value = value.string();
        let (title, description) = quality_choice_parts(&value);
        label.set_text(title);
        detail.set_text(description);
    });
    dropdown.set_list_factory(Some(&list_factory));
}

fn storage_values(current: u32) -> Vec<u32> {
    let mut values = STORAGE_LIMITS
        .iter()
        .map(|(value, _)| *value)
        .collect::<Vec<_>>();
    if !values.contains(&current) {
        values.push(current);
        values.sort_unstable();
    }
    values
}

fn storage_label(value: u32) -> String {
    STORAGE_LIMITS
        .iter()
        .find(|(preset, _)| *preset == value)
        .map_or_else(|| format!("{} MB", value), |(_, label)| (*label).to_owned())
}

fn autostart_enabled() -> Result<bool, String> {
    let status = Command::new("systemctl")
        .args(["--user", "is-enabled", "--quiet", "wreathd.service"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| format!("Could not read Linux autostart: {error}"))?;
    Ok(status.success())
}

fn set_autostart_enabled(enabled: bool) -> Result<(), String> {
    let action = if enabled { "enable" } else { "disable" };
    let output = Command::new("systemctl")
        .args(["--user", action, "wreathd.service"])
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("Could not update Linux autostart: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        Err(if detail.is_empty() {
            "Could not update Linux autostart.".to_owned()
        } else {
            format!("Could not update Linux autostart: {detail}")
        })
    }
}

fn microphone_model(microphones: &[Microphone]) -> StringList {
    let mut labels = vec!["Follow Linux default"];
    labels.extend(
        microphones
            .iter()
            .map(|microphone| microphone.label.as_str()),
    );
    StringList::new(&labels)
}

fn selected_microphone_index(microphones: &[Microphone], config: &Config) -> u32 {
    microphones
        .iter()
        .position(|microphone| {
            config.audio.microphone_device.as_deref() == Some(microphone.name.as_str())
        })
        .and_then(|index| u32::try_from(index + 1).ok())
        .unwrap_or(0)
}

fn settings_panel(title: &str, description: &str) -> GtkBox {
    let panel = GtkBox::new(Orientation::Vertical, 0);
    panel.add_css_class("settings-panel");
    panel.set_halign(Align::Fill);
    panel.set_hexpand(true);
    panel.set_tooltip_text(Some(&format!("{title}: {description}")));
    panel
}

fn embedded_picture(bytes: &[u8]) -> Picture {
    let loader = PixbufLoader::new();
    if loader.write(bytes).is_ok()
        && loader.close().is_ok()
        && let Some(pixbuf) = loader.pixbuf()
    {
        let pixbuf = pixbuf
            .scale_simple(253, 190, gdk_pixbuf::InterpType::Bilinear)
            .unwrap_or(pixbuf);
        let texture = gtk::gdk::Texture::for_pixbuf(&pixbuf);
        return Picture::for_paintable(&texture);
    }
    Picture::new()
}

fn settings_tab(label: &str, page: &'static str) -> (Button, &'static str) {
    let button = Button::with_label(label);
    button.add_css_class("settings-tab");
    button.update_property(&[gtk::accessible::Property::Label(&format!(
        "{label} settings"
    ))]);
    button.update_state(&[gtk::accessible::State::Selected(Some(false))]);
    (button, page)
}

fn settings_grid() -> Grid {
    let grid = Grid::new();
    grid.set_row_spacing(12);
    grid.set_hexpand(true);
    grid
}

fn attach_row(
    grid: &Grid,
    row: i32,
    label_text: &str,
    detail_text: &str,
    control: &impl IsA<gtk::Widget>,
) -> SettingsRow {
    attach_row_with_kind(grid, row, label_text, detail_text, control, false)
}

fn attach_audio_row(
    grid: &Grid,
    row: i32,
    label_text: &str,
    detail_text: &str,
    control: &impl IsA<gtk::Widget>,
) -> SettingsRow {
    attach_row_with_kind(grid, row, label_text, detail_text, control, true)
}

fn attach_row_with_kind(
    grid: &Grid,
    row: i32,
    label_text: &str,
    detail_text: &str,
    control: &impl IsA<gtk::Widget>,
    audio_column: bool,
) -> SettingsRow {
    let label = Label::new(Some(label_text));
    label.add_css_class("row-label");
    label.set_halign(Align::Start);
    let detail = Label::new(Some(detail_text));
    detail.add_css_class("row-detail");
    detail.set_halign(Align::Start);
    detail.set_wrap(true);
    detail.set_xalign(0.0);
    let copy = GtkBox::new(Orientation::Vertical, 2);
    copy.set_valign(Align::Center);
    copy.set_hexpand(true);
    copy.set_size_request(250, -1);
    copy.append(&label);
    copy.append(&detail);
    control.set_tooltip_text(Some(detail_text));
    let widget: &gtk::Widget = control.as_ref();
    widget.update_property(&[
        gtk::accessible::Property::Label(label_text),
        gtk::accessible::Property::Description(detail_text),
    ]);
    widget.add_css_class("settings-control");
    widget.set_size_request(if audio_column { 236 } else { 360 }, 42);
    widget.set_halign(Align::End);
    let container = GtkBox::new(Orientation::Horizontal, 18);
    container.add_css_class("settings-row");
    container.set_hexpand(true);
    container.set_valign(Align::Center);
    container.append(&copy);
    container.append(control);
    grid.attach(&container, 0, row, 1, 1);
    SettingsRow {
        container,
        copy,
        control: control.clone().upcast(),
        audio_column,
    }
}

fn settings_control_width(window_width: i32, audio_column: bool) -> i32 {
    let rail = if window_width < 1_080 { 72.0 } else { 88.0 };
    let padding = settings_page_padding(window_width);
    let page_width = (window_width as f32 - rail - padding * 2.0).max(1.0);
    let available = if audio_column {
        (page_width - 12.0) / 2.0
    } else {
        page_width
    };
    (available * 0.38).clamp(190.0, 360.0).round() as i32
}

fn settings_page_padding(window_width: i32) -> f32 {
    if window_width < 1_080 {
        28.0
    } else if window_width < 1_300 {
        36.0
    } else {
        48.0
    }
}

fn settings_sticker_size(window_width: i32, window_height: i32) -> (i32, i32) {
    const ASPECT_RATIO: f32 = 577.0 / 433.0;
    let rail = if window_width < 1_080 { 72.0 } else { 88.0 };
    let page_width =
        (window_width as f32 - rail - settings_page_padding(window_width) * 2.0).max(1.0);
    let available_height = (window_height as f32 - 558.0).max(0.0);
    let height = available_height
        .min(190.0)
        .min(page_width * 0.42 / ASPECT_RATIO);
    if height < 96.0 {
        return (0, 0);
    }
    (
        (height * ASPECT_RATIO).round() as i32,
        height.round() as i32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turns_pressed_keys_into_a_hotkey() {
        let modifiers = gtk::gdk::ModifierType::SUPER_MASK | gtk::gdk::ModifierType::SHIFT_MASK;
        let hotkey = hotkey_from_key(gtk::gdk::Key::r, modifiers).unwrap();

        assert_eq!(hotkey.to_string(), "SUPER+SHIFT+R");
    }

    #[test]
    fn includes_the_modifier_currently_being_pressed() {
        assert_eq!(
            hotkey_modifiers(
                gtk::gdk::Key::Control_L,
                gtk::gdk::ModifierType::NO_MODIFIER_MASK
            ),
            vec!["CTRL"]
        );
    }

    #[test]
    fn non_modifier_finishes_hotkey_capture_without_confirmation() {
        assert_eq!(
            hotkey_capture_action(gtk::gdk::Key::r, gtk::gdk::ModifierType::CONTROL_MASK),
            HotkeyCaptureAction::Complete(HotkeyConfig {
                modifiers: vec!["CTRL".into()],
                key: "R".into(),
            })
        );
        assert_eq!(
            hotkey_capture_action(
                gtk::gdk::Key::Control_L,
                gtk::gdk::ModifierType::NO_MODIFIER_MASK
            ),
            HotkeyCaptureAction::Preview("CTRL+…".into())
        );
        assert_eq!(
            hotkey_capture_action(
                gtk::gdk::Key::Escape,
                gtk::gdk::ModifierType::NO_MODIFIER_MASK
            ),
            HotkeyCaptureAction::Cancel
        );
    }

    #[test]
    fn windows_quality_presets_and_custom_values_are_preserved() {
        assert_eq!(quality_values(75), vec![50, 65, 75, 85, 100]);
        assert_eq!(quality_label(75), "High");
        assert_eq!(quality_values(62), vec![50, 62, 65, 75, 85, 100]);
        assert_eq!(quality_label(62), "62%");
    }

    #[test]
    fn quality_size_preview_matches_the_windows_backend_formula() {
        let config = Config::default();
        let monitor = selected_monitor_for_estimate(&[], gtk::INVALID_LIST_POSITION);
        let mut spec = ReplaySpec::from_config(&config, &monitor);
        spec.quality = 75;
        let audio_bytes = if spec.desktop_audio || spec.microphone_audio {
            24_000_u64.saturating_mul(u64::from(spec.duration_seconds))
        } else {
            0
        };
        let encoded_bytes = spec.estimated_buffer_bytes().saturating_add(audio_bytes);
        let expected = encoded_bytes
            .saturating_add(encoded_bytes.div_ceil(50))
            .div_ceil(1_048_576);

        assert_eq!(quality_estimated_megabytes(&config, &monitor, 75), expected);
        assert_eq!(
            quality_choice_parts("High\u{1f}≈ 120 MB total · 30 s"),
            ("High", "≈ 120 MB total · 30 s")
        );
    }

    #[test]
    fn windows_storage_limits_and_custom_values_are_preserved() {
        assert_eq!(
            storage_values(10_240),
            vec![1_024, 5_120, 10_240, 25_600, 51_200, 102_400]
        );
        assert_eq!(storage_label(10_240), "10 GB");
        assert_eq!(storage_label(2_048), "2048 MB");
        assert!(storage_values(2_048).contains(&2_048));
    }

    #[test]
    fn an_unavailable_configured_output_stays_selectable() {
        let outputs = configured_desktop_outputs(Vec::new(), Some("custom.monitor"));
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].name, "custom.monitor");
        assert!(outputs[0].label.contains("unavailable"));
    }

    #[test]
    fn an_unavailable_configured_microphone_stays_selectable() {
        let microphones = configured_microphones(Vec::new(), Some("custom.input"));
        assert_eq!(microphones.len(), 1);
        assert_eq!(microphones[0].name, "custom.input");
        assert!(microphones[0].label.contains("unavailable"));
    }

    #[test]
    fn system_default_microphone_keeps_an_empty_device_id() {
        let config = Config::default();
        assert_eq!(selected_microphone_index(&[], &config), 0);
    }

    #[test]
    fn frame_rates_follow_the_selected_monitor() {
        let monitors = vec![Monitor {
            id: 0,
            name: "DP-1".into(),
            description: "Display".into(),
            make: String::new(),
            model: String::new(),
            serial: String::new(),
            width: 1920,
            height: 1080,
            refresh_rate: 50.0,
            focused: true,
            disabled: false,
        }];

        assert_eq!(frame_rate_values(&monitors, 0, 50), vec![30, 48, 50]);
    }

    #[test]
    fn settings_controls_follow_the_windows_row_geometry() {
        assert_eq!(settings_control_width(1_440, false), 360);
        assert_eq!(settings_control_width(1_440, true), 236);
        assert_eq!(settings_control_width(1_280, false), 360);
        assert_eq!(settings_control_width(1_280, true), 211);
        assert_eq!(settings_control_width(980, false), 324);
        assert_eq!(settings_control_width(980, true), 190);
    }

    #[test]
    fn settings_sticker_follows_the_windows_bottom_layout() {
        assert_eq!(settings_sticker_size(1_440, 900), (253, 190));
        assert_eq!(settings_sticker_size(980, 680), (163, 122));
        assert_eq!(settings_sticker_size(980, 650), (0, 0));
    }
}
