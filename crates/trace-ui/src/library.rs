use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use gtk::gio;
use gtk::glib::{self, ControlFlow};
use gtk::pango;
use gtk::prelude::*;
use gtk::{
    Align, Box as GtkBox, Button, ContentFit, Entry, FlowBox, FlowBoxChild, Grid, Label,
    Orientation, Picture, ScrolledWindow, SelectionMode, Stack, Video,
};
use trace_core::clips::{self, Clip, ClipPreview};
use trace_core::config::Config;
use trace_core::paths::AppPaths;

#[derive(Clone)]
pub struct ClipViews {
    pub library: GtkBox,
    pub player: GtkBox,
    header: Grid,
    heading: GtkBox,
    search: Entry,
    refresh: Button,
}

impl ClipViews {
    pub fn set_compact(&self, compact: bool) {
        self.header.remove(&self.heading);
        self.header.remove(&self.search);
        self.header.remove(&self.refresh);
        if compact {
            self.header.set_row_spacing(12);
            self.header.attach(&self.heading, 0, 0, 2, 1);
            self.header.attach(&self.search, 0, 1, 1, 1);
            self.header.attach(&self.refresh, 1, 1, 1, 1);
            self.search.set_hexpand(true);
        } else {
            self.header.set_row_spacing(0);
            self.header.attach(&self.heading, 0, 0, 1, 1);
            self.header.attach(&self.search, 1, 0, 1, 1);
            self.header.attach(&self.refresh, 2, 0, 1, 1);
            self.search.set_hexpand(false);
        }
    }
}

struct PreviewUpdate {
    key: PathBuf,
    preview: ClipPreview,
}

struct PreviewWidgets {
    picture: Picture,
    duration: Label,
}

struct LibraryState {
    clips: RefCell<Vec<Clip>>,
    flow: FlowBox,
    empty: GtkBox,
    count: Label,
    search: Entry,
    preview_widgets: RefCell<HashMap<PathBuf, PreviewWidgets>>,
    jobs: Vec<mpsc::Sender<Clip>>,
    player: Rc<PlayerState>,
    stack: Stack,
}

struct PlayerState {
    video: Video,
    title: Label,
    meta: Label,
    current: RefCell<Option<PathBuf>>,
}

