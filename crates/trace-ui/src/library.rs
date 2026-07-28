use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use gtk::glib::{self, ControlFlow};
use gtk::pango;
use gtk::prelude::*;
use gtk::{
    Align, AspectFrame, Box as GtkBox, Button, ContentFit, DragSource, DropTarget, Entry, FlowBox,
    FlowBoxChild, Grid, Image, Label, Orientation, Overlay, Picture, Popover, PositionType,
    ScrolledWindow, SelectionMode, Stack, Video,
};
use gtk::{gdk, gio};
use trace_core::clips::{self, Clip, ClipPreview};
use trace_core::config::Config;
use trace_core::paths::AppPaths;

#[derive(Clone)]
pub struct ClipViews {
    pub library: GtkBox,
    pub collections: GtkBox,
    pub player: GtkBox,
    header: Grid,
    heading: GtkBox,
    search: Entry,
    refresh: Button,
    flow: FlowBox,
    collection_flow: FlowBox,
}

impl ClipViews {
    pub fn set_layout(&self, compact: bool, columns: u32) {
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
        self.flow.set_min_children_per_line(columns);
        self.flow.set_max_children_per_line(columns);
        self.collection_flow.set_min_children_per_line(columns);
        self.collection_flow.set_max_children_per_line(columns);
    }

    pub fn refresh(&self) {
        // The directory monitor handles normal updates; navigation also calls this
        // so changes inside nested collection directories appear immediately.
        self.refresh.emit_clicked();
    }
}

struct PreviewUpdate {
    key: PathBuf,
    preview: ClipPreview,
}

#[derive(Clone)]
struct PreviewWidgets {
    picture: Picture,
    duration: Label,
}

struct LibraryState {
    clips: RefCell<Vec<Clip>>,
    directory: PathBuf,
    thumbnail_directory: PathBuf,
    flow: FlowBox,
    empty: GtkBox,
    count: Label,
    search: Entry,
    preview_widgets: RefCell<HashMap<PathBuf, Vec<PreviewWidgets>>>,
    jobs: Vec<mpsc::Sender<Clip>>,
    player: Rc<PlayerState>,
    stack: Stack,
    collection_flow: FlowBox,
    collection_tiles: FlowBox,
    collection_empty: GtkBox,
    collection_title: Label,
    selected_collection: RefCell<Option<PathBuf>>,
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
    let title = Label::new(Some("Library"));
    title.add_css_class("page-title");
    title.set_halign(Align::Start);
    let count = Label::new(Some("Your local moments"));
    count.add_css_class("page-subtitle");
    count.set_halign(Align::Start);
    heading.append(&title);
    heading.append(&count);
    let search = Entry::new();
    search.add_css_class("search");
    search.set_placeholder_text(Some("Search library"));
    search.set_size_request(190, 34);
    let refresh = Button::from_icon_name("view-refresh-symbolic");
    refresh.add_css_class("icon-action");
    refresh.set_tooltip_text(Some("Refresh clips"));
    refresh.set_size_request(34, 34);
    refresh.set_halign(Align::End);
    header.attach(&heading, 0, 0, 1, 1);
    header.attach(&search, 1, 0, 1, 1);
    header.attach(&refresh, 2, 0, 1, 1);
    page.append(&header);

    let flow = FlowBox::new();
    flow.set_selection_mode(SelectionMode::None);
    flow.set_column_spacing(20);
    flow.set_row_spacing(24);
    flow.set_min_children_per_line(1);
    flow.set_max_children_per_line(3);
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

    let (
        collections_page,
        collection_tiles,
        collection_flow,
        collection_empty,
        collection_title,
        create_collection,
    ) = build_collections_page();

    let state = Rc::new(LibraryState {
        clips: RefCell::new(Vec::new()),
        directory: config.storage.directory.clone(),
        thumbnail_directory: paths.thumbnail_dir.clone(),
        flow,
        empty,
        count,
        search,
        preview_widgets: RefCell::new(HashMap::new()),
        jobs: job_senders,
        player,
        stack: stack.clone(),
        collection_flow: collection_flow.clone(),
        collection_tiles,
        collection_empty,
        collection_title,
        selected_collection: RefCell::new(None),
    });

    let preview_state = state.clone();
    glib::timeout_add_local(Duration::from_millis(80), move || {
        while let Ok(update) = preview_receiver.try_recv() {
            if let Some(widget_sets) = preview_state.preview_widgets.borrow().get(&update.key) {
                for widgets in widget_sets {
                    if let Some(path) = update.preview.thumbnail.as_ref() {
                        widgets.picture.set_filename(Some(path));
                    }
                    if let Some(seconds) = update.preview.duration_seconds {
                        widgets.duration.set_text(&clips::format_duration(seconds));
                        widgets.duration.set_visible(true);
                    }
                }
            }
        }
        ControlFlow::Continue
    });

