use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;
use gtk::{
    Adjustment, Align, Box as GtkBox, Button, CheckButton, DropDown, Entry, EventControllerFocus,
    EventControllerKey, GestureClick, Grid, Image, Label, Orientation, Scale, ScrolledWindow,
    SpinButton, Stack, StringList,
};
use wreath_core::audio::{self, Microphone};
use wreath_core::config::{Codec, Config, HotkeyConfig};
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
}

#[derive(Clone)]
struct SettingsRow {
    grid: Grid,
    label: Label,
    control: gtk::Widget,
    row: i32,
}

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
            grid.set_row_spacing(if compact { 7 } else { 9 });
        }
        for row in &self.rows {
            row.grid.remove(&row.label);
            row.grid.remove(&row.control);
            if compact {
                row.label.set_size_request(-1, -1);
                row.label.set_margin_bottom(2);
                row.grid.attach(&row.label, 0, row.row * 2, 1, 1);
                row.grid.attach(&row.control, 0, row.row * 2 + 1, 1, 1);
            } else {
                row.label.set_size_request(132, -1);
                row.label.set_margin_bottom(0);
                row.grid.attach(&row.label, 0, row.row, 1, 1);
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
            .set_size_request(if compact { -1 } else { 92 }, 42);
        for panel in &self.panels {
            panel.set_size_request(if compact { -1 } else { 640 }, -1);
            panel.set_hexpand(compact);
        }
    }
}

