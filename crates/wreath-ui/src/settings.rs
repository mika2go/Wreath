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
    Adjustment, Align, Box as GtkBox, Button, CheckButton, ContentFit, DropDown, Entry,
    EventControllerFocus, EventControllerKey, FileDialog, GestureClick, Grid, Image, Label,
    Orientation, Picture, Scale, ScrolledWindow, SpinButton, Stack, StringList,
};
use wreath_core::audio::{self, DesktopOutput, Microphone};
use wreath_core::config::{Codec, Config, HotkeyConfig, MAX_FRAMES_PER_SECOND};
use wreath_core::display::{self, Monitor};
use wreath_core::paths::AppPaths;
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
    grid: Grid,
    copy: GtkBox,
    control: gtk::Widget,
    row: i32,
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
const SETTINGS_STICKER_PNG: &[u8] = include_bytes!("../../../assets/wreath-settings-67.png");

#[derive(Clone)]
struct HotkeyCapture {
    entry: Entry,
    confirmed: Rc<RefCell<HotkeyConfig>>,
    awaiting_confirmation: Rc<Cell<bool>>,
}

impl HotkeyCapture {
    fn new(hotkey: &HotkeyConfig) -> Self {
        let entry = Entry::new();
        entry.set_text(&hotkey.to_string());
        entry.set_editable(false);
        entry.set_hexpand(true);
        entry.set_tooltip_text(Some(
            "Click, press the new shortcut, then press Enter to confirm",
        ));

        let confirmed = Rc::new(RefCell::new(hotkey.clone()));
        let pending = Rc::new(RefCell::new(None::<HotkeyConfig>));
        let recording = Rc::new(Cell::new(false));
        let awaiting_confirmation = Rc::new(Cell::new(false));

        let focus_controller = EventControllerFocus::new();
        let focus_entry = entry.clone();
        let enter_pending = pending.clone();
        let enter_recording = recording.clone();
        let enter_awaiting = awaiting_confirmation.clone();
        focus_controller.connect_enter(move |_| {
            begin_hotkey_capture(
                &focus_entry,
                &enter_pending,
                &enter_recording,
                &enter_awaiting,
            );
        });

        let leave_entry = entry.clone();
        let focus_pending = pending.clone();
        let focus_recording = recording.clone();
        focus_controller.connect_leave(move |_| {
            restore_system_shortcuts(&leave_entry);
            if focus_recording.replace(false) {
                leave_entry.remove_css_class("recording");
                if focus_pending.borrow().is_none() {
                    leave_entry.set_text("No shortcut captured · click to try again");
                }
            }
        });
        entry.add_controller(focus_controller);

        let click_controller = GestureClick::new();
        let click_entry = entry.clone();
        let click_pending = pending.clone();
        let click_recording = recording.clone();
        let click_awaiting = awaiting_confirmation.clone();
        click_controller.connect_pressed(move |_, _, _, _| {
            begin_hotkey_capture(
                &click_entry,
                &click_pending,
                &click_recording,
                &click_awaiting,
            );
        });
        entry.add_controller(click_controller);

        let controller = EventControllerKey::new();
        let key_entry = entry.clone();
        let key_confirmed = confirmed.clone();
        let key_pending = pending.clone();
        let key_recording = recording.clone();
        let key_awaiting = awaiting_confirmation.clone();
        controller.connect_key_pressed(move |_, key, _, modifiers| {
            if !key_recording.get() {
                return glib::Propagation::Proceed;
            }

            if is_confirm_key(key) {
                if let Some(hotkey) = key_pending.borrow_mut().take() {
                    key_entry.set_text(&hotkey.to_string());
                    *key_confirmed.borrow_mut() = hotkey;
                    key_recording.set(false);
                    key_awaiting.set(false);
                    key_entry.remove_css_class("recording");
                    key_entry.set_tooltip_text(Some(
                        "Shortcut confirmed. Click to record a different shortcut",
                    ));
                    clear_hotkey_focus(&key_entry);
                } else {
                    key_entry.set_text("Press a shortcut first · Enter confirms");
                }
                return glib::Propagation::Stop;
            }

            if key == gtk::gdk::Key::Escape {
                key_pending.borrow_mut().take();
                key_entry.set_text(&key_confirmed.borrow().to_string());
                key_recording.set(false);
                key_awaiting.set(false);
                key_entry.remove_css_class("recording");
                clear_hotkey_focus(&key_entry);
                return glib::Propagation::Stop;
            }

            let modifier_names = hotkey_modifiers(key, modifiers);
            if is_modifier_key(key) {
                let preview = if modifier_names.is_empty() {
                    "Press shortcut · Enter confirms".to_owned()
                } else {
                    format!("{}+…", modifier_names.join("+"))
                };
                key_entry.set_text(&preview);
                return glib::Propagation::Stop;
            }

            match hotkey_from_key(key, modifiers) {
                Ok(hotkey) => {
                    key_entry.set_text(&format!("{hotkey} · Enter confirms"));
                    *key_pending.borrow_mut() = Some(hotkey);
                }
                Err(_) => {
                    key_pending.borrow_mut().take();
                    key_entry.set_text("Unsupported key · try another shortcut");
                }
            }
            glib::Propagation::Stop
        });
        entry.add_controller(controller);

        Self {
            entry,
            confirmed,
            awaiting_confirmation,
        }
    }