    refresh_all(&state);

    let search_state = state.clone();
    state
        .search
        .connect_changed(move |_| render_clips(&search_state));

    let refresh_state = state.clone();
    let clip_directory = config.storage.directory.clone();
    refresh.connect_clicked(move |_| {
        let _ = &clip_directory;
        refresh_all(&refresh_state);
    });

    let create_state = state.clone();
    create_collection.connect_clicked(move |button| show_create_collection(button, &create_state));

    if let Ok(monitor) = gio::File::for_path(&config.storage.directory)
        .monitor_directory(gio::FileMonitorFlags::NONE, None::<&gio::Cancellable>)
    {
        let monitor = Rc::new(monitor);
        let monitor_keepalive = monitor.clone();
        let monitor_state = state.clone();
        let clip_directory = config.storage.directory.clone();
        monitor.connect_changed(move |_, _, _, _| {
            let _ = &monitor_keepalive;
            let _ = &clip_directory;
            refresh_all(&monitor_state);
        });
    }

    ClipViews {
        library: page,
        collections: collections_page,
        player: player_page,
        header,
        heading,
        search: state.search.clone(),
        refresh,
        flow: state.flow.clone(),
        collection_flow,
    }
}

fn build_collections_page() -> (GtkBox, FlowBox, FlowBox, GtkBox, Label, Button) {
    let page = GtkBox::new(Orientation::Vertical, 0);
    page.add_css_class("collections-page");

    let header = GtkBox::new(Orientation::Horizontal, 18);
    header.set_margin_bottom(24);
    let heading = GtkBox::new(Orientation::Vertical, 3);
    heading.set_hexpand(true);
    let title = Label::new(Some("Collections"));
    title.add_css_class("page-title");
    title.set_halign(Align::Start);
    let subtitle = Label::new(Some("Drag clips into your local folders"));
    subtitle.add_css_class("page-subtitle");
    subtitle.set_halign(Align::Start);
    heading.append(&title);
    heading.append(&subtitle);
    let create = Button::from_icon_name("folder-new-symbolic");
    create.add_css_class("icon-action");
    create.set_tooltip_text(Some("New collection"));
    create.set_size_request(34, 34);
    header.append(&heading);
    header.append(&create);
    page.append(&header);

    let collection_label = Label::new(Some("COLLECTIONS"));
    collection_label.add_css_class("section-title");
    collection_label.set_halign(Align::Start);
    collection_label.set_margin_bottom(10);
    page.append(&collection_label);
    let folders = FlowBox::new();
    folders.add_css_class("collection-folders");
    folders.set_selection_mode(SelectionMode::None);
    folders.set_column_spacing(12);
    folders.set_row_spacing(10);
    folders.set_max_children_per_line(5);
    folders.set_min_children_per_line(1);
    folders.set_homogeneous(false);
    folders.set_margin_bottom(28);
    page.append(&folders);

    let clip_title = Label::new(Some("All clips"));
    clip_title.add_css_class("collection-clip-title");
    clip_title.set_halign(Align::Start);
    clip_title.set_margin_bottom(14);
    page.append(&clip_title);

    let clips = FlowBox::new();
    clips.set_selection_mode(SelectionMode::None);
    clips.set_column_spacing(20);
    clips.set_row_spacing(24);
    clips.set_min_children_per_line(1);
    clips.set_max_children_per_line(3);
    clips.set_homogeneous(true);
    clips.set_valign(Align::Start);
    let scroll = ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_vexpand(true);
    scroll.set_child(Some(&clips));
    page.append(&scroll);

    let empty = GtkBox::new(Orientation::Vertical, 7);
    empty.add_css_class("collection-empty");
    empty.set_vexpand(true);
    empty.set_valign(Align::Center);
    let empty_icon = Image::from_icon_name("folder-videos-symbolic");
    empty_icon.set_pixel_size(32);
    let empty_title = Label::new(Some("No clips in this collection"));
    empty_title.add_css_class("empty-title");
    let empty_detail = Label::new(Some("Drag a clip onto a folder above."));
    empty_detail.add_css_class("empty-detail");
    empty.append(&empty_icon);
    empty.append(&empty_title);
    empty.append(&empty_detail);
    empty.set_visible(false);
    page.append(&empty);

    (page, folders, clips, empty, clip_title, create)
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

fn refresh_all(state: &Rc<LibraryState>) {
    reload_clips(state, &state.directory);
    state.preview_widgets.borrow_mut().clear();
    render_clips(state);
    render_collections(state);
}

fn render_clips(state: &Rc<LibraryState>) {
    while let Some(child) = state.flow.first_child() {
        state.flow.remove(&child);
    }
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
            .entry(clip.path.clone())
            .or_default()
            .push(widgets);
        state.flow.insert(&child, -1);
        let worker = index % state.jobs.len();
        let _ = state.jobs[worker].send(clip);
    }
}