pub fn build() -> SettingsView {
    let paths = AppPaths::discover();
    let config = Config::load(&paths).unwrap_or_default();
    let monitors = display::monitors().unwrap_or_default();
    let microphones = audio::microphones().unwrap_or_default();

    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("settings-page");

    let title = Label::new(Some("Settings"));
    title.add_css_class("page-title");
    title.set_halign(Align::Start);
    let subtitle = Label::new(Some(
        "Capture exactly what you want, without background clutter.",
    ));
    subtitle.add_css_class("page-subtitle");
    subtitle.set_halign(Align::Start);
    subtitle.set_wrap(true);
    subtitle.set_margin_bottom(34);
    root.append(&title);
    root.append(&subtitle);

    let tabs = GtkBox::new(Orientation::Horizontal, 4);
    tabs.add_css_class("settings-tabs");
    tabs.set_halign(Align::Start);
    tabs.set_margin_bottom(28);
    let settings_stack = Stack::new();
    settings_stack.add_css_class("settings-stack");
    settings_stack.set_hhomogeneous(false);
    settings_stack.set_vhomogeneous(false);
    settings_stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    settings_stack.set_transition_duration(120);

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
            }
            active.add_css_class("active");
        });
    }
    root.append(&tabs);
    root.append(&settings_stack);

    let mut rows = Vec::new();
    let mut grids = Vec::new();
    let display_grid = settings_grid();
    grids.push(display_grid.clone());
    let monitor_model = monitor_model(&monitors);
    let monitor_dropdown = DropDown::new(Some(monitor_model.clone()), None::<gtk::Expression>);
    monitor_dropdown.set_hexpand(true);
    monitor_dropdown.set_selected(selected_monitor_index(&monitors, &config));
    rows.push(attach_row(&display_grid, 0, "Monitor", &monitor_dropdown));
    display_page.append(&display_grid);

    let capture_grid = settings_grid();
    grids.push(capture_grid.clone());
    let duration = spin_button(5.0, 600.0, 5.0, f64::from(config.capture.duration_seconds));
    duration.set_tooltip_text(Some("Seconds retained in the encoded replay buffer"));
    rows.push(attach_row(&capture_grid, 0, "Clip length", &duration));
    let fps = spin_button(
        15.0,
        240.0,
        15.0,
        f64::from(config.capture.frames_per_second),
    );
    rows.push(attach_row(&capture_grid, 1, "Frames per second", &fps));
    let codec_model = StringList::new(&["Automatic", "H.264", "HEVC", "AV1"]);
    let codec = DropDown::new(Some(codec_model), None::<gtk::Expression>);
    codec.set_selected(match config.capture.codec {
        Codec::Auto => 0,
        Codec::H264 => 1,
        Codec::Hevc => 2,
        Codec::Av1 => 3,
    });
    rows.push(attach_row(&capture_grid, 2, "Codec", &codec));
    let quality = Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 1.0);
    quality.set_value(f64::from(config.capture.quality));
    quality.set_draw_value(false);
    quality.set_hexpand(true);
    rows.push(attach_row(&capture_grid, 3, "Quality", &quality));
    quality_page.append(&capture_grid);

    let control_grid = settings_grid();
    grids.push(control_grid.clone());
    let hotkey = HotkeyCapture::new(&config.hotkey);
    rows.push(attach_row(&control_grid, 0, "Save replay", &hotkey.entry));
    controls_page.append(&control_grid);

    let audio_grid = settings_grid();
    grids.push(audio_grid.clone());
    let desktop_audio = CheckButton::with_label("Desktop audio");
    desktop_audio.set_active(config.audio.desktop);
    rows.push(attach_row(&audio_grid, 0, "System sound", &desktop_audio));
    let desktop_gain = Scale::with_range(Orientation::Horizontal, 0.0, 200.0, 5.0);
    desktop_gain.set_value(f64::from(config.audio.desktop_gain_percent));
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
        &audio_grid,
        1,
        "Desktop level",
        &desktop_gain_control,
    ));
    let displayed_desktop_gain = desktop_gain_value.clone();
    desktop_gain.connect_value_changed(move |scale| {
        displayed_desktop_gain.set_text(&format!("{:.0}%", scale.value()));
    });
    let microphone = CheckButton::with_label("Include microphone");
    microphone.set_active(config.audio.microphone);
    rows.push(attach_row(&audio_grid, 2, "Voice", &microphone));
    let microphone_model = microphone_model(&microphones);
    let microphone_dropdown = DropDown::new(Some(microphone_model), None::<gtk::Expression>);
    microphone_dropdown.set_hexpand(true);
    microphone_dropdown.set_selected(selected_microphone_index(&microphones, &config));
    microphone_dropdown.set_sensitive(config.audio.microphone && !microphones.is_empty());
    microphone_dropdown.set_tooltip_text(Some("PipeWire microphone used in new clips"));
    rows.push(attach_row(
        &audio_grid,
        3,
        "Input device",
        &microphone_dropdown,
    ));
    let microphone_gain = Scale::with_range(Orientation::Horizontal, 0.0, 200.0, 5.0);
    microphone_gain.set_value(f64::from(config.audio.microphone_gain_percent));
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
        &audio_grid,
        4,
        "Microphone level",
        &microphone_gain_control,
    ));
    let displayed_gain = microphone_gain_value.clone();
    microphone_gain.connect_value_changed(move |scale| {
        displayed_gain.set_text(&format!("{:.0}%", scale.value()));
    });
    let microphone_toggle_dropdown = microphone_dropdown.clone();
    let microphone_toggle_gain = microphone_gain.clone();
    let microphones_available = !microphones.is_empty();
    microphone.connect_toggled(move |toggle| {
        microphone_toggle_dropdown.set_sensitive(toggle.is_active() && microphones_available);
        microphone_toggle_gain.set_sensitive(toggle.is_active());
    });
    let desktop_toggle_gain = desktop_gain.clone();
    desktop_audio.connect_toggled(move |toggle| {
        desktop_toggle_gain.set_sensitive(toggle.is_active());
    });
    audio_page.append(&audio_grid);

    let storage_grid = settings_grid();
    grids.push(storage_grid.clone());
    let output = Entry::new();
    output.set_text(&config.storage.directory.to_string_lossy());
    output.set_hexpand(true);
    rows.push(attach_row(&storage_grid, 0, "Save location", &output));
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
    let apply_label = Label::new(Some("Save"));
    apply_content.append(&apply_icon);
    apply_content.append(&apply_label);
    let apply = Button::new();
    apply.add_css_class("settings-save-action");
    apply.set_child(Some(&apply_content));
    apply.set_size_request(92, 42);
    footer.append(&feedback);
    footer.append(&apply);
    root.append(&footer);

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
            &quality,
            &hotkey,
            &desktop_audio,
            &desktop_gain,
            &microphone,
            &microphones,
            &microphone_dropdown,
            &microphone_gain,
            &output,
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
                        reset_apply_label.set_text("Save");
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
                saved_apply_label.set_text("Save");
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
    quality: &Scale,
    hotkey: &HotkeyCapture,
    desktop_audio: &CheckButton,
    desktop_gain: &Scale,
    microphone: &CheckButton,
    microphones: &[Microphone],
    microphone_dropdown: &DropDown,
    microphone_gain: &Scale,
    output: &Entry,
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
    config.capture.quality = quality.value().round().clamp(0.0, 100.0) as u8;
    config.hotkey = hotkey.value()?;
    config.audio.desktop = desktop_audio.is_active();
    config.audio.desktop_gain_percent = desktop_gain.value().round().clamp(0.0, 200.0) as u16;
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
    panel.set_halign(Align::Start);
    panel.set_size_request(640, -1);
    let title = Label::new(Some(title));
    title.add_css_class("settings-panel-title");
    title.set_halign(Align::Start);
    let description = Label::new(Some(description));
    description.add_css_class("settings-panel-description");
    description.set_halign(Align::Start);
    description.set_wrap(true);
    description.set_margin_bottom(22);
    panel.append(&title);
    panel.append(&description);
    panel
}

fn settings_tab(label: &str, page: &'static str) -> (Button, &'static str) {
    let button = Button::with_label(label);
    button.add_css_class("settings-tab");
    (button, page)
}

fn settings_grid() -> Grid {
    let grid = Grid::new();
    grid.set_column_spacing(28);
    grid.set_row_spacing(9);
    grid.set_hexpand(true);
    grid
}

fn attach_row(
    grid: &Grid,
    row: i32,
    label_text: &str,
    control: &impl IsA<gtk::Widget>,
) -> SettingsRow {
    let label = Label::new(Some(label_text));
    label.add_css_class("row-label");
    label.set_halign(Align::Start);
    label.set_valign(Align::Center);
    label.set_size_request(132, -1);
    grid.attach(&label, 0, row, 1, 1);
    grid.attach(control, 1, row, 1, 1);
    SettingsRow {
        grid: grid.clone(),
        label,
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
}