    fn value(&self) -> Result<HotkeyConfig, String> {
        if self.awaiting_confirmation.get() {
            return Err("Confirm the recorded shortcut with Enter before saving.".into());
        }
        Ok(self.confirmed.borrow().clone())
    }
}

fn begin_hotkey_capture(
    entry: &Entry,
    pending: &RefCell<Option<HotkeyConfig>>,
    recording: &Cell<bool>,
    awaiting_confirmation: &Cell<bool>,
) {
    recording.set(true);
    awaiting_confirmation.set(true);
    inhibit_system_shortcuts(entry);
    if let Some(hotkey) = pending.borrow().as_ref() {
        entry.set_text(&format!("{hotkey} · Enter confirms"));
    } else {
        entry.set_text("Press shortcut · Enter confirms");
    }
    entry.add_css_class("recording");
    entry.set_tooltip_text(Some("Press all shortcut keys together, then press Enter"));
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

fn is_confirm_key(key: gtk::gdk::Key) -> bool {
    matches!(
        key,
        gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter | gtk::gdk::Key::ISO_Enter
    )
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
    pub fn set_compact(&self, compact: bool) {
        for grid in &self.grids {
            grid.set_column_spacing(if compact { 0 } else { 28 });
            grid.set_row_spacing(if compact { 14 } else { 18 });
        }
        for row in &self.rows {
            row.grid.remove(&row.copy);
            row.grid.remove(&row.control);
            if compact {
                row.copy.set_size_request(-1, -1);
                row.copy.set_margin_bottom(2);
                row.grid.attach(&row.copy, 0, row.row * 2, 1, 1);
                row.grid.attach(&row.control, 0, row.row * 2 + 1, 1, 1);
            } else {
                row.copy.set_size_request(250, -1);
                row.copy.set_margin_bottom(0);
                row.grid.attach(&row.copy, 0, row.row, 1, 1);
                row.grid.attach(&row.control, 1, row.row, 1, 1);
            }
        }
        self.footer.set_orientation(if compact {
            Orientation::Vertical
        } else {
            Orientation::Horizontal
        });
        self.footer.set_spacing(if compact { 10 } else { 18 });
        self.feedback.set_wrap(compact);
        self.apply
            .set_halign(if compact { Align::Fill } else { Align::End });
        self.apply
            .set_size_request(if compact { -1 } else { 132 }, 42);
        for panel in &self.panels {
            panel.set_size_request(-1, -1);
            panel.set_hexpand(true);
        }
        self.audio_columns.set_orientation(if compact {
            Orientation::Vertical
        } else {
            Orientation::Horizontal
        });
        self.audio_columns
            .set_spacing(if compact { 18 } else { 24 });
        self.sticker.set_visible(!compact);
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
    let microphones = audio::microphones().unwrap_or_default();
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
        "Monitor",
        "Capture this display when the replay buffer starts.",
        &monitor_dropdown,
    ));
    let fps = spin_button(
        15.0,
        f64::from(MAX_FRAMES_PER_SECOND),
        15.0,
        f64::from(config.capture.frames_per_second),
    );
    rows.push(attach_row(
        &display_grid,
        1,
        "Frame rate",
        "Higher rates look smoother and use more GPU memory.",
        &fps,
    ));
    let capture_cursor = CheckButton::with_label("Include cursor");
    capture_cursor.set_active(config.capture.cursor);
    rows.push(attach_row(
        &display_grid,
        2,
        "Capture cursor",
        "Include the hardware cursor in saved clips.",
        &capture_cursor,
    ));
    display_page.append(&display_grid);

    let capture_grid = settings_grid();
    grids.push(capture_grid.clone());
    let duration = spin_button(5.0, 600.0, 5.0, f64::from(config.capture.duration_seconds));
    duration.set_tooltip_text(Some("Seconds retained in the encoded replay buffer"));
    rows.push(attach_row(
        &capture_grid,
        0,
        "Clip length",
        "Longer replays keep more moments but use more memory.",
        &duration,
    ));
    let codec_model = StringList::new(&["Automatic", "H.264", "HEVC", "AV1"]);
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
        "Video codec",
        "Automatic follows the best supported hardware encoder.",
        &codec,
    ));
    let quality_values = quality_values(config.capture.quality);
    let quality_labels = quality_values
        .iter()
        .map(|value| quality_label(*value))
        .collect::<Vec<_>>();
    let quality_model = StringList::new(
        &quality_labels
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    );
    let quality = DropDown::new(Some(quality_model), None::<gtk::Expression>);
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
        "Higher presets preserve more detail and use more storage.",
        &quality,
    ));
    quality_page.append(&capture_grid);

    let control_grid = settings_grid();
    grids.push(control_grid.clone());
    let hotkey = HotkeyCapture::new(&config.hotkey);
    rows.push(attach_row(
        &control_grid,
        0,
        "Save replay hotkey",
        controls_description,
        &hotkey.entry,
    ));
    let autostart = CheckButton::with_label("Start after sign-in");
    autostart.set_active(initial_autostart);
    rows.push(attach_row(
        &control_grid,
        1,
        "Start with Linux",
        "Start Wreath automatically when your user session begins.",
        &autostart,
    ));
    controls_page.append(&control_grid);

    let audio_columns = GtkBox::new(Orientation::Horizontal, 24);
    audio_columns.set_hexpand(true);
    let game_audio_grid = settings_grid();
    game_audio_grid.set_hexpand(true);
    let microphone_grid = settings_grid();
    microphone_grid.set_hexpand(true);
    grids.push(game_audio_grid.clone());
    grids.push(microphone_grid.clone());
    let desktop_audio = CheckButton::with_label("Desktop audio");
    desktop_audio.set_active(config.audio.desktop);
    rows.push(attach_row(
        &game_audio_grid,
        0,
        "Game audio",
        "Record game and system sound.",
        &desktop_audio,
    ));
    let desktop_gain = Scale::with_range(Orientation::Horizontal, 0.0, 200.0, 5.0);
    desktop_gain.update_property(&[
        gtk::accessible::Property::Label("Game audio level"),
        gtk::accessible::Property::Description(
            "Recording level; Linux system volume stays unchanged.",
        ),
    ]);
    desktop_gain.set_value(f64::from(config.audio.desktop_gain_percent));
    desktop_gain.update_property(&[gtk::accessible::Property::ValueText(&format!(
        "{}%",
        config.audio.desktop_gain_percent
    ))]);
    desktop_gain.set_draw_value(false);
    desktop_gain.set_hexpand(true);
    desktop_gain.set_sensitive(config.audio.desktop);
    desktop_gain.set_tooltip_text(Some(
        "Only changes desktop loudness in Wreath recordings, never the system volume",
    ));
    let desktop_gain_value = Label::new(Some(&format!("{}%", config.audio.desktop_gain_percent)));
    desktop_gain_value.add_css_class("gain-value");
    desktop_gain_value.set_width_chars(4);
    desktop_gain_value.set_xalign(1.0);
    let desktop_gain_control = GtkBox::new(Orientation::Horizontal, 14);
    desktop_gain_control.set_hexpand(true);
    desktop_gain_control.append(&desktop_gain);
    desktop_gain_control.append(&desktop_gain_value);
    rows.push(attach_row(
        &game_audio_grid,
        1,
        "Game audio level",
        "Recording level; Linux system volume stays unchanged.",
        &desktop_gain_control,
    ));
    let displayed_desktop_gain = desktop_gain_value.clone();
    desktop_gain.connect_value_changed(move |scale| {
        let value = format!("{:.0}%", scale.value());
        displayed_desktop_gain.set_text(&value);
        scale.update_property(&[gtk::accessible::Property::ValueText(&value)]);
    });
    let desktop_output_model = desktop_output_model(&desktop_outputs);
    let desktop_output_dropdown =
        DropDown::new(Some(desktop_output_model), None::<gtk::Expression>);
    desktop_output_dropdown.set_hexpand(true);
    desktop_output_dropdown.set_selected(selected_desktop_output_index(&desktop_outputs, &config));
    desktop_output_dropdown.set_sensitive(config.audio.desktop);
    rows.push(attach_row(
        &game_audio_grid,
        2,
        "Output device",
        "Capture this output instead of following the Linux default.",
        &desktop_output_dropdown,
    ));
    let microphone = CheckButton::with_label("Include microphone");
    microphone.set_active(config.audio.microphone);
    rows.push(attach_row(
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
    microphone_dropdown.set_sensitive(config.audio.microphone && !microphones.is_empty());
    microphone_dropdown.set_tooltip_text(Some("PipeWire microphone used in new clips"));
    rows.push(attach_row(
        &microphone_grid,
        1,
        "Input device",
        "Use this PipeWire input for new clips.",
        &microphone_dropdown,
    ));
    let microphone_gain = Scale::with_range(Orientation::Horizontal, 0.0, 200.0, 5.0);
    microphone_gain.update_property(&[
        gtk::accessible::Property::Label("Microphone level"),
        gtk::accessible::Property::Description(
            "Recording level; Linux microphone volume stays unchanged.",
        ),
    ]);
    microphone_gain.set_value(f64::from(config.audio.microphone_gain_percent));
    microphone_gain.update_property(&[gtk::accessible::Property::ValueText(&format!(
        "{}%",
        config.audio.microphone_gain_percent
    ))]);
    microphone_gain.set_draw_value(false);
    microphone_gain.set_hexpand(true);
    microphone_gain.set_sensitive(config.audio.microphone);
    microphone_gain.set_tooltip_text(Some(
        "Only changes microphone loudness in Wreath recordings, never the system microphone volume",
    ));
    let microphone_gain_value =
        Label::new(Some(&format!("{}%", config.audio.microphone_gain_percent)));
    microphone_gain_value.add_css_class("gain-value");
    microphone_gain_value.set_width_chars(4);
    microphone_gain_value.set_xalign(1.0);
    let microphone_gain_control = GtkBox::new(Orientation::Horizontal, 14);
    microphone_gain_control.set_hexpand(true);
    microphone_gain_control.append(&microphone_gain);
    microphone_gain_control.append(&microphone_gain_value);
    rows.push(attach_row(
        &microphone_grid,
        2,
        "Microphone level",
        "Recording level; Linux microphone volume stays unchanged.",
        &microphone_gain_control,
    ));
    let displayed_gain = microphone_gain_value.clone();
    microphone_gain.connect_value_changed(move |scale| {
        let value = format!("{:.0}%", scale.value());
        displayed_gain.set_text(&value);
        scale.update_property(&[gtk::accessible::Property::ValueText(&value)]);
    });
    let microphone_toggle_dropdown = microphone_dropdown.clone();
    let microphone_toggle_gain = microphone_gain.clone();
    let microphones_available = !microphones.is_empty();
    microphone.connect_toggled(move |toggle| {
        microphone_toggle_dropdown.set_sensitive(toggle.is_active() && microphones_available);
        microphone_toggle_gain.set_sensitive(toggle.is_active());
    });
    let desktop_toggle_gain = desktop_gain.clone();
    let desktop_toggle_output = desktop_output_dropdown.clone();
    desktop_audio.connect_toggled(move |toggle| {
        desktop_toggle_gain.set_sensitive(toggle.is_active());
        desktop_toggle_output.set_sensitive(toggle.is_active());
    });
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
    let feedback = Label::new(Some("Everything stays on this machine."));
    feedback.add_css_class("feedback");
    feedback.set_halign(Align::Start);
    feedback.set_hexpand(true);
    let apply_content = GtkBox::new(Orientation::Horizontal, 8);
    let apply_icon = Image::from_icon_name("document-save-symbolic");
    apply_icon.set_pixel_size(17);
    let apply_label = Label::new(Some("Save settings"));
    apply_content.append(&apply_icon);
    apply_content.append(&apply_label);
    let apply = Button::new();
    apply.add_css_class("settings-save-action");
    apply.update_property(&[gtk::accessible::Property::Label("Save settings")]);
    apply.set_child(Some(&apply_content));
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
    let sticker_spacer = GtkBox::new(Orientation::Vertical, 0);
    sticker_spacer.set_vexpand(true);
    root.append(&sticker_spacer);
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
    root.append(&sticker);

    let save_paths = paths.clone();
    let saved_feedback = feedback.clone();
    let saved_apply = apply.clone();
    let saved_apply_icon = apply_icon.clone();
    let saved_apply_label = apply_label.clone();
    let save_generation = Rc::new(Cell::new(0_u64));
    apply.connect_clicked(move |_| {
        let generation = save_generation.get().wrapping_add(1);
        save_generation.set(generation);
        match collect_and_save(
            &save_paths,
            &monitors,
            &monitor_dropdown,
            &duration,
            &fps,
            &codec,
            &quality_values,
            &quality,
            &capture_cursor,
            &hotkey,
            initial_autostart,
            &autostart,
            &desktop_audio,
            &desktop_gain,
            &desktop_outputs,
            &desktop_output_dropdown,
            &microphone,
            &microphones,
            &microphone_dropdown,
            &microphone_gain,
            &output,
            &storage_values,
            &storage_limit,
        ) {
            Ok(()) => {
                saved_feedback.set_text("✓ Changes saved locally. Recorder updated.");
                saved_feedback.remove_css_class("error");
                saved_feedback.add_css_class("success");
                saved_apply_icon.set_icon_name(Some("emblem-ok-symbolic"));
                saved_apply_label.set_text("Saved");
                saved_apply.add_css_class("saved");

                let reset_apply = saved_apply.clone();
                let reset_apply_icon = saved_apply_icon.clone();
                let reset_apply_label = saved_apply_label.clone();
                let reset_generation = save_generation.clone();
                glib::timeout_add_local_once(Duration::from_millis(2200), move || {
                    if reset_generation.get() == generation {
                        reset_apply_icon.set_icon_name(Some("document-save-symbolic"));
                        reset_apply_label.set_text("Save settings");
                        reset_apply.remove_css_class("saved");
                    }
                });

                let reset_feedback = saved_feedback.clone();
                let reset_generation = save_generation.clone();
                glib::timeout_add_local_once(Duration::from_secs(5), move || {
                    if reset_generation.get() == generation {
                        reset_feedback.set_text("Everything stays on this machine.");
                        reset_feedback.remove_css_class("success");
                    }
                });
            }
            Err(error) => {
                saved_feedback.set_text(&error);
                saved_feedback.remove_css_class("success");
                saved_feedback.add_css_class("error");
                saved_apply_icon.set_icon_name(Some("document-save-symbolic"));
                saved_apply_label.set_text("Save settings");
                saved_apply.remove_css_class("saved");
            }
        }
    });

    let scroll = ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_child(Some(&root));
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
    duration: &SpinButton,
    fps: &SpinButton,
    codec: &DropDown,
    quality_values: &[u8],
    quality: &DropDown,
    capture_cursor: &CheckButton,
    hotkey: &HotkeyCapture,
    initial_autostart: bool,
    autostart: &CheckButton,
    desktop_audio: &CheckButton,
    desktop_gain: &Scale,
    desktop_outputs: &[DesktopOutput],
    desktop_output_dropdown: &DropDown,
    microphone: &CheckButton,
    microphones: &[Microphone],
    microphone_dropdown: &DropDown,
    microphone_gain: &Scale,
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
    config.capture.duration_seconds =
        u16::try_from(duration.value_as_int()).map_err(|_| "Invalid clip length.".to_owned())?;
    config.capture.frames_per_second =
        u16::try_from(fps.value_as_int()).map_err(|_| "Invalid frame rate.".to_owned())?;
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
    config.audio.desktop_gain_percent = desktop_gain.value().round().clamp(0.0, 200.0) as u16;
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
    config.audio.microphone_device = microphones
        .get(microphone_index)
        .map(|microphone| microphone.name.clone());
    config.audio.microphone_gain_percent = microphone_gain.value().round().clamp(0.0, 200.0) as u16;
    if config.audio.microphone && config.audio.microphone_device.is_none() {
        return Err("Select an available microphone.".into());
    }
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
    if autostart.is_active() != initial_autostart {
        set_autostart_enabled(autostart.is_active())?;
    }
    config.save(paths).map_err(|error| error.to_string())?;

    let control = sibling_control_executable();
    shortcuts::replace(Some(&previous_hotkey), &config.hotkey, &control)
        .map_err(|error| error.to_string())?;
    let _ = Command::new(control)
        .arg("reload")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
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
    if microphones.is_empty() {
        return StringList::new(&["No microphones found"]);
    }
    let labels = microphones
        .iter()
        .map(|microphone| microphone.label.as_str())
        .collect::<Vec<_>>();
    StringList::new(&labels)
}

fn selected_microphone_index(microphones: &[Microphone], config: &Config) -> u32 {
    microphones
        .iter()
        .position(|microphone| {
            config.audio.microphone_device.as_deref() == Some(microphone.name.as_str())
        })
        .or_else(|| {
            microphones
                .iter()
                .position(|microphone| microphone.is_default)
        })
        .or((!microphones.is_empty()).then_some(0))
        .and_then(|index| u32::try_from(index).ok())
        .unwrap_or(gtk::INVALID_LIST_POSITION)
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
    grid.set_column_spacing(28);
    grid.set_row_spacing(18);
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
    copy.set_size_request(250, -1);
    copy.append(&label);
    copy.append(&detail);
    control.set_tooltip_text(Some(detail_text));
    let widget: &gtk::Widget = control.as_ref();
    widget.update_property(&[
        gtk::accessible::Property::Label(label_text),
        gtk::accessible::Property::Description(detail_text),
    ]);
    grid.attach(&copy, 0, row, 1, 1);
    grid.attach(control, 1, row, 1, 1);
    SettingsRow {
        grid: grid.clone(),
        copy,
        control: control.clone().upcast(),
        row,
    }
}

fn spin_button(minimum: f64, maximum: f64, step: f64, value: f64) -> SpinButton {
    let adjustment = Adjustment::new(value, minimum, maximum, step, step * 2.0, 0.0);
    let spin = SpinButton::new(Some(&adjustment), step, 0);
    spin.set_hexpand(true);
    spin
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
    fn enter_is_reserved_for_confirmation() {
        assert!(is_confirm_key(gtk::gdk::Key::Return));
        assert!(is_confirm_key(gtk::gdk::Key::KP_Enter));
    }

    #[test]
    fn windows_quality_presets_and_custom_values_are_preserved() {
        assert_eq!(quality_values(75), vec![50, 65, 75, 85, 100]);
        assert_eq!(quality_label(75), "High");
        assert_eq!(quality_values(62), vec![50, 62, 65, 75, 85, 100]);
        assert_eq!(quality_label(62), "62%");
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
}
