use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;
use gtk::{
    Adjustment, Align, Box as GtkBox, Button, CheckButton, DropDown, Entry, Grid, Label,
    Orientation, Scale, ScrolledWindow, Separator, SpinButton, StringList,
};
use trace_core::audio::{self, Microphone};
use trace_core::config::{Codec, Config, HotkeyConfig};
use trace_core::hyprland::{self, Monitor};
use trace_core::paths::AppPaths;

#[derive(Clone)]
pub struct SettingsView {
    pub page: ScrolledWindow,
    rows: Vec<SettingsRow>,
    grids: Vec<Grid>,
    footer: GtkBox,
    feedback: Label,
    apply: Button,
}

#[derive(Clone)]
struct SettingsRow {
    grid: Grid,
    label: Label,
    control: gtk::Widget,
    row: i32,
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
            .set_size_request(if compact { -1 } else { 132 }, 42);
    }
}

pub fn build() -> SettingsView {
    let paths = AppPaths::discover();
    let config = Config::load(&paths).unwrap_or_default();
    let monitors = hyprland::monitors().unwrap_or_default();
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

    let mut rows = Vec::new();
    let mut grids = Vec::new();
    root.append(&section_title("DISPLAY"));
    let display_grid = settings_grid();
    grids.push(display_grid.clone());
    let monitor_model = monitor_model(&monitors);
    let monitor_dropdown = DropDown::new(Some(monitor_model.clone()), None::<gtk::Expression>);
    monitor_dropdown.set_hexpand(true);
    monitor_dropdown.set_selected(selected_monitor_index(&monitors, &config));
    rows.push(attach_row(&display_grid, 0, "Monitor", &monitor_dropdown));
    root.append(&display_grid);

    root.append(&section_separator());
    root.append(&section_title("CAPTURE"));
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
    root.append(&capture_grid);

    root.append(&section_separator());
    root.append(&section_title("CONTROL"));
    let control_grid = settings_grid();
    grids.push(control_grid.clone());
    let hotkey = Entry::new();
    hotkey.set_text(&config.hotkey.to_string());
    hotkey.set_placeholder_text(Some("SUPER+SHIFT+R"));
    hotkey.set_hexpand(true);
    rows.push(attach_row(&control_grid, 0, "Save replay", &hotkey));
    root.append(&control_grid);

    root.append(&section_separator());
    root.append(&section_title("AUDIO"));
    let audio_grid = settings_grid();
    grids.push(audio_grid.clone());
    let desktop_audio = CheckButton::with_label("Desktop audio");
    desktop_audio.set_active(config.audio.desktop);
    rows.push(attach_row(&audio_grid, 0, "System sound", &desktop_audio));
    let microphone = CheckButton::with_label("Include microphone");
    microphone.set_active(config.audio.microphone);
    rows.push(attach_row(&audio_grid, 1, "Voice", &microphone));
    let microphone_model = microphone_model(&microphones);
    let microphone_dropdown = DropDown::new(Some(microphone_model), None::<gtk::Expression>);
    microphone_dropdown.set_hexpand(true);
    microphone_dropdown.set_selected(selected_microphone_index(&microphones, &config));
    microphone_dropdown.set_sensitive(config.audio.microphone && !microphones.is_empty());
    microphone_dropdown.set_tooltip_text(Some("PipeWire microphone used in new clips"));
    rows.push(attach_row(
        &audio_grid,
        2,
        "Input device",
        &microphone_dropdown,
    ));
    let microphone_gain = Scale::with_range(Orientation::Horizontal, 0.0, 200.0, 5.0);
    microphone_gain.set_value(f64::from(config.audio.microphone_gain_percent));
    microphone_gain.set_draw_value(false);
    microphone_gain.set_hexpand(true);
    microphone_gain.set_sensitive(config.audio.microphone);
    microphone_gain.set_tooltip_text(Some(
        "Only changes microphone loudness in Trace recordings, never the system microphone volume",
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
        3,
        "Recording level",
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
    root.append(&audio_grid);

    root.append(&section_separator());
    root.append(&section_title("STORAGE"));
    let storage_grid = settings_grid();
    grids.push(storage_grid.clone());
    let output = Entry::new();
    output.set_text(&config.storage.directory.to_string_lossy());
    output.set_hexpand(true);
    rows.push(attach_row(&storage_grid, 0, "Save location", &output));
    root.append(&storage_grid);

    let footer = GtkBox::new(Orientation::Horizontal, 18);
    footer.set_margin_top(32);
    let feedback = Label::new(Some("Everything stays on this machine."));
    feedback.add_css_class("feedback");
    feedback.set_halign(Align::Start);
    feedback.set_hexpand(true);
    let apply = Button::with_label("Save changes");
    apply.add_css_class("primary-action");
    apply.set_size_request(132, 42);
    footer.append(&feedback);
    footer.append(&apply);
    root.append(&footer);

    let save_paths = paths.clone();
    let saved_feedback = feedback.clone();
    let saved_apply = apply.clone();
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
                saved_apply.set_label("✓ Saved");
                saved_apply.add_css_class("saved");

                let reset_apply = saved_apply.clone();
                let reset_generation = save_generation.clone();
                glib::timeout_add_local_once(Duration::from_millis(2200), move || {
                    if reset_generation.get() == generation {
                        reset_apply.set_label("Save changes");
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
                saved_apply.set_label("Save changes");
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
    hotkey: &Entry,
    desktop_audio: &CheckButton,
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
    config.hotkey =
        HotkeyConfig::parse(hotkey.text().as_str()).map_err(|error| error.to_string())?;
    config.audio.desktop = desktop_audio.is_active();
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
    hyprland::replace_replay_bind(Some(&previous_hotkey), &config.hotkey, &control)
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
        .map(|directory| directory.join("tracectl"))
        .filter(|path| path.exists())
        .unwrap_or_else(|| PathBuf::from("tracectl"))
}

fn monitor_model(monitors: &[Monitor]) -> StringList {
    let labels = monitors
        .iter()
        .map(|monitor| {
            format!(
                "{} · {} × {} · {:.0} Hz",
                monitor.name, monitor.width, monitor.height, monitor.refresh_rate
            )
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

fn section_title(text: &str) -> Label {
    let label = Label::new(Some(text));
    label.add_css_class("section-title");
    label.set_halign(Align::Start);
    label.set_margin_bottom(8);
    label
}

fn section_separator() -> Separator {
    let separator = Separator::new(Orientation::Horizontal);
    separator.set_margin_top(22);
    separator.set_margin_bottom(20);
    separator
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
