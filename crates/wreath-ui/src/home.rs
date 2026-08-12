use gdk_pixbuf::PixbufLoader;
use gdk_pixbuf::prelude::PixbufLoaderExt;
use gtk::gdk;
use gtk::prelude::*;
use gtk::{Align, Box as GtkBox, ContentFit, Fixed, Label, Orientation, Overlay, Picture};
use wreath_core::clips;
use wreath_core::config::Config;
use wreath_core::display;
use wreath_core::paths::AppPaths;

const HOME_GIRL_PNG: &[u8] = include_bytes!("../../../assets/wreath-home-girl.png");
const HOME_GIRL_ASPECT_RATIO: f32 = 1206.0 / 1693.0;
const HOME_GIRL_BOTTOM_OVERFLOW: f32 = 70.0;

#[derive(Clone)]
pub struct HomeView {
    pub page: Overlay,
    content: GtkBox,
    mascot: Fixed,
    mascot_picture: Picture,
    title: Label,
    status: GtkBox,
    state_label: Label,
    message: Label,
    detail: Label,
    signal_ticks: Vec<GtkBox>,
    display_value: Label,
    audio_value: Label,
    library_value: Label,
}

impl HomeView {
    pub fn set_layout(&self, window_width: i32, window_height: i32) {
        let (content_width, mascot_width, mascot_height, visible_height) =
            home_layout(window_width, window_height);
        let (mascot_left, mascot_top) = home_mascot_offset(
            self.page.width(),
            self.page.height(),
            mascot_width,
            visible_height,
        );
        self.content.set_size_request(content_width, -1);
        self.mascot.set_size_request(mascot_width, visible_height);
        self.mascot.set_margin_start(mascot_left);
        self.mascot.set_margin_top(mascot_top);
        self.mascot_picture
            .set_size_request(mascot_width, mascot_height);
        self.mascot
            .set_visible(mascot_width > 0 && visible_height > 0);
    }

    pub fn refresh(&self) {
        let paths = AppPaths::discover();
        let config = Config::load(&paths).unwrap_or_default();
        let local_clips = clips::scan(&config.storage.directory).unwrap_or_default();
        let total_size = local_clips.iter().map(|clip| clip.size_bytes).sum();
        let hotkey_ready = config.hotkey.is_bound();

        self.title.set_text(if hotkey_ready {
            "Replay ready"
        } else {
            "Shortcut required"
        });
        self.status.remove_css_class("ready");
        self.status.remove_css_class("error");
        self.status
            .add_css_class(if hotkey_ready { "ready" } else { "error" });
        self.state_label.set_text(if hotkey_ready {
            "CAPTURE READY"
        } else {
            "HOTKEY NOT SET"
        });
        self.message.set_text(&if hotkey_ready {
            format!(
                "Your last {} seconds are ready.",
                config.capture.duration_seconds
            )
        } else {
            "Set a shortcut before you start clipping.".to_owned()
        });
        self.detail.set_text(if hotkey_ready {
            "Save a replay without leaving what you are doing."
        } else {
            "Wreath cannot save a replay until a hotkey is configured in Settings → Controls."
        });
        for tick in &self.signal_ticks {
            tick.remove_css_class("ready");
            tick.remove_css_class("error");
            tick.add_css_class(if hotkey_ready { "ready" } else { "error" });
        }
        self.display_value.set_text(&selected_display(&config));
        self.audio_value.set_text(audio_label(&config));
        self.library_value.set_text(&format!(
            "{} clips · {}",
            local_clips.len(),
            format_bytes(total_size)
        ));
    }
}