fn render_collections(state: &Rc<LibraryState>) {
    while let Some(child) = state.collection_tiles.first_child() {
        state.collection_tiles.remove(&child);
    }
    while let Some(child) = state.collection_flow.first_child() {
        state.collection_flow.remove(&child);
    }

    let selected_path = state.selected_collection.borrow().clone();
    let all = collection_button("All clips", state.clips.borrow().len(), None, state);
    state.collection_tiles.insert(&all, -1);
    for collection in clips::collections(&state.directory).unwrap_or_default() {
        let button = collection_button(
            &collection.name,
            collection.clip_count,
            Some(collection.path),
            state,
        );
        state.collection_tiles.insert(&button, -1);
    }

    let visible = state
        .clips
        .borrow()
        .iter()
        .filter(|clip| {
            selected_path
                .as_ref()
                .is_none_or(|path| clip.path.parent() == Some(path.as_path()))
        })
        .cloned()
        .collect::<Vec<_>>();
    let heading = selected_path
        .as_ref()
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("All clips");
    state.collection_title.set_text(heading);
    state.collection_empty.set_visible(visible.is_empty());
    state.collection_flow.set_visible(!visible.is_empty());
    for (index, clip) in visible.into_iter().take(200).enumerate() {
        let (child, widgets) = clip_card(&clip, state);
        state
            .preview_widgets
            .borrow_mut()
            .entry(clip.path.clone())
            .or_default()
            .push(widgets);
        state.collection_flow.insert(&child, -1);
        let worker = index % state.jobs.len();
        let _ = state.jobs[worker].send(clip);
    }
}

fn collection_button(
    name: &str,
    count: usize,
    path: Option<PathBuf>,
    state: &Rc<LibraryState>,
) -> FlowBoxChild {
    let row = GtkBox::new(Orientation::Horizontal, 9);
    let icon = Image::from_icon_name(if path.is_some() {
        "folder-symbolic"
    } else {
        "folder-videos-symbolic"
    });
    icon.set_pixel_size(18);
    let labels = GtkBox::new(Orientation::Vertical, 1);
    let title = Label::new(Some(name));
    title.add_css_class("collection-name");
    title.set_halign(Align::Start);
    let detail = Label::new(Some(&format!("{count} clips")));
    detail.add_css_class("collection-count");
    detail.set_halign(Align::Start);
    labels.append(&title);
    labels.append(&detail);
    row.append(&icon);
    row.append(&labels);
    let button = Button::new();
    button.add_css_class("collection-button");
    if *state.selected_collection.borrow() == path {
        button.add_css_class("active");
    }
    button.set_child(Some(&row));
    button.set_tooltip_text(Some("Open collection"));

    let selected = path.clone();
    let selected_state = state.clone();
    button.connect_clicked(move |_| {
        selected_state.selected_collection.replace(selected.clone());
        refresh_all(&selected_state);
    });

    if let Some(collection_path) = path {
        let target = DropTarget::new(String::static_type(), gdk::DragAction::MOVE);
        let drop_state = state.clone();
        target.connect_drop(move |_, value, _, _| {
            let Ok(source) = value.get::<String>() else {
                return false;
            };
            let source = PathBuf::from(source);
            let clip = drop_state
                .clips
                .borrow()
                .iter()
                .find(|clip| clip.path == source)
                .cloned();
            let Some(clip) = clip else {
                return false;
            };
            match clips::move_to_collection(
                &clip,
                &drop_state.directory,
                &collection_path,
                &drop_state.thumbnail_directory,
            ) {
                Ok(_) => {
                    drop_state.count.remove_css_class("error");
                    refresh_all(&drop_state);
                    true
                }
                Err(error) => {
                    drop_state
                        .count
                        .set_text(&format!("Could not move clip: {error}"));
                    drop_state.count.add_css_class("error");
                    false
                }
            }
        });
        button.add_controller(target);
    }

    let child = FlowBoxChild::new();
    child.set_child(Some(&button));
    child
}

