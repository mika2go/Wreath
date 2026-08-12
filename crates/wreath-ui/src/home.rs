use gdk_pixbuf::PixbufLoader;
use gdk_pixbuf::prelude::PixbufLoaderExt;
use gtk::gdk;
use gtk::prelude::*;
use gtk::{Align, Box as GtkBox, ContentFit, Label, Orientation, Overlay, Picture};
use wreath_core::clips;
use wreath_core::config::Config;
use wreath_core::display;
use wreath_core::paths::AppPaths;

const HOME_GIRL_PNG: &[u8] = include_bytes!("../../../assets/wreath-home-girl.png");

#[derive(Clone)]
pub struct HomeView {
    pub page: Overlay,
    content: GtkBox,
    mascot: GtkBox,
}

impl HomeView {
    pub fn set_layout(&self, compact: bool, very_narrow: bool) {
        self.content
            .set_size_request(if compact { 500 } else { 620 }, -1);
        self.mascot.set_size_request(
            if compact { 220 } else { 306 },
            if compact { 309 } else { 430 },
        );
        self.mascot.set_visible(!very_narrow);
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

    let stage = GtkBox::new(Orientation::Horizontal, 0);
    stage.add_css_class("home-stage");
    let mascot_spacer = GtkBox::new(Orientation::Horizontal, 0);
    mascot_spacer.set_hexpand(true);
    let mascot = GtkBox::new(Orientation::Vertical, 0);
    mascot.add_css_class("home-mascot");
    mascot.set_halign(Align::End);
    mascot.set_valign(Align::End);
    mascot.set_size_request(306, 430);
    mascot.set_overflow(gtk::Overflow::Hidden);
    let mascot_picture = embedded_picture(HOME_GIRL_PNG);
    mascot_picture.set_can_shrink(true);
    mascot_picture.set_content_fit(ContentFit::Contain);
    mascot_picture.set_hexpand(true);
    mascot_picture.set_vexpand(true);
    mascot_picture.set_size_request(1, 1);
    mascot.append(&mascot_picture);
    stage.append(&mascot_spacer);
    stage.append(&mascot);
    page.set_child(Some(&stage));

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
    facts.append(&fact("Display", &display, false));
    facts.append(&fact("Audio", audio, true));
    facts.append(&fact(
        "Library",
        &format!("{} clips · {}", local_clips.len(), format_bytes(total_size)),
        true,
    ));
    status_body.append(&facts);

    status.append(&status_rule);
    status.append(&status_body);
    content.append(&status);
    page.add_overlay(&content);

    HomeView {
        page,
        content,
        mascot,
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

fn fact(label: &str, value: &str, divided: bool) -> GtkBox {
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
    item
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

#[cfg(test)]
mod tests {
    use super::{audio_label, format_bytes};
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
}