pub fn build(stack: &Stack) -> ClipViews {
    let paths = AppPaths::discover();
    let config = Config::load(&paths).unwrap_or_default();
    let _ = std::fs::create_dir_all(&config.storage.directory);
    let (player_page, player) = build_player(stack);
    let (preview_sender, preview_receiver) = mpsc::channel::<PreviewUpdate>();
    let mut job_senders = Vec::with_capacity(2);
    for index in 0..2 {
        let (jobs, job_receiver) = mpsc::channel::<Clip>();
        job_senders.push(jobs);
        let updates = preview_sender.clone();
        let thumbnails = paths.thumbnail_dir.clone();
        let _ = std::thread::Builder::new()
            .name(format!("trace-thumbnail-{index}"))
            .spawn(move || {
                while let Ok(clip) = job_receiver.recv() {
                    let preview = clips::build_preview(&clip, &thumbnails);
                    let _ = updates.send(PreviewUpdate {
                        key: clip.path,
                        preview,
                    });
                }
            });
    }

    let page = GtkBox::new(Orientation::Vertical, 0);
    page.add_css_class("clips-page");

    let header = Grid::new();
    header.set_column_spacing(16);
    header.set_margin_bottom(26);
    let heading = GtkBox::new(Orientation::Vertical, 3);
    heading.set_hexpand(true);
    let title = Label::new(Some("Clips"));
    title.add_css_class("page-title");
    title.set_halign(Align::Start);
    let count = Label::new(Some("Your local moments"));
    count.add_css_class("page-subtitle");
    count.set_halign(Align::Start);
    heading.append(&title);
    heading.append(&count);
    let search = Entry::new();
    search.add_css_class("search");
    search.set_placeholder_text(Some("Search clips"));
    search.set_size_request(210, 40);
    let refresh = Button::with_label("↻");
    refresh.add_css_class("icon-action");
    refresh.set_tooltip_text(Some("Refresh clips"));
    refresh.set_size_request(42, 40);
    refresh.set_halign(Align::End);
    header.attach(&heading, 0, 0, 1, 1);
    header.attach(&search, 1, 0, 1, 1);
    header.attach(&refresh, 2, 0, 1, 1);
    page.append(&header);

    let flow = FlowBox::new();
    flow.set_selection_mode(SelectionMode::None);
    flow.set_column_spacing(18);
    flow.set_row_spacing(22);
    flow.set_min_children_per_line(1);
    flow.set_max_children_per_line(4);
    flow.set_homogeneous(true);
    flow.set_valign(Align::Start);

    let scroll = ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_vexpand(true);
    scroll.set_child(Some(&flow));
    page.append(&scroll);

    let empty = empty_state(&config.storage.directory);
    empty.set_visible(false);
    page.append(&empty);

    let state = Rc::new(LibraryState {
        clips: RefCell::new(Vec::new()),
        flow,
        empty,
        count,
        search,
        preview_widgets: RefCell::new(HashMap::new()),
        jobs: job_senders,
        player,
        stack: stack.clone(),
    });

    let preview_state = state.clone();
    glib::timeout_add_local(Duration::from_millis(80), move || {
        while let Ok(update) = preview_receiver.try_recv() {
            if let Some(widgets) = preview_state.preview_widgets.borrow().get(&update.key) {
                if let Some(path) = update.preview.thumbnail {
                    widgets.picture.set_filename(Some(path));
                }
                if let Some(seconds) = update.preview.duration_seconds {
                    widgets.duration.set_text(&clips::format_duration(seconds));
                    widgets.duration.set_visible(true);
                }
            }
        }
        ControlFlow::Continue
    });

    reload_clips(&state, &config.storage.directory);
    render_clips(&state);

    let search_state = state.clone();
    state
        .search
        .connect_changed(move |_| render_clips(&search_state));

    let refresh_state = state.clone();
    let clip_directory = config.storage.directory.clone();
    refresh.connect_clicked(move |_| {
        reload_clips(&refresh_state, &clip_directory);
        render_clips(&refresh_state);
    });

    if let Ok(monitor) = gio::File::for_path(&config.storage.directory)
        .monitor_directory(gio::FileMonitorFlags::NONE, None::<&gio::Cancellable>)
    {
        let monitor = Rc::new(monitor);
        let monitor_keepalive = monitor.clone();
        let monitor_state = state.clone();
        let clip_directory = config.storage.directory.clone();
        monitor.connect_changed(move |_, _, _, _| {
            let _ = &monitor_keepalive;
            reload_clips(&monitor_state, &clip_directory);
            render_clips(&monitor_state);
        });
    }

    ClipViews {
        library: page,
        player: player_page,
        header,
        heading,
        search: state.search.clone(),
        refresh,
    }
}

fn reload_clips(state: &LibraryState, directory: &std::path::Path) {
    let loaded = clips::scan(directory).unwrap_or_default();
    let count = match loaded.len() {
        0 => "No local clips yet".to_owned(),
        1 => "1 local clip".to_owned(),
        count => format!("{count} local clips"),
    };
    state.count.set_text(&count);
    state.clips.replace(loaded);
}

fn render_clips(state: &Rc<LibraryState>) {
    while let Some(child) = state.flow.first_child() {
        state.flow.remove(&child);
    }
    state.preview_widgets.borrow_mut().clear();
    let query = state.search.text().to_ascii_lowercase();
    let clips = state
        .clips
        .borrow()
        .iter()
        .filter(|clip| query.is_empty() || clip.title.to_ascii_lowercase().contains(&query))
        .cloned()
        .collect::<Vec<_>>();
    state.empty.set_visible(clips.is_empty());
    state.flow.set_visible(!clips.is_empty());
    for (index, clip) in clips.into_iter().take(200).enumerate() {
        let (child, widgets) = clip_card(&clip, state);
        state
            .preview_widgets
            .borrow_mut()
            .insert(clip.path.clone(), widgets);
        state.flow.insert(&child, -1);
        let worker = index % state.jobs.len();
        let _ = state.jobs[worker].send(clip);
    }
}