fn show_create_collection(button: &Button, state: &Rc<LibraryState>) {
    let popover = Popover::new();
    popover.add_css_class("compact-popover");
    popover.set_position(PositionType::Bottom);
    popover.set_parent(button);
    let content = GtkBox::new(Orientation::Vertical, 10);
    content.add_css_class("compact-popover-content");
    let title = Label::new(Some("New collection"));
    title.add_css_class("delete-confirmation-title");
    title.set_halign(Align::Start);
    let name = Entry::new();
    name.set_placeholder_text(Some("e.g. Funny"));
    name.set_max_length(80);
    let actions = GtkBox::new(Orientation::Horizontal, 8);
    actions.set_halign(Align::End);
    let cancel = Button::with_label("Cancel");
    cancel.add_css_class("popover-action");
    let create = Button::with_label("Create");
    create.add_css_class("primary-action");
    actions.append(&cancel);
    actions.append(&create);
    content.append(&title);
    content.append(&name);
    content.append(&actions);
    popover.set_child(Some(&content));
    popover.connect_closed(|popover| popover.unparent());
    let cancelled = popover.clone();
    cancel.connect_clicked(move |_| cancelled.popdown());
    let created = popover.clone();
    let create_state = state.clone();
    let entered_name = name.clone();
    create.connect_clicked(move |_| {
        match clips::create_collection(&create_state.directory, entered_name.text().as_str()) {
            Ok(path) => {
                create_state.selected_collection.replace(Some(path));
                create_state.count.remove_css_class("error");
                created.popdown();
                refresh_all(&create_state);
            }
            Err(error) => {
                create_state
                    .count
                    .set_text(&format!("Could not create collection: {error}"));
                create_state.count.add_css_class("error");
            }
        }
    });
    popover.popup();
    name.grab_focus();
}

fn clip_card(clip: &Clip, state: &Rc<LibraryState>) -> (FlowBoxChild, PreviewWidgets) {
    let item = Overlay::new();
    item.add_css_class("clip-item");
    item.set_hexpand(true);

    let open = Button::new();
    open.add_css_class("clip-open");
    open.set_hexpand(true);
    open.set_tooltip_text(Some(&clip.path.to_string_lossy()));
    let body = GtkBox::new(Orientation::Vertical, 0);
    let picture = Picture::new();
    picture.add_css_class("clip-preview");
    picture.set_content_fit(ContentFit::Contain);
    picture.set_can_shrink(true);
    picture.set_hexpand(true);
    picture.set_size_request(250, 141);
    let preview_frame = AspectFrame::new(0.5, 0.5, 16.0 / 9.0, false);
    preview_frame.add_css_class("clip-preview-frame");
    preview_frame.set_hexpand(true);
    preview_frame.set_overflow(gtk::Overflow::Hidden);
    preview_frame.set_child(Some(&picture));
    body.append(&preview_frame);

    let text = GtkBox::new(Orientation::Vertical, 3);
    text.set_margin_top(11);
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
    open.set_child(Some(&body));
    item.set_child(Some(&open));

    let rename = Button::from_icon_name("document-edit-symbolic");
    rename.add_css_class("clip-rename");
    rename.set_tooltip_text(Some("Rename clip"));
    rename.set_halign(Align::End);
    rename.set_valign(Align::Start);
    rename.set_margin_top(7);
    rename.set_margin_end(41);
    rename.set_size_request(34, 34);
    item.add_overlay(&rename);

    let delete = Button::from_icon_name("user-trash-symbolic");
    delete.add_css_class("clip-delete");
    delete.set_tooltip_text(Some("Delete clip"));
    delete.set_halign(Align::End);
    delete.set_valign(Align::Start);
    delete.set_margin_top(7);
    delete.set_margin_end(7);
    delete.set_size_request(34, 34);
    item.add_overlay(&delete);

    let selected = clip.clone();
    let player = state.player.clone();
    let stack = state.stack.clone();
    open.connect_clicked(move |_| {
        show_player(&player, &selected);
        stack.set_visible_child_name("player");
    });
    let deleted = clip.clone();
    let delete_state = state.clone();
    delete.connect_clicked(move |button| confirm_delete(button, &deleted, &delete_state));
    let renamed = clip.clone();
    let rename_state = state.clone();
    rename.connect_clicked(move |button| show_rename(button, &renamed, &rename_state));

    let drag = DragSource::new();
    drag.set_actions(gdk::DragAction::MOVE);
    let dragged_path = clip.path.to_string_lossy().into_owned();
    drag.connect_prepare(move |_, _, _| {
        Some(gdk::ContentProvider::for_value(&dragged_path.to_value()))
    });
    item.add_controller(drag);

    let child = FlowBoxChild::new();
    child.set_child(Some(&item));
    (child, PreviewWidgets { picture, duration })
}

