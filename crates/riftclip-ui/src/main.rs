use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use gtk::gdk;
use gtk::prelude::*;
use gtk::{
    Adjustment, Align, Application, ApplicationWindow, Box as GtkBox, Button, CheckButton,
    CssProvider, DropDown, Entry, Grid, Label, Orientation, Scale, ScrolledWindow, Separator,
    SpinButton, StringList,
};
use riftclip_core::config::{Codec, Config, HotkeyConfig};
use riftclip_core::hyprland::{self, Monitor};
use riftclip_core::paths::AppPaths;

const APP_ID: &str = "io.github.mika2go.Riftclip";

fn main() -> ExitCode {
    let application = Application::builder().application_id(APP_ID).build();
    application.connect_startup(|_| install_css());
    application.connect_activate(build_ui);
    application.run().into()
}

fn install_css() {
    let provider = CssProvider::new();
    provider.load_from_string(include_str!("style.css"));
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

fn build_ui(application: &Application) {
    let paths = AppPaths::discover();
    let config = Config::load(&paths).unwrap_or_default();
    let monitors = hyprland::monitors().unwrap_or_default();

    let window = ApplicationWindow::builder()
        .application(application)
        .title("Riftclip")
        .default_width(680)
        .default_height(720)
        .resizable(true)
        .build();
    window.add_css_class("riftclip-window");

    let root = GtkBox::new(Orientation::Vertical, 0);
    root.set_margin_top(34);
    root.set_margin_bottom(28);
    root.set_margin_start(38);
    root.set_margin_end(38);

    let heading = GtkBox::new(Orientation::Horizontal, 16);
    heading.set_margin_bottom(34);
    let title_stack = GtkBox::new(Orientation::Vertical, 3);
    title_stack.set_hexpand(true);
    let eyebrow = Label::new(Some("INSTANT REPLAY"));
    eyebrow.add_css_class("eyebrow");
    eyebrow.set_halign(Align::Start);
    let title = Label::new(Some("Riftclip"));
    title.add_css_class("title");
    title.set_halign(Align::Start);
    let subtitle = Label::new(Some("Local capture. Nothing leaves this machine."));
    subtitle.add_css_class("subtitle");
    subtitle.set_halign(Align::Start);
    title_stack.append(&eyebrow);
    title_stack.append(&title);
    title_stack.append(&subtitle);
    heading.append(&title_stack);

    let state = Label::new(Some("LOCAL"));
    state.add_css_class("state");
    state.set_valign(Align::Start);
    heading.append(&state);
    root.append(&heading);

    let display_title = section_title("DISPLAY");
    root.append(&display_title);
    let display_grid = settings_grid();
    let monitor_model = monitor_model(&monitors);
    let monitor_dropdown = DropDown::new(Some(monitor_model.clone()), None::<gtk::Expression>);
    monitor_dropdown.set_hexpand(true);
    monitor_dropdown.set_selected(selected_monitor_index(&monitors, &config));
    attach_row(&display_grid, 0, "Monitor", &monitor_dropdown);
    root.append(&display_grid);

    root.append(&section_separator());
    root.append(&section_title("CAPTURE"));
    let capture_grid = settings_grid();
    let duration = spin_button(5.0, 600.0, 5.0, f64::from(config.capture.duration_seconds));
    duration.set_tooltip_text(Some("Seconds retained in the encoded replay buffer"));
    attach_row(&capture_grid, 0, "Clip length", &duration);

    let fps = spin_button(
        15.0,
        240.0,
        15.0,
        f64::from(config.capture.frames_per_second),
    );
    attach_row(&capture_grid, 1, "Frames per second", &fps);

    let codec_model = StringList::new(&["Automatic", "H.264", "HEVC", "AV1"]);
    let codec = DropDown::new(Some(codec_model), None::<gtk::Expression>);
    codec.set_selected(match config.capture.codec {
        Codec::Auto => 0,
        Codec::H264 => 1,
        Codec::Hevc => 2,
        Codec::Av1 => 3,
    });
    attach_row(&capture_grid, 2, "Codec", &codec);

    let quality = Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 1.0);
    quality.set_value(f64::from(config.capture.quality));
    quality.set_draw_value(false);
    quality.set_hexpand(true);
    attach_row(&capture_grid, 3, "Quality", &quality);
    root.append(&capture_grid);

    root.append(&section_separator());
    root.append(&section_title("CONTROL"));
    let control_grid = settings_grid();
    let hotkey = Entry::new();
    hotkey.set_text(&config.hotkey.to_string());
    hotkey.set_placeholder_text(Some("SUPER+SHIFT+R"));
    hotkey.set_hexpand(true);
    attach_row(&control_grid, 0, "Save replay", &hotkey);
    root.append(&control_grid);

    root.append(&section_separator());
    root.append(&section_title("AUDIO & STORAGE"));
    let final_grid = settings_grid();
    let desktop_audio = CheckButton::with_label("Desktop audio");
    desktop_audio.set_active(config.audio.desktop);
    attach_row(&final_grid, 0, "Audio", &desktop_audio);
    let microphone = CheckButton::with_label("Microphone");
    microphone.set_active(config.audio.microphone);
    attach_row(&final_grid, 1, "", &microphone);
    let output = Entry::new();
    output.set_text(&config.storage.directory.to_string_lossy());
    output.set_hexpand(true);
    attach_row(&final_grid, 2, "Save location", &output);
    root.append(&final_grid);

    let footer = GtkBox::new(Orientation::Horizontal, 18);
    footer.set_margin_top(32);
    let feedback = Label::new(Some("Changes stay local."));
    feedback.add_css_class("feedback");
    feedback.set_halign(Align::Start);
    feedback.set_hexpand(true);
    let apply = Button::with_label("Apply settings");
    apply.add_css_class("apply");
    apply.set_size_request(132, 40);
    footer.append(&feedback);
    footer.append(&apply);
    root.append(&footer);

    let save_paths = paths.clone();
    apply.connect_clicked(move |_| {
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
            &output,
        ) {
            Ok(()) => {
                feedback.set_text("Saved locally.");
                feedback.remove_css_class("error");
            }
            Err(error) => {
                feedback.set_text(&error);
                feedback.add_css_class("error");
            }
        }
    });

    let scroll = ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_child(Some(&root));
    window.set_child(Some(&scroll));
    window.present();
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
    output: &Entry,
) -> Result<(), String> {
    let mut config = Config::load(paths).unwrap_or_default();
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
    let output_path = PathBuf::from(output.text().as_str());
    if !output_path.is_absolute() {
        return Err("Save location must be an absolute local path.".into());
    }
    config.storage.directory = output_path;
    config.save(paths).map_err(|error| error.to_string())?;

    let control = sibling_control_executable();
    hyprland::install_replay_bind(&config.hotkey, &control).map_err(|error| error.to_string())?;
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
        .map(|directory| directory.join("riftclipctl"))
        .filter(|path| path.exists())
        .unwrap_or_else(|| PathBuf::from("riftclipctl"))
}

fn monitor_model(monitors: &[Monitor]) -> StringList {
    let labels = monitors
        .iter()
        .map(|monitor| {
            format!(
                "{} · {} × {} · {:.0} Hz",
                monitor.description, monitor.width, monitor.height, monitor.refresh_rate
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

fn attach_row(grid: &Grid, row: i32, label_text: &str, control: &impl IsA<gtk::Widget>) {
    let label = Label::new(Some(label_text));
    label.add_css_class("row-label");
    label.set_halign(Align::Start);
    label.set_valign(Align::Center);
    label.set_size_request(158, -1);
    grid.attach(&label, 0, row, 1, 1);
    grid.attach(control, 1, row, 1, 1);
}

fn spin_button(minimum: f64, maximum: f64, step: f64, value: f64) -> SpinButton {
    let adjustment = Adjustment::new(value, minimum, maximum, step, step * 2.0, 0.0);
    let spin = SpinButton::new(Some(&adjustment), step, 0);
    spin.set_hexpand(true);
    spin
}