pub fn build() -> HomeView {
    let paths = AppPaths::discover();
    let config = Config::load(&paths).unwrap_or_default();
    let local_clips = clips::scan(&config.storage.directory).unwrap_or_default();
    let total_size = local_clips.iter().map(|clip| clip.size_bytes).sum();
    let hotkey_ready = config.hotkey.is_bound();

    let page = Overlay::new();
    page.add_css_class("home-page");
    page.set_overflow(gtk::Overflow::Hidden);

    let stage = GtkBox::new(Orientation::Horizontal, 0);
    stage.add_css_class("home-stage");
    stage.set_hexpand(true);
    stage.set_vexpand(true);
    page.set_child(Some(&stage));

    let mascot = Fixed::new();
    mascot.add_css_class("home-mascot");
    mascot.set_halign(Align::Start);
    mascot.set_valign(Align::Start);
    mascot.set_size_request(356, 430);
    mascot.set_overflow(gtk::Overflow::Hidden);
    let mascot_picture = embedded_picture(HOME_GIRL_PNG);
    // The source artwork is 1206×1693. GTK must be allowed to scale it into
    // the Windows-sized destination instead of requesting its full source size.
    mascot_picture.set_can_shrink(true);
    mascot_picture.set_content_fit(ContentFit::Contain);
    mascot_picture.set_size_request(356, 500);
    mascot.put(&mascot_picture, 0.0, 0.0);
    page.add_overlay(&mascot);

    let content = GtkBox::new(Orientation::Vertical, 0);
    content.add_css_class("home-content");
    content.set_halign(Align::Start);
    content.set_valign(Align::Start);
    content.set_size_request(620, -1);

    let context = Label::new(Some("Replay buffer"));
    context.add_css_class("home-context");
    context.set_halign(Align::Start);
    let title = Label::new(Some(if hotkey_ready {
        "Replay ready"
    } else {
        "Shortcut required"
    }));
    title.add_css_class("home-title");
    title.set_halign(Align::Start);
    content.append(&context);
    content.append(&title);

    let status = GtkBox::new(Orientation::Horizontal, 0);
    status.add_css_class("home-status");
    if hotkey_ready {
        status.add_css_class("ready");
    } else {
        status.add_css_class("error");
    }

    let status_rule = GtkBox::new(Orientation::Vertical, 0);
    status_rule.add_css_class("home-status-rule");
    let status_body = GtkBox::new(Orientation::Vertical, 0);
    status_body.add_css_class("home-status-body");

    let state = GtkBox::new(Orientation::Horizontal, 14);
    let state_dot = GtkBox::new(Orientation::Horizontal, 0);
    state_dot.add_css_class("home-state-dot");
    let state_label = Label::new(Some(if hotkey_ready {
        "CAPTURE READY"
    } else {
        "HOTKEY NOT SET"
    }));
    state_label.add_css_class("home-state-label");
    state_label.set_halign(Align::Start);
    state.append(&state_dot);
    state.append(&state_label);
    status_body.append(&state);

    let message = Label::new(Some(&if hotkey_ready {
        format!(
            "Your last {} seconds are ready.",
            config.capture.duration_seconds
        )
    } else {
        "Set a shortcut before you start clipping.".to_owned()
    }));
    message.add_css_class("home-status-title");
    message.set_halign(Align::Start);
    message.set_wrap(true);
    status_body.append(&message);

    let detail = Label::new(Some(if hotkey_ready {
        "Save a replay without leaving what you are doing."
    } else {
        "Wreath cannot save a replay until a hotkey is configured in Settings → Controls."
    }));
    detail.add_css_class("home-status-detail");
    detail.set_halign(Align::Start);
    detail.set_wrap(true);
    status_body.append(&detail);

    let signal = GtkBox::new(Orientation::Horizontal, 0);
    signal.add_css_class("home-signal");
    signal.set_homogeneous(true);
    let mut signal_ticks = Vec::new();
    for index in 0..28 {
        let holder = GtkBox::new(Orientation::Vertical, 0);
        holder.set_valign(Align::Center);
        let tick = GtkBox::new(Orientation::Vertical, 0);
        tick.add_css_class("home-signal-tick");
        tick.add_css_class(match index % 5 {
            0 => "tall",
            1 | 4 => "short",
            _ => "medium",
        });
        if index >= 24 {
            tick.add_css_class(if hotkey_ready { "ready" } else { "error" });
            signal_ticks.push(tick.clone());
        }
        holder.append(&tick);
        signal.append(&holder);
    }
    status_body.append(&signal);

    let facts = GtkBox::new(Orientation::Horizontal, 0);
    facts.add_css_class("home-facts");
    facts.set_homogeneous(true);
    let display = selected_display(&config);
    let audio = audio_label(&config);
    let (display_fact, display_value) = fact("Display", &display, false);
    let (audio_fact, audio_value) = fact("Audio", audio, true);
    let (library_fact, library_value) = fact(
        "Library",
        &format!("{} clips · {}", local_clips.len(), format_bytes(total_size)),
        true,
    );
    facts.append(&display_fact);
    facts.append(&audio_fact);
    facts.append(&library_fact);
    status_body.append(&facts);

    status.append(&status_rule);
    status.append(&status_body);
    content.append(&status);
    page.add_overlay(&content);

    HomeView {
        page,
        content,
        mascot,
        mascot_picture,
        title,
        status,
        state_label,
        message,
        detail,
        signal_ticks,
        display_value,
        audio_value,
        library_value,
    }
}

fn embedded_picture(bytes: &[u8]) -> Picture {
    let loader = PixbufLoader::new();
    if loader.write(bytes).is_ok()
        && loader.close().is_ok()
        && let Some(pixbuf) = loader.pixbuf()
    {
        let texture = gdk::Texture::for_pixbuf(&pixbuf);
        return Picture::for_paintable(&texture);
    }
    Picture::new()
}