fn clip_card(clip: &Clip, state: &Rc<LibraryState>) -> (FlowBoxChild, PreviewWidgets) {
    let card = Button::new();
    card.add_css_class("clip-card");
    card.set_tooltip_text(Some(&clip.path.to_string_lossy()));
    let body = GtkBox::new(Orientation::Vertical, 0);
    let picture = Picture::new();
    picture.add_css_class("clip-preview");
    picture.set_content_fit(ContentFit::Cover);
    picture.set_can_shrink(true);
    picture.set_size_request(250, 142);
    body.append(&picture);

    let text = GtkBox::new(Orientation::Vertical, 2);
    text.set_margin_top(10);
    let title = Label::new(Some(&clip.title));
    title.add_css_class("clip-title");
    title.set_halign(Align::Start);
    title.set_ellipsize(pango::EllipsizeMode::End);
    title.set_max_width_chars(26);
    let metadata = GtkBox::new(Orientation::Horizontal, 7);
    let age = Label::new(Some(&clips::format_age(clip.modified)));
    age.add_css_class("clip-meta");
    let separator = Label::new(Some("·"));
    separator.add_css_class("clip-meta");
    let size = Label::new(Some(&clips::format_size(clip.size_bytes)));
    size.add_css_class("clip-meta");
    let duration = Label::new(None);
    duration.add_css_class("duration");
    duration.set_hexpand(true);
    duration.set_halign(Align::End);
    duration.set_visible(false);
    metadata.append(&age);
    metadata.append(&separator);
    metadata.append(&size);
    metadata.append(&duration);
    text.append(&title);
    text.append(&metadata);
    body.append(&text);
    card.set_child(Some(&body));

    let selected = clip.clone();
    let player = state.player.clone();
    let stack = state.stack.clone();
    card.connect_clicked(move |_| {
        show_player(&player, &selected);
        stack.set_visible_child_name("player");
    });
    let child = FlowBoxChild::new();
    child.set_child(Some(&card));
    (child, PreviewWidgets { picture, duration })
}

fn empty_state(directory: &std::path::Path) -> GtkBox {
    let empty = GtkBox::new(Orientation::Vertical, 8);
    empty.add_css_class("empty-state");
    empty.set_vexpand(true);
    empty.set_valign(Align::Center);
    let icon = Label::new(Some("◫"));
    icon.add_css_class("empty-icon");
    let title = Label::new(Some("No clips yet"));
    title.add_css_class("empty-title");
    let detail = Label::new(Some(&format!(
        "Press your Trace hotkey to save a moment.\nClips appear from {}",
        directory.display()
    )));
    detail.add_css_class("empty-detail");
    detail.set_justify(gtk::Justification::Center);
    detail.set_wrap(true);
    detail.set_wrap_mode(pango::WrapMode::WordChar);
    detail.set_max_width_chars(44);
    empty.append(&icon);
    empty.append(&title);
    empty.append(&detail);
    empty
}

fn build_player(stack: &Stack) -> (GtkBox, Rc<PlayerState>) {
    let page = GtkBox::new(Orientation::Vertical, 0);
    page.add_css_class("player-page");

    let header = GtkBox::new(Orientation::Horizontal, 14);
    header.set_margin_bottom(20);
    let back = Button::with_label("←  Clips");
    back.add_css_class("back-action");
    let titles = GtkBox::new(Orientation::Vertical, 2);
    titles.set_hexpand(true);
    let title = Label::new(Some("Clip"));
    title.add_css_class("player-title");
    title.set_halign(Align::Start);
    title.set_ellipsize(pango::EllipsizeMode::End);
    let meta = Label::new(None);
    meta.add_css_class("page-subtitle");
    meta.set_halign(Align::Start);
    titles.append(&title);
    titles.append(&meta);
    let reveal = Button::with_label("Open folder");
    reveal.add_css_class("secondary-action");
    header.append(&back);
    header.append(&titles);
    header.append(&reveal);
    page.append(&header);

    let video = Video::new();
    video.add_css_class("video-stage");
    video.set_hexpand(true);
    video.set_vexpand(true);
    video.set_autoplay(true);
    video.set_loop(false);
    page.append(&video);

    let player = Rc::new(PlayerState {
        video,
        title,
        meta,
        current: RefCell::new(None),
    });

    let back_player = player.clone();
    let back_stack = stack.clone();
    back.connect_clicked(move |_| {
        back_player.video.set_file(None::<&gio::File>);
        back_player.current.replace(None);
        back_stack.set_visible_child_name("clips");
    });

    let reveal_player = player.clone();
    reveal.connect_clicked(move |_| {
        let Some(path) = reveal_player.current.borrow().clone() else {
            return;
        };
        let Some(parent) = path.parent() else {
            return;
        };
        let _ = Command::new("xdg-open")
            .arg(parent)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    });
    (page, player)
}

fn show_player(player: &PlayerState, clip: &Clip) {
    player.title.set_text(&clip.title);
    player.meta.set_text(&format!(
        "{}  ·  {}",
        clips::format_age(clip.modified),
        clips::format_size(clip.size_bytes)
    ));
    player.current.replace(Some(clip.path.clone()));
    let file = gio::File::for_path(&clip.path);
    player.video.set_file(Some(&file));
}
