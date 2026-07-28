use gtk::glib;
use gtk::prelude::*;
use gtk::{Align, Box as GtkBox, Button, Image, Label, Orientation};
use trace_core::clips;
use trace_core::config::Config;
use trace_core::paths::AppPaths;

#[derive(Clone)]
pub struct HomeView {
    pub page: GtkBox,
    pub open_library: Button,
    pub open_collections: Button,
    stats: GtkBox,
    actions: GtkBox,
}

impl HomeView {
    pub fn set_compact(&self, compact: bool) {
        self.stats.set_orientation(if compact {
            Orientation::Vertical
        } else {
            Orientation::Horizontal
        });
        self.stats.set_spacing(if compact { 16 } else { 38 });
        self.actions.set_orientation(if compact {
            Orientation::Vertical
        } else {
            Orientation::Horizontal
        });
        self.open_library
            .set_halign(if compact { Align::Fill } else { Align::Start });
        self.open_collections
            .set_halign(if compact { Align::Fill } else { Align::Start });
    }
}

pub fn build() -> HomeView {
    let paths = AppPaths::discover();
    let config = Config::load(&paths).unwrap_or_default();
    let clip_count = clips::scan(&config.storage.directory)
        .map(|clips| clips.len())
        .unwrap_or_default();
    let collection_count = clips::collections(&config.storage.directory)
        .map(|collections| collections.len())
        .unwrap_or_default();

    let page = GtkBox::new(Orientation::Vertical, 0);
    page.add_css_class("home-page");
    let greeting = Label::new(Some(&greeting()));
    greeting.add_css_class("home-greeting");
    greeting.set_halign(Align::Start);
    let subtitle = Label::new(Some("Your local replay workspace is ready."));
    subtitle.add_css_class("page-subtitle");
    subtitle.set_halign(Align::Start);
    page.append(&greeting);
    page.append(&subtitle);

    let stats = GtkBox::new(Orientation::Horizontal, 38);
    stats.add_css_class("home-stats");
    stats.set_halign(Align::Start);
    stats.append(&stat("CLIPS", &clip_count.to_string()));
    stats.append(&stat("COLLECTIONS", &collection_count.to_string()));
    stats.append(&stat(
        "REPLAY",
        &format!("{} sec", config.capture.duration_seconds),
    ));
    page.append(&stats);

    let section = Label::new(Some("QUICK ACCESS"));
    section.add_css_class("section-title");
    section.set_halign(Align::Start);
    section.set_margin_bottom(10);
    page.append(&section);

    let actions = GtkBox::new(Orientation::Horizontal, 12);
    actions.set_halign(Align::Start);
    let open_library = action("Open Library", "video-display-symbolic");
    let open_collections = action("Browse Collections", "folder-symbolic");
    actions.append(&open_library);
    actions.append(&open_collections);
    page.append(&actions);

    HomeView {
        page,
        open_library,
        open_collections,
        stats,
        actions,
    }
}

fn greeting() -> String {
    let hour = glib::DateTime::now_local()
        .map(|time| time.hour())
        .unwrap_or(12);
    let salutation = match hour {
        5..=11 => "Good morning",
        12..=17 => "Good afternoon",
        _ => "Good evening",
    };
    let user = std::env::var("USER")
        .ok()
        .filter(|value| !value.is_empty())
        .map(|value| {
            let mut characters = value.chars();
            characters
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + characters.as_str())
                .unwrap_or(value)
        });
    user.map_or_else(
        || salutation.to_owned(),
        |user| format!("{salutation}, {user}"),
    )
}

fn stat(label: &str, value: &str) -> GtkBox {
    let item = GtkBox::new(Orientation::Vertical, 2);
    let value = Label::new(Some(value));
    value.add_css_class("home-stat-value");
    value.set_halign(Align::Start);
    let label = Label::new(Some(label));
    label.add_css_class("home-stat-label");
    label.set_halign(Align::Start);
    item.append(&value);
    item.append(&label);
    item
}

fn action(label: &str, icon_name: &str) -> Button {
    let content = GtkBox::new(Orientation::Horizontal, 10);
    let icon = Image::from_icon_name(icon_name);
    icon.set_pixel_size(18);
    let label = Label::new(Some(label));
    content.append(&icon);
    content.append(&label);
    let button = Button::new();
    button.add_css_class("home-action");
    button.set_child(Some(&content));
    button
}