fn fact(label: &str, value: &str, divided: bool) -> (GtkBox, Label) {
    let item = GtkBox::new(Orientation::Vertical, 4);
    item.add_css_class("home-fact");
    if divided {
        item.add_css_class("divided");
    }
    let label = Label::new(Some(label));
    label.add_css_class("home-fact-label");
    label.set_halign(Align::Start);
    let value = Label::new(Some(value));
    value.add_css_class("home-fact-value");
    value.set_halign(Align::Start);
    value.set_ellipsize(gtk::pango::EllipsizeMode::End);
    item.append(&label);
    item.append(&value);
    (item, value)
}

fn selected_display(config: &Config) -> String {
    display::monitors()
        .ok()
        .and_then(|monitors| {
            config
                .capture
                .monitor
                .as_deref()
                .and_then(|configured| {
                    monitors.iter().find(|monitor| {
                        monitor.name.eq_ignore_ascii_case(configured)
                            || monitor.description.eq_ignore_ascii_case(configured)
                    })
                })
                .or_else(|| monitors.iter().find(|monitor| monitor.focused))
                .or_else(|| monitors.first())
                .map(|monitor| {
                    format!(
                        "{}×{} · {} fps",
                        monitor.width, monitor.height, config.capture.frames_per_second
                    )
                })
        })
        .unwrap_or_else(|| format!("{} fps", config.capture.frames_per_second))
}

fn audio_label(config: &Config) -> &'static str {
    match (config.audio.desktop, config.audio.microphone) {
        (true, true) => "Game + microphone",
        (true, false) => "Game audio",
        (false, true) => "Microphone",
        (false, false) => "Audio off",
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    }
}

fn home_layout(window_width: i32, window_height: i32) -> (i32, i32, i32, i32) {
    let width = window_width as f32;
    let height = window_height as f32;
    let rail = if width < 1_080.0 { 72.0 } else { 88.0 };
    let padding = if width < 1_080.0 {
        28.0
    } else if width < 1_300.0 {
        36.0
    } else {
        48.0
    };
    let content_left = rail + padding;
    let content_right = width - padding;
    let minimum_content_width = if width < 1_080.0 { 420.0 } else { 540.0 };
    let available_height = (height - 250.0).max(180.0);
    let available_width = (width - content_left - minimum_content_width - 18.0).max(160.0);
    let mascot_height = (height * 0.57)
        .clamp(240.0, 500.0)
        .min(available_height)
        .min(available_width / HOME_GIRL_ASPECT_RATIO);
    let mascot_width = mascot_height * HOME_GIRL_ASPECT_RATIO;
    let text_right = (width - mascot_width - 18.0)
        .max(content_left + minimum_content_width)
        .min(content_right);
    let visible_height = (mascot_height - HOME_GIRL_BOTTOM_OVERFLOW).max(1.0);
    (
        (text_right - content_left).round() as i32,
        mascot_width.round() as i32,
        mascot_height.round() as i32,
        visible_height.round() as i32,
    )
}

fn home_mascot_offset(
    page_width: i32,
    page_height: i32,
    mascot_width: i32,
    visible_height: i32,
) -> (i32, i32) {
    (
        (page_width - mascot_width).max(0),
        (page_height - visible_height).max(0),
    )
}

#[cfg(test)]
mod tests {
    use super::{audio_label, format_bytes, home_layout, home_mascot_offset};
    use wreath_core::config::Config;

    #[test]
    fn home_storage_uses_the_same_units_as_windows() {
        assert_eq!(format_bytes(512 * 1_024), "0.5 MB");
        assert_eq!(format_bytes(5 * 1_073_741_824), "5.0 GB");
    }

    #[test]
    fn home_audio_summary_covers_every_capture_combination() {
        let mut config = Config::default();
        config.audio.desktop = true;
        config.audio.microphone = false;
        assert_eq!(audio_label(&config), "Game audio");
        config.audio.microphone = true;
        assert_eq!(audio_label(&config), "Game + microphone");
        config.audio.desktop = false;
        assert_eq!(audio_label(&config), "Microphone");
        config.audio.microphone = false;
        assert_eq!(audio_label(&config), "Audio off");
    }

    #[test]
    fn home_layout_matches_the_windows_mascot_crop_and_text_width() {
        assert_eq!(home_layout(1_440, 900), (930, 356, 500, 430));
        assert_eq!(home_layout(980, 680), (586, 276, 388, 318));
    }

    #[test]
    fn home_mascot_is_anchored_to_the_real_page_corner() {
        assert_eq!(home_mascot_offset(1_352, 816, 356, 430), (996, 386));
        assert_eq!(home_mascot_offset(200, 180, 276, 318), (0, 0));
    }
}
