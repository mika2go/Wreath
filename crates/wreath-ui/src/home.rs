use gtk::glib;
use gtk::pango;
use gtk::prelude::*;
use gtk::{
    Align, Box as GtkBox, Button, ContentFit, FlowBox, Image, Label, Orientation, Overlay, Picture,
    SelectionMode,
};
use wreath_core::clips::{self, Clip};
use wreath_core::config::Config;
use wreath_core::paths::AppPaths;

#[derive(Clone)]
pub struct HomeView {
    pub page: GtkBox,
    pub open_library: Button,
    pub open_collections: Button,
    stats: GtkBox,
    actions: GtkBox,
    recent: FlowBox,
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
        let columns = if compact { 2 } else { 4 };
        self.recent.set_min_children_per_line(columns);
        self.recent.set_max_children_per_line(columns);
    }
}

pub fn build() -> HomeView {
    let paths = AppPaths::discover();
    let config = Config::load(&paths).unwrap_or_default();
    let local_clips = clips::scan(&config.storage.directory).unwrap_or_default();
    let clip_count = local_clips.len();
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

    if !local_clips.is_empty() {
        let recent_header = GtkBox::new(Orientation::Horizontal, 12);
        recent_header.add_css_class("home-recent-header");
        let recent_title = Label::new(Some("Recent clips"));
        recent_title.add_css_class("home-section-title");
        recent_title.set_halign(Align::Start);
        recent_title.set_hexpand(true);
        let recent_count = Label::new(Some(&format!("Latest {}", local_clips.len().min(8))));
        recent_count.add_css_class("home-section-detail");
        recent_header.append(&recent_title);
        recent_header.append(&recent_count);
        page.append(&recent_header);
    }

    let recent = FlowBox::new();
    recent.add_css_class("home-recent-flow");
    recent.set_selection_mode(SelectionMode::None);
    recent.set_column_spacing(12);
    recent.set_row_spacing(12);
    recent.set_homogeneous(true);
    recent.set_min_children_per_line(4);
    recent.set_max_children_per_line(4);
    recent.set_valign(Align::Start);
    recent.set_vexpand(false);
    for clip in local_clips.iter().take(8) {
        recent.insert(&recent_clip(clip, &paths.thumbnail_dir), -1);
    }
    recent.set_visible(!local_clips.is_empty());
    page.append(&recent);

    HomeView {
        page,
        open_library,
        open_collections,
        stats,
        actions,
        recent,
    }
}

fn recent_clip(clip: &Clip, thumbnail_directory: &std::path::Path) -> GtkBox {
    let card = GtkBox::new(Orientation::Vertical, 0);
    card.add_css_class("home-recent-card");
    card.set_hexpand(true);

    let picture = Picture::new();
    picture.add_css_class("home-recent-picture");
    picture.set_content_fit(ContentFit::Cover);
    picture.set_can_shrink(true);
    picture.set_hexpand(true);
    picture.set_vexpand(true);
    picture.set_halign(Align::Fill);
    picture.set_valign(Align::Fill);
    let thumbnail = clips::thumbnail_path(clip, thumbnail_directory);
    if thumbnail.exists() {
        picture.set_filename(Some(&thumbnail));
    }
    let preview = Overlay::new();
    preview.add_css_class("home-recent-preview");
    preview.set_size_request(-1, 112);
    preview.set_overflow(gtk::Overflow::Hidden);
    preview.set_child(Some(&picture));
    let play = Image::from_icon_name("media-playback-start-symbolic");
    play.add_css_class("home-recent-play");
    play.set_pixel_size(12);
    play.set_halign(Align::Center);
    play.set_valign(Align::Center);
    preview.add_overlay(&play);
    card.append(&preview);

    let info = GtkBox::new(Orientation::Vertical, 2);
    info.add_css_class("home-recent-info");
    let title = Label::new(Some(&clip.title));
    title.add_css_class("home-recent-title");
    title.set_halign(Align::Start);
    title.set_ellipsize(pango::EllipsizeMode::End);
    let meta = Label::new(Some(&format!(
        "{} · {}",
        clips::format_age(clip.modified),
        clips::format_size(clip.size_bytes)
    )));
    meta.add_css_class("home-section-detail");
    meta.set_halign(Align::Start);
    info.append(&title);
    info.append(&meta);
    card.append(&info);
    card
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