fn show_rename(button: &Button, clip: &Clip, state: &Rc<LibraryState>) {
    let popover = Popover::new();
    popover.add_css_class("compact-popover");
    popover.set_position(PositionType::Bottom);
    popover.set_parent(button);
    let content = GtkBox::new(Orientation::Vertical, 10);
    content.add_css_class("compact-popover-content");
    let title = Label::new(Some("Rename clip"));
    title.add_css_class("delete-confirmation-title");
    title.set_halign(Align::Start);
    let name = Entry::new();
    let current_name = clip
        .path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(&clip.title);
    name.set_text(current_name);
    name.set_max_length(80);
    name.select_region(0, -1);
    let actions = GtkBox::new(Orientation::Horizontal, 8);
    actions.set_halign(Align::End);
    let cancel = Button::with_label("Cancel");
    cancel.add_css_class("popover-action");
    let save = Button::with_label("Rename");
    save.add_css_class("primary-action");
    actions.append(&cancel);
    actions.append(&save);
    content.append(&title);
    content.append(&name);
    content.append(&actions);
    popover.set_child(Some(&content));
    popover.connect_closed(|popover| popover.unparent());
    let cancelled = popover.clone();
    cancel.connect_clicked(move |_| cancelled.popdown());
    let renamed = popover.clone();
    let selected = clip.clone();
    let rename_state = state.clone();
    let entered_name = name.clone();
    save.connect_clicked(move |_| {
        match clips::rename(
            &selected,
            entered_name.text().as_str(),
            &rename_state.thumbnail_directory,
        ) {
            Ok(_) => {
                rename_state.count.remove_css_class("error");
                renamed.popdown();
                refresh_all(&rename_state);
            }
            Err(error) => {
                rename_state
                    .count
                    .set_text(&format!("Could not rename clip: {error}"));
                rename_state.count.add_css_class("error");
            }
        }
    });
    popover.popup();
    name.grab_focus();
}

fn confirm_delete(button: &Button, clip: &Clip, state: &Rc<LibraryState>) {
    let popover = Popover::new();
    popover.add_css_class("delete-confirmation");
    popover.set_position(PositionType::Bottom);
    popover.set_autohide(true);
    popover.set_parent(button);

    let content = GtkBox::new(Orientation::Vertical, 5);
    content.add_css_class("delete-confirmation-content");
    let title = Label::new(Some("Delete clip?"));
    title.add_css_class("delete-confirmation-title");
    title.set_halign(Align::Start);
    let detail = Label::new(Some(&clip.title));
    detail.add_css_class("delete-confirmation-detail");
    detail.set_halign(Align::Start);
    detail.set_ellipsize(pango::EllipsizeMode::Middle);
    detail.set_max_width_chars(32);
    let warning = Label::new(Some("This permanently removes the local file."));
    warning.add_css_class("delete-confirmation-warning");
    warning.set_halign(Align::Start);
    warning.set_margin_top(2);
    let actions = GtkBox::new(Orientation::Horizontal, 8);
    actions.set_halign(Align::End);
    actions.set_margin_top(9);
    let cancel = Button::with_label("Cancel");
    cancel.add_css_class("popover-action");
    let confirm = Button::with_label("Delete");
    confirm.add_css_class("danger-action");
    actions.append(&cancel);
    actions.append(&confirm);
    content.append(&title);
    content.append(&detail);
    content.append(&warning);
    content.append(&actions);
    popover.set_child(Some(&content));
    popover.connect_closed(|popover| popover.unparent());

    let cancelled = popover.clone();
    cancel.connect_clicked(move |_| cancelled.popdown());

    let deleted = clip.clone();
    let delete_state = state.clone();
    let confirmed = popover.clone();
    confirm.connect_clicked(move |_| {
        confirmed.popdown();
        if let Err(error) = clips::delete(&deleted, &delete_state.thumbnail_directory) {
            delete_state
                .count
                .set_text(&format!("Could not delete clip: {error}"));
            delete_state.count.add_css_class("error");
        } else {
            delete_state.count.remove_css_class("error");
            refresh_all(&delete_state);
        }
    });
    popover.popup();
}

fn empty_state(directory: &std::path::Path) -> GtkBox {
    let empty = GtkBox::new(Orientation::Vertical, 8);
    empty.add_css_class("empty-state");
    empty.set_vexpand(true);
    empty.set_valign(Align::Center);
    let icon = Image::from_icon_name("folder-videos-symbolic");
    icon.set_pixel_size(38);
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
    let back = Button::with_label("←  Library");
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
        back_stack.set_visible_child_name("library");
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
