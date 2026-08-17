use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use gtk::glib::{self, ControlFlow};
use gtk::pango;
use gtk::prelude::*;
use gtk::{
    Align, AspectFrame, Box as GtkBox, Button, ContentFit, DragSource, DropTarget, Entry,
    EventControllerKey, EventControllerMotion, FlowBox, FlowBoxChild, GestureClick, Grid, Image,
    Label, MediaFile, Orientation, Overlay, Picture, Popover, PositionType, Scale, ScrolledWindow,
    SelectionMode, Stack,
};
use gtk::{gdk, gio};
use wreath_core::clips::{self, Clip, ClipPreview};
use wreath_core::config::Config;
use wreath_core::paths::AppPaths;

#[derive(Clone)]
pub struct ClipViews {
    pub library: GtkBox,
    pub collections: GtkBox,
    pub player: GtkBox,
    pub editor: GtkBox,
    header: Grid,
    heading: GtkBox,
    toolbar: GtkBox,
    search: Entry,
    refresh: Button,
    flow: FlowBox,
    collection_flow: FlowBox,
    collections_sidebar: GtkBox,
    player_controller: Rc<PlayerState>,
    editor_controller: Rc<crate::editor::EditorController>,
    state: Rc<LibraryState>,
}

impl ClipViews {
    pub fn search(&self) -> Entry {
        self.search.clone()
    }

    pub fn toggle_playback(&self, page: &str) -> bool {
        match page {
            "player" => self.player_controller.toggle_playback(),
            "editor" => self.editor_controller.toggle_playback(),
            _ => false,
        }
    }

    pub fn set_layout(&self, compact: bool, columns: u32, collection_columns: u32) {
        self.header.remove(&self.heading);
        self.header.remove(&self.toolbar);
        if compact {
            self.header.set_row_spacing(12);
            self.header.attach(&self.heading, 0, 0, 2, 1);
            self.header.attach(&self.toolbar, 0, 1, 2, 1);
            self.toolbar.set_halign(Align::Start);
        } else {
            self.header.set_row_spacing(0);
            self.header.attach(&self.heading, 0, 0, 1, 1);
            self.header.attach(&self.toolbar, 1, 0, 1, 1);
            self.toolbar.set_halign(Align::End);
        }
        self.toolbar.set_margin_top(if compact { 0 } else { 25 });
        self.state
            .collection_controls
            .root
            .set_margin_top(if compact { 0 } else { 25 });
        self.flow.set_min_children_per_line(columns);
        self.flow.set_max_children_per_line(columns);
        self.collection_flow
            .set_min_children_per_line(collection_columns);
        self.collection_flow
            .set_max_children_per_line(collection_columns);
        self.collections_sidebar
            .set_size_request(if compact { 210 } else { 240 }, -1);
    }

    pub fn update_preview_geometry(&self, columns: u32, collection_columns: u32) {
        set_preview_heights(
            &self.state.library_aspects.borrow(),
            self.flow.width(),
            columns,
        );
        set_preview_heights(
            &self.state.collection_aspects.borrow(),
            self.collection_flow.width(),
            collection_columns,
        );
    }

    pub fn refresh(&self) {
        self.refresh.emit_clicked();
    }

    pub fn clear_selection(&self) {
        clear_selection(&self.state);
    }

    pub fn exit_player_fullscreen(&self) -> bool {
        self.player_controller.exit_fullscreen()
    }
}

struct PreviewUpdate {
    key: PathBuf,
    preview: ClipPreview,
}

#[derive(Clone)]
struct PreviewWidgets {
    picture: Picture,
    fallback: Label,
    aspect: AspectFrame,
}

struct LibraryState {
    clips: RefCell<Vec<Clip>>,
    directory: RefCell<PathBuf>,
    directory_monitor: RefCell<Option<gio::FileMonitor>>,
    thumbnail_directory: PathBuf,
    flow: FlowBox,
    scroll: ScrolledWindow,
    empty: GtkBox,
    empty_label: Label,
    count: Label,
    library_meta: Label,
    search: Entry,
    preview_widgets: RefCell<HashMap<PathBuf, Vec<PreviewWidgets>>>,
    library_aspects: RefCell<Vec<AspectFrame>>,
    collection_aspects: RefCell<Vec<AspectFrame>>,
    jobs: Vec<mpsc::Sender<Clip>>,
    player: Rc<PlayerState>,
    editor: Rc<crate::editor::EditorController>,
    stack: Stack,
    collection_flow: FlowBox,
    collection_tiles: FlowBox,
    collection_empty: Label,
    collection_title: Label,
    delete_collection: Button,
    selected_collection: RefCell<Option<PathBuf>>,
    selection_mode: Cell<bool>,
    selected_clips: RefCell<HashSet<PathBuf>>,
    library_controls: SelectionControls,
    collection_controls: SelectionControls,
    on_library_change: Rc<dyn Fn()>,
}

#[derive(Clone)]
struct SelectionControls {
    root: GtkBox,
    browse_actions: GtkBox,
    selection_actions: GtkBox,
    select: Button,
    cancel: Button,
    select_all: Button,
    move_selected: Button,
    refresh: Option<Button>,
}

#[derive(Clone, Copy)]
enum SelectionScope {
    Library,
    Collection,
}

struct CollectionsPage {
    page: GtkBox,
    tiles: FlowBox,
    flow: FlowBox,
    empty: Label,
    sidebar: GtkBox,
    title: Label,
    create: Button,
    delete: Button,
    controls: SelectionControls,
}

struct PlayerState {
    media: MediaFile,
    stage: Picture,
    fullscreen_root: Overlay,
    fullscreen_window: RefCell<Option<glib::WeakRef<gtk::Window>>>,
    windowed_child: RefCell<Option<gtk::Widget>>,
    title: Label,
    meta: Label,
    current: RefCell<Option<Clip>>,
    library_directory: RefCell<PathBuf>,
    playlist: RefCell<Vec<Clip>>,
    current_index: Cell<Option<usize>>,
    return_page: RefCell<String>,
    previous: Button,
    next: Button,
    play_pause: Button,
    play_pause_icon: Image,
    timeline: Scale,
    time: Label,
    volume: Scale,
    volume_value: Label,
    mute: Button,
    mute_icon: Image,
    fullscreen: Button,
    fullscreen_controls: GtkBox,
    fullscreen_previous: Button,
    fullscreen_next: Button,
    fullscreen_play_pause: Button,
    fullscreen_play_pause_icon: Image,
    fullscreen_timeline: Scale,
    fullscreen_time: Label,
    fullscreen_volume: Scale,
    fullscreen_mute: Button,
    fullscreen_mute_icon: Image,
    fullscreen_hide_source: RefCell<Option<glib::SourceId>>,
    updating_timeline: Cell<bool>,
    updating_volume: Cell<bool>,
    last_audible_volume: Cell<f64>,
}

impl PlayerState {
    fn stop(&self) {
        self.media.pause();
        self.media.clear();
        self.current.replace(None);
        self.playlist.borrow_mut().clear();
        self.current_index.set(None);
        self.exit_fullscreen();
    }

    fn toggle_playback(&self) -> bool {
        if self.current.borrow().is_none() {
            return false;
        }
        if self.media.is_playing() {
            self.media.pause();
        } else {
            self.media.play();
        }
        self.update_controls();
        true
    }

    fn show(&self, clip: &Clip, playlist: Vec<Clip>, return_page: &str) {
        let index = playlist
            .iter()
            .position(|candidate| candidate.path == clip.path)
            .unwrap_or(0);
        self.playlist.replace(playlist);
        self.return_page.replace(return_page.to_owned());
        self.show_index(index);
    }

    fn show_index(&self, index: usize) {
        let Some(clip) = self.playlist.borrow().get(index).cloned() else {
            return;
        };
        self.title.set_text(&clip.title);
        self.meta.set_text(&format!(
            "{}  •  {}",
            clips::format_age(clip.modified),
            clips::format_size(clip.size_bytes)
        ));
        self.current.replace(Some(clip.clone()));
        self.current_index.set(Some(index));
        self.media.set_file(Some(&gio::File::for_path(&clip.path)));
        self.apply_audio();
        self.media.play();
        self.update_controls();
    }

    fn step(&self, offset: isize) {
        let Some(index) = self.current_index.get() else {
            return;
        };
        let Some(next) = index.checked_add_signed(offset) else {
            return;
        };
        self.show_index(next);
    }

    fn update_controls(&self) {
        let index = self.current_index.get();
        let count = self.playlist.borrow().len();
        self.previous
            .set_sensitive(index.is_some_and(|value| value > 0));
        self.fullscreen_previous
            .set_sensitive(index.is_some_and(|value| value > 0));
        self.next
            .set_sensitive(index.is_some_and(|value| value + 1 < count));
        self.fullscreen_next
            .set_sensitive(index.is_some_and(|value| value + 1 < count));
        let playing = self.media.is_playing();
        let playback_icon = if playing {
            "media-playback-pause-symbolic"
        } else {
            "media-playback-start-symbolic"
        };
        let playback_label = if playing { "Pause" } else { "Play" };
        self.play_pause_icon.set_icon_name(Some(playback_icon));
        self.fullscreen_play_pause_icon
            .set_icon_name(Some(playback_icon));
        self.play_pause
            .update_property(&[gtk::accessible::Property::Label(playback_label)]);
        self.fullscreen_play_pause
            .update_property(&[gtk::accessible::Property::Label(playback_label)]);

        let duration = media_seconds(self.media.duration());
        let position = media_seconds(self.media.timestamp()).min(duration);
        self.updating_timeline.set(true);
        self.timeline.set_range(0.0, duration.max(0.001));
        self.timeline.set_value(position);
        self.fullscreen_timeline.set_range(0.0, duration.max(0.001));
        self.fullscreen_timeline.set_value(position);
        self.updating_timeline.set(false);
        let time = format!(
            "{} / {}",
            format_player_time(position),
            format_player_time(duration)
        );
        self.time.set_text(&time);
        self.fullscreen_time.set_text(&time);
        let timeline_properties = [
            gtk::accessible::Property::Label("Playback position"),
            gtk::accessible::Property::ValueText(&format!(
                "{} of {}",
                format_player_time(position),
                format_player_time(duration)
            )),
        ];
        self.timeline.update_property(&timeline_properties);
        self.fullscreen_timeline
            .update_property(&timeline_properties);
        self.updating_volume.set(true);
        self.fullscreen_volume.set_value(self.volume.value());
        self.updating_volume.set(false);
        self.volume_value
            .set_text(&format!("{}%", self.volume.value().round() as u8));
        let muted = self.volume.value() <= 0.0;
        let volume_icon = if muted {
            "audio-volume-muted-symbolic"
        } else {
            "audio-volume-high-symbolic"
        };
        let volume_label = if muted {
            "Unmute player"
        } else {
            "Mute player"
        };
        self.mute_icon.set_icon_name(Some(volume_icon));
        self.fullscreen_mute_icon.set_icon_name(Some(volume_icon));
        let volume_properties = [
            gtk::accessible::Property::Label("Player volume"),
            gtk::accessible::Property::ValueText(&format!(
                "{} percent",
                self.volume.value().round() as u8
            )),
        ];
        self.volume.update_property(&volume_properties);
        self.fullscreen_volume.update_property(&volume_properties);
        self.mute
            .update_property(&[gtk::accessible::Property::Label(volume_label)]);
        self.fullscreen_mute
            .update_property(&[gtk::accessible::Property::Label(volume_label)]);
    }

    fn toggle_mute(&self) {
        if self.volume.value() <= 0.0 {
            self.volume
                .set_value(self.last_audible_volume.get().max(1.0));
        } else {
            self.last_audible_volume.set(self.volume.value());
            self.volume.set_value(0.0);
        }
    }

    fn apply_audio(&self) {
        let (volume, muted) = player_audio_settings(self.volume.value());
        self.media.set_volume(volume);
        self.media.set_muted(muted);
    }

    fn toggle_fullscreen(&self) {
        if self.windowed_child.borrow().is_some() {
            self.exit_fullscreen();
            return;
        }

        let Some(window) = self
            .stage
            .root()
            .and_then(|root| root.downcast::<gtk::Window>().ok())
        else {
            return;
        };
        let Some(windowed_child) = window.child() else {
            return;
        };

        self.windowed_child.replace(Some(windowed_child));
        self.fullscreen_window.replace(Some(window.downgrade()));
        window.set_child(Some(&self.fullscreen_root));
        window.fullscreen();
        self.fullscreen.set_tooltip_text(Some("Exit fullscreen"));
        self.fullscreen
            .update_property(&[gtk::accessible::Property::Label("Exit fullscreen")]);
    }

    fn exit_fullscreen(&self) -> bool {
        let window = self
            .fullscreen_window
            .borrow_mut()
            .take()
            .and_then(|window| window.upgrade());
        let windowed_child = self.windowed_child.borrow_mut().take();
        let (Some(window), Some(windowed_child)) = (window, windowed_child) else {
            return false;
        };

        window.unfullscreen();
        window.set_child(Some(&windowed_child));
        if let Some(source) = self.fullscreen_hide_source.borrow_mut().take() {
            source.remove();
        }
        self.fullscreen_controls.set_visible(true);
        self.fullscreen.set_tooltip_text(Some("Enter fullscreen"));
        self.fullscreen
            .update_property(&[gtk::accessible::Property::Label("Enter fullscreen")]);
        true
    }
}

fn reveal_fullscreen_controls(player: &Rc<PlayerState>) {
    if player.windowed_child.borrow().is_none() {
        return;
    }
    player.fullscreen_controls.set_visible(true);
    if let Some(source) = player.fullscreen_hide_source.borrow_mut().take() {
        source.remove();
    }
    let weak_player = Rc::downgrade(player);
    let source = glib::timeout_add_local_once(Duration::from_secs(3), move || {
        if let Some(player) = weak_player.upgrade()
            && player.windowed_child.borrow().is_some()
        {
            player.fullscreen_controls.set_visible(false);
            player.fullscreen_hide_source.borrow_mut().take();
        }
    });
    player.fullscreen_hide_source.replace(Some(source));
}

fn media_seconds(microseconds: i64) -> f64 {
    microseconds.max(0) as f64 / 1_000_000.0
}

fn player_audio_settings(percent: f64) -> (f64, bool) {
    let volume = percent.clamp(0.0, 100.0) / 100.0;
    (volume, volume <= f64::EPSILON)
}

fn format_player_time(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as u64;
    format!("{:02}:{:02}", total / 60, total % 60)
}

pub fn build(stack: &Stack, on_library_change: impl Fn() + 'static) -> ClipViews {
    let paths = AppPaths::discover();
    let config = Config::load(&paths).unwrap_or_default();
    let _ = std::fs::create_dir_all(&config.storage.directory);
    let editor_view = crate::editor::build(stack);
    let (player_page, player) =
        build_player(stack, &editor_view.controller, &config.storage.directory);
    let (preview_sender, preview_receiver) = mpsc::channel::<PreviewUpdate>();
    let mut job_senders = Vec::with_capacity(2);
    for index in 0..2 {
        let (jobs, job_receiver) = mpsc::channel::<Clip>();
        job_senders.push(jobs);
        let updates = preview_sender.clone();
        let thumbnails = paths.thumbnail_dir.clone();
        let _ = std::thread::Builder::new()
            .name(format!("wreath-thumbnail-{index}"))
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
    let heading = GtkBox::new(Orientation::Vertical, 8);
    heading.set_hexpand(true);
    let title = Label::new(Some("Library"));
    title.add_css_class("page-title");
    title.set_halign(Align::Start);
    let count = Label::new(Some("Local replays"));
    count.add_css_class("page-subtitle");
    count.set_halign(Align::Start);
    heading.append(&count);
    heading.append(&title);
    let search = Entry::new();
    search.add_css_class("search");
    search.set_placeholder_text(Some("Search clips"));
    search.set_size_request(190, 34);
    let library_controls = selection_controls(true);
    header.attach(&heading, 0, 0, 1, 1);
    header.attach(&library_controls.root, 1, 0, 1, 1);
    page.append(&header);

    let library_meta = Label::new(Some("0 clips  •  0 B"));
    library_meta.add_css_class("library-meta");
    library_meta.set_halign(Align::Start);
    library_meta.set_margin_bottom(12);
    page.append(&library_meta);

    let flow = FlowBox::new();
    flow.set_selection_mode(SelectionMode::None);
    flow.set_column_spacing(12);
    flow.set_row_spacing(12);
    flow.set_min_children_per_line(1);
    flow.set_max_children_per_line(4);
    flow.set_homogeneous(true);
    flow.set_valign(Align::Start);

    let scroll = ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_vexpand(true);
    scroll.set_child(Some(&flow));
    page.append(&scroll);

    let (empty, empty_label) = empty_state();
    empty.set_visible(false);
    page.append(&empty);

    let collections_page = build_collections_page();

    let state = Rc::new(LibraryState {
        clips: RefCell::new(Vec::new()),
        directory: RefCell::new(config.storage.directory.clone()),
        directory_monitor: RefCell::new(None),
        thumbnail_directory: paths.thumbnail_dir.clone(),
        flow,
        scroll,
        empty,
        empty_label,
        count,
        library_meta,
        search,
        preview_widgets: RefCell::new(HashMap::new()),
        library_aspects: RefCell::new(Vec::new()),
        collection_aspects: RefCell::new(Vec::new()),
        jobs: job_senders,
        player,
        editor: editor_view.controller.clone(),
        stack: stack.clone(),
        collection_flow: collections_page.flow.clone(),
        collection_tiles: collections_page.tiles.clone(),
        collection_empty: collections_page.empty.clone(),
        collection_title: collections_page.title.clone(),
        delete_collection: collections_page.delete.clone(),
        selected_collection: RefCell::new(None),
        selection_mode: Cell::new(false),
        selected_clips: RefCell::new(HashSet::new()),
        library_controls: library_controls.clone(),
        collection_controls: collections_page.controls.clone(),
        on_library_change: Rc::new(on_library_change),
    });

    let completed_state = Rc::downgrade(&state);
    editor_view.controller.set_on_complete(move || {
        if let Some(state) = completed_state.upgrade() {
            refresh_all(&state);
        }
    });

    let visibility_player = state.player.clone();
    let visibility_editor = state.editor.clone();
    stack.connect_visible_child_name_notify(move |stack| {
        let page = stack.visible_child_name();
        if page.as_deref() != Some("player") {
            visibility_player.stop();
        }
        if page.as_deref() != Some("editor") {
            visibility_editor.stop();
        }
    });

    let preview_state = state.clone();
    glib::timeout_add_local(Duration::from_millis(80), move || {
        while let Ok(update) = preview_receiver.try_recv() {
            if let Some(widget_sets) = preview_state.preview_widgets.borrow().get(&update.key) {
                for widgets in widget_sets {
                    if let Some(path) = update.preview.thumbnail.as_ref() {
                        widgets.picture.set_filename(Some(path));
                        widgets.fallback.set_visible(false);
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

    if let Some(refresh) = library_controls.refresh.as_ref() {
        let refresh_state = state.clone();
        refresh.connect_clicked(move |_| refresh_all(&refresh_state));
    }

    let create_state = state.clone();
    collections_page
        .create
        .connect_clicked(move |button| show_create_collection(button, &create_state));

    connect_selection_controls(&library_controls, SelectionScope::Library, &state);
    connect_selection_controls(
        &collections_page.controls,
        SelectionScope::Collection,
        &state,
    );
    let delete_state = state.clone();
    collections_page.delete.connect_clicked(move |button| {
        let Some(path) = delete_state.selected_collection.borrow().clone() else {
            return;
        };
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("Collection")
            .to_owned();
        confirm_delete_collection(button, &name, &path, &delete_state);
    });

    ClipViews {
        library: page,
        collections: collections_page.page,
        player: player_page,
        editor: editor_view.page,
        header,
        heading,
        toolbar: library_controls.root.clone(),
        search: state.search.clone(),
        refresh: library_controls
            .refresh
            .expect("library controls always include refresh"),
        flow: state.flow.clone(),
        collection_flow: state.collection_flow.clone(),
        collections_sidebar: collections_page.sidebar.clone(),
        player_controller: state.player.clone(),
        editor_controller: state.editor.clone(),
        state,
    }
}

fn selection_controls(include_refresh: bool) -> SelectionControls {
    let root = GtkBox::new(Orientation::Horizontal, 10);
    root.add_css_class("selection-toolbar");
    root.set_halign(Align::End);
    root.set_valign(Align::Center);
    root.set_margin_top(25);

    let browse_actions = GtkBox::new(Orientation::Horizontal, 10);
    let select = Button::with_label("Select clips");
    select.add_css_class("toolbar-action");
    browse_actions.append(&select);
    let refresh = include_refresh.then(|| {
        let button = Button::with_label("Refresh");
        button.add_css_class("toolbar-action");
        button.set_tooltip_text(Some("Refresh clips"));
        browse_actions.append(&button);
        button
    });

    let selection_actions = GtkBox::new(Orientation::Horizontal, 10);
    let cancel = Button::with_label("Cancel");
    cancel.add_css_class("toolbar-action");
    let select_all = Button::with_label("Select all");
    select_all.add_css_class("toolbar-action");
    let move_selected = Button::with_label("Move (0)");
    move_selected.add_css_class("move-selected-action");
    move_selected.set_sensitive(false);
    selection_actions.append(&cancel);
    selection_actions.append(&select_all);
    selection_actions.append(&move_selected);
    selection_actions.set_visible(false);

    root.append(&browse_actions);
    root.append(&selection_actions);
    SelectionControls {
        root,
        browse_actions,
        selection_actions,
        select,
        cancel,
        select_all,
        move_selected,
        refresh,
    }
}

fn build_collections_page() -> CollectionsPage {
    let page = GtkBox::new(Orientation::Vertical, 0);
    page.add_css_class("collections-page");

    let header = GtkBox::new(Orientation::Horizontal, 18);
    header.set_margin_bottom(26);
    let heading = GtkBox::new(Orientation::Vertical, 8);
    heading.set_hexpand(true);
    let title = Label::new(Some("Collections"));
    title.add_css_class("page-title");
    title.set_halign(Align::Start);
    let subtitle = Label::new(Some("Keep clips grouped without uploads"));
    subtitle.add_css_class("page-subtitle");
    subtitle.set_halign(Align::Start);
    heading.append(&subtitle);
    heading.append(&title);
    let controls = selection_controls(false);
    header.append(&heading);
    header.append(&controls.root);
    page.append(&header);

    let body = GtkBox::new(Orientation::Horizontal, 26);
    body.add_css_class("collections-body");
    body.set_vexpand(true);
    let sidebar = GtkBox::new(Orientation::Vertical, 6);
    sidebar.add_css_class("collections-sidebar");
    sidebar.set_size_request(220, -1);
    let create = Button::with_label("+ New collection");
    create.add_css_class("collection-create-action");
    let delete = Button::with_label("Delete collection");
    delete.add_css_class("collection-delete-action");
    delete.set_visible(false);
    sidebar.append(&create);
    sidebar.append(&delete);

    let folders = FlowBox::new();
    folders.add_css_class("collection-folders");
    folders.set_selection_mode(SelectionMode::None);
    folders.set_column_spacing(0);
    folders.set_row_spacing(4);
    folders.set_max_children_per_line(1);
    folders.set_min_children_per_line(1);
    folders.set_homogeneous(true);
    folders.set_valign(Align::Start);
    sidebar.append(&folders);
    body.append(&sidebar);

    let collection_content = GtkBox::new(Orientation::Vertical, 0);
    collection_content.add_css_class("collection-content");
    collection_content.set_hexpand(true);
    collection_content.set_vexpand(true);
    let clip_title = Label::new(Some("All clips"));
    clip_title.add_css_class("collection-clip-title");
    clip_title.set_halign(Align::Start);
    clip_title.set_margin_bottom(14);
    collection_content.append(&clip_title);

    let clips = FlowBox::new();
    clips.set_selection_mode(SelectionMode::None);
    clips.set_column_spacing(12);
    clips.set_row_spacing(12);
    clips.set_min_children_per_line(1);
    clips.set_max_children_per_line(3);
    clips.set_homogeneous(true);
    clips.set_valign(Align::Start);
    let scroll = ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_vexpand(true);
    scroll.set_child(Some(&clips));
    collection_content.append(&scroll);

    let empty = Label::new(Some("This collection is empty"));
    empty.add_css_class("collection-empty");
    empty.set_halign(Align::Start);
    empty.set_valign(Align::Start);
    empty.set_margin_top(66);
    empty.set_visible(false);
    collection_content.append(&empty);
    body.append(&collection_content);
    page.append(&body);

    CollectionsPage {
        page,
        tiles: folders,
        flow: clips,
        empty,
        sidebar,
        title: clip_title,
        create,
        delete,
        controls,
    }
}

fn reload_clips(state: &LibraryState, directory: &std::path::Path) {
    let loaded = clips::scan(directory).unwrap_or_default();
    let size_bytes = loaded.iter().map(|clip| clip.size_bytes).sum::<u64>();
    state.count.set_text("Local replays");
    state.count.remove_css_class("error");
    state.library_meta.set_text(&format!(
        "{} clips  •  {}",
        loaded.len(),
        clips::format_size(size_bytes)
    ));
    state.clips.replace(loaded);
    state
        .selected_clips
        .borrow_mut()
        .retain(|path| state.clips.borrow().iter().any(|clip| &clip.path == path));
}

fn refresh_all(state: &Rc<LibraryState>) {
    let configured_directory = Config::load(&AppPaths::discover())
        .unwrap_or_default()
        .storage
        .directory;
    let directory_changed = *state.directory.borrow() != configured_directory;
    if directory_changed {
        let _ = std::fs::create_dir_all(&configured_directory);
        state.directory.replace(configured_directory.clone());
        state
            .player
            .library_directory
            .replace(configured_directory.clone());
        state.selected_collection.replace(None);
        state.selected_clips.borrow_mut().clear();
        state.selection_mode.set(false);
    }
    if directory_changed || state.directory_monitor.borrow().is_none() {
        install_directory_monitor(state);
    }
    reload_clips(state, &configured_directory);
    state.preview_widgets.borrow_mut().clear();
    render_clips(state);
    render_collections(state);
    update_selection_controls(state);
    (state.on_library_change)();
}

fn install_directory_monitor(state: &Rc<LibraryState>) {
    let directory = state.directory.borrow().clone();
    let Ok(monitor) = gio::File::for_path(directory)
        .monitor_directory(gio::FileMonitorFlags::NONE, None::<&gio::Cancellable>)
    else {
        state.directory_monitor.replace(None);
        return;
    };
    let monitor_state = Rc::downgrade(state);
    monitor.connect_changed(move |_, _, _, _| {
        if let Some(state) = monitor_state.upgrade() {
            refresh_all(&state);
        }
    });
    state.directory_monitor.replace(Some(monitor));
}

fn render_clips(state: &Rc<LibraryState>) {
    while let Some(child) = state.flow.first_child() {
        state.flow.remove(&child);
    }
    state.library_aspects.borrow_mut().clear();
    let query = state.search.text().trim().to_ascii_lowercase();
    let clips = state
        .clips
        .borrow()
        .iter()
        .filter(|clip| clip_matches_query(clip, &query))
        .cloned()
        .collect::<Vec<_>>();
    let empty = clips.is_empty();
    if empty {
        state
            .empty_label
            .set_text(if state.clips.borrow().is_empty() {
                "No clips yet"
            } else {
                "No matching clips"
            });
    }
    state.empty.set_visible(empty);
    state.scroll.set_visible(!empty);
    state.flow.set_visible(!empty);
    for (index, clip) in clips.into_iter().take(200).enumerate() {
        let (child, widgets) = clip_card(&clip, state);
        state
            .library_aspects
            .borrow_mut()
            .push(widgets.aspect.clone());
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
    state.collection_aspects.borrow_mut().clear();

    let selected_path = state.selected_collection.borrow().clone();
    let query = state.search.text().trim().to_ascii_lowercase();
    let all = collection_button("All clips", state.clips.borrow().len(), None, state);
    state.collection_tiles.insert(&all, -1);
    for collection in clips::collections(state.directory.borrow().as_path()).unwrap_or_default() {
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
                && clip_matches_query(clip, &query)
        })
        .cloned()
        .collect::<Vec<_>>();
    let heading = selected_path
        .as_ref()
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("All clips");
    state.collection_title.set_text(heading);
    state.delete_collection.set_visible(selected_path.is_some());
    state.collection_empty.set_visible(visible.is_empty());
    state.collection_flow.set_visible(!visible.is_empty());
    for (index, clip) in visible.into_iter().take(200).enumerate() {
        let (child, widgets) = clip_card(&clip, state);
        state
            .collection_aspects
            .borrow_mut()
            .push(widgets.aspect.clone());
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
    let title = Label::new(Some(name));
    title.add_css_class("collection-name");
    title.set_halign(Align::Start);
    title.set_hexpand(true);
    let detail = Label::new(Some(&count.to_string()));
    detail.add_css_class("collection-count");
    detail.set_halign(Align::Start);
    row.append(&title);
    row.append(&detail);
    let button = Button::new();
    button.add_css_class("collection-button");
    if *state.selected_collection.borrow() == path {
        button.add_css_class("active");
    }
    button.set_child(Some(&row));
    button.set_tooltip_text(Some(&format!("Open {name}")));
    button.update_property(&[gtk::accessible::Property::Label(&format!(
        "{name}, {count} clips"
    ))]);
    button.update_state(&[gtk::accessible::State::Selected(Some(
        *state.selected_collection.borrow() == path,
    ))]);

    let selected = path.clone();
    let selected_state = state.clone();
    button.connect_clicked(move |_| {
        selected_state.selected_collection.replace(selected.clone());
        refresh_all(&selected_state);
    });

    if let Some(collection_path) = path.as_ref() {
        let target = DropTarget::new(String::static_type(), gdk::DragAction::MOVE);
        let drop_state = state.clone();
        let collection_path = collection_path.clone();
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
                drop_state.directory.borrow().as_path(),
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

fn confirm_delete_collection(
    button: &Button,
    name: &str,
    path: &std::path::Path,
    state: &Rc<LibraryState>,
) {
    let popover = Popover::new();
    popover.add_css_class("delete-confirmation");
    popover.set_position(PositionType::Bottom);
    popover.set_parent(button);
    let content = GtkBox::new(Orientation::Vertical, 5);
    content.add_css_class("delete-confirmation-content");
    let title = Label::new(Some("Delete collection?"));
    title.add_css_class("delete-confirmation-title");
    title.set_halign(Align::Start);
    let detail = Label::new(Some(&format!(
        "{name} is removed; its clips move safely back to Library."
    )));
    detail.add_css_class("delete-confirmation-detail");
    detail.set_halign(Align::Start);
    let actions = GtkBox::new(Orientation::Horizontal, 8);
    actions.set_halign(Align::End);
    actions.set_margin_top(9);
    let cancel = Button::with_label("Cancel");
    cancel.add_css_class("popover-action");
    let confirm = Button::with_label("Delete collection");
    confirm.add_css_class("danger-action");
    actions.append(&cancel);
    actions.append(&confirm);
    content.append(&title);
    content.append(&detail);
    content.append(&actions);
    popover.set_child(Some(&content));
    popover.connect_closed(|popover| popover.unparent());
    let cancelled = popover.clone();
    cancel.connect_clicked(move |_| cancelled.popdown());
    let confirmed = popover.clone();
    let removed_path = path.to_path_buf();
    let remove_state = state.clone();
    confirm.connect_clicked(move |_| {
        match clips::delete_collection(
            remove_state.directory.borrow().as_path(),
            &removed_path,
            &remove_state.thumbnail_directory,
        ) {
            Ok(()) => {
                if remove_state.selected_collection.borrow().as_ref() == Some(&removed_path) {
                    remove_state.selected_collection.replace(None);
                }
                confirmed.popdown();
                remove_state.count.remove_css_class("error");
                refresh_all(&remove_state);
            }
            Err(error) => {
                remove_state
                    .count
                    .set_text(&format!("Could not delete collection: {error}"));
                remove_state.count.add_css_class("error");
                confirmed.popdown();
            }
        }
    });
    popover.popup();
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
    let field_label = Label::new(Some("Collection name"));
    field_label.add_css_class("dialog-field-label");
    field_label.set_halign(Align::Start);
    let name = Entry::new();
    name.set_placeholder_text(Some("e.g. Funny"));
    name.set_max_length(80);
    name.update_property(&[
        gtk::accessible::Property::Label("Collection name"),
        gtk::accessible::Property::KeyShortcuts("Enter Escape"),
    ]);
    let actions = GtkBox::new(Orientation::Horizontal, 8);
    actions.set_halign(Align::End);
    let cancel = Button::with_label("Cancel");
    cancel.add_css_class("popover-action");
    let create = Button::with_label("Create");
    create.add_css_class("primary-action");
    actions.append(&cancel);
    actions.append(&create);
    content.append(&title);
    content.append(&field_label);
    content.append(&name);
    let shortcuts = Label::new(Some(
        "Ctrl+A select all · Ctrl+C/X/V · Enter confirm · Esc cancel",
    ));
    shortcuts.add_css_class("dialog-shortcuts");
    shortcuts.set_halign(Align::Start);
    content.append(&shortcuts);
    content.append(&actions);
    popover.set_child(Some(&content));
    popover.connect_closed(|popover| popover.unparent());
    let cancelled = popover.clone();
    cancel.connect_clicked(move |_| cancelled.popdown());
    let created = popover.clone();
    let create_state = state.clone();
    let entered_name = name.clone();
    create.connect_clicked(move |_| {
        match clips::create_collection(
            create_state.directory.borrow().as_path(),
            entered_name.text().as_str(),
        ) {
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
    let submit = create.clone();
    name.connect_activate(move |_| submit.emit_clicked());
    popover.popup();
    name.grab_focus();
}

fn connect_selection_controls(
    controls: &SelectionControls,
    scope: SelectionScope,
    state: &Rc<LibraryState>,
) {
    let select_state = state.clone();
    controls.select.connect_clicked(move |_| {
        select_state.selection_mode.set(true);
        rerender_clip_views(&select_state);
    });

    let cancel_state = state.clone();
    controls
        .cancel
        .connect_clicked(move |_| clear_selection(&cancel_state));

    let select_all_state = state.clone();
    controls.select_all.connect_clicked(move |_| {
        let paths = visible_clip_paths(&select_all_state, scope);
        select_all_state.selection_mode.set(true);
        select_all_state.selected_clips.borrow_mut().extend(paths);
        rerender_clip_views(&select_all_state);
    });

    let move_state = state.clone();
    controls
        .move_selected
        .connect_clicked(move |button| show_move_selected(button, &move_state));
}

fn visible_clip_paths(state: &LibraryState, scope: SelectionScope) -> Vec<PathBuf> {
    visible_clips(state, scope)
        .into_iter()
        .map(|clip| clip.path)
        .collect()
}

fn visible_clips(state: &LibraryState, scope: SelectionScope) -> Vec<Clip> {
    let query = state.search.text().trim().to_ascii_lowercase();
    let collection = state.selected_collection.borrow().clone();
    state
        .clips
        .borrow()
        .iter()
        .filter(|clip| match scope {
            SelectionScope::Library => clip_matches_query(clip, &query),
            SelectionScope::Collection => {
                collection
                    .as_ref()
                    .is_none_or(|path| clip.path.parent() == Some(path.as_path()))
                    && clip_matches_query(clip, &query)
            }
        })
        .cloned()
        .collect()
}

fn clip_matches_query(clip: &Clip, query: &str) -> bool {
    query.is_empty()
        || clip.title.to_ascii_lowercase().contains(query)
        || clip
            .path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().to_ascii_lowercase().contains(query))
}

fn clear_selection(state: &Rc<LibraryState>) {
    if !state.selection_mode.get() && state.selected_clips.borrow().is_empty() {
        return;
    }
    state.selection_mode.set(false);
    state.selected_clips.borrow_mut().clear();
    rerender_clip_views(state);
}

fn toggle_clip_selection(state: &Rc<LibraryState>, path: &std::path::Path) {
    state.selection_mode.set(true);
    let mut selected = state.selected_clips.borrow_mut();
    if !selected.remove(path) {
        selected.insert(path.to_owned());
    }
    drop(selected);
    rerender_clip_views(state);
}

fn rerender_clip_views(state: &Rc<LibraryState>) {
    state.preview_widgets.borrow_mut().clear();
    render_clips(state);
    render_collections(state);
    update_selection_controls(state);
}

fn update_selection_controls(state: &LibraryState) {
    let selection_mode = state.selection_mode.get();
    let selected = state.selected_clips.borrow().len();
    let can_move = selected > 0
        && clips::collections(state.directory.borrow().as_path())
            .is_ok_and(|collections| !collections.is_empty());
    for controls in [&state.library_controls, &state.collection_controls] {
        controls.browse_actions.set_visible(!selection_mode);
        controls.selection_actions.set_visible(selection_mode);
        controls
            .move_selected
            .set_label(&format!("Move ({selected})"));
        controls.move_selected.set_sensitive(can_move);
    }
}

fn show_move_selected(button: &Button, state: &Rc<LibraryState>) {
    let collections = clips::collections(state.directory.borrow().as_path()).unwrap_or_default();
    if collections.is_empty() || state.selected_clips.borrow().is_empty() {
        return;
    }

    let popover = Popover::new();
    popover.add_css_class("clip-menu");
    popover.set_position(PositionType::Bottom);
    popover.set_parent(button);
    let content = GtkBox::new(Orientation::Vertical, 2);
    content.add_css_class("clip-menu-content");
    let heading = Label::new(Some("MOVE SELECTED CLIPS TO"));
    heading.add_css_class("menu-section-label");
    heading.set_halign(Align::Start);
    content.append(&heading);

    for collection in collections.into_iter().take(8) {
        let action = menu_action(&collection.name);
        let moved = popover.clone();
        let destination = collection.path;
        let move_state = state.clone();
        action.connect_clicked(move |_| {
            moved.popdown();
            move_selected_to_collection(&move_state, &destination);
        });
        content.append(&action);
    }
    popover.set_child(Some(&content));
    popover.connect_closed(|popover| popover.unparent());
    popover.popup();
}

fn move_selected_to_collection(state: &Rc<LibraryState>, destination: &std::path::Path) {
    let selected = state.selected_clips.borrow().clone();
    let chosen = state
        .clips
        .borrow()
        .iter()
        .filter(|clip| selected.contains(&clip.path))
        .filter(|clip| clip.path.parent() != Some(destination))
        .cloned()
        .collect::<Vec<_>>();

    let collection_name = destination
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("collection");
    if chosen.is_empty() {
        state
            .count
            .set_text("The selected clips are already in this collection");
        return;
    }
    if let Some(existing) = chosen.iter().find_map(|clip| {
        let destination = destination.join(clip.path.file_name()?);
        destination.exists().then_some(destination)
    }) {
        let file_name = existing
            .file_name()
            .map_or_else(|| "a clip".into(), |name| name.to_string_lossy());
        state.count.set_text(&format!(
            "Cannot move clips: {file_name} already exists in {collection_name}"
        ));
        state.count.add_css_class("error");
        return;
    }

    let mut moved = 0;
    let mut failure = None;
    for clip in chosen {
        match clips::move_to_collection(
            &clip,
            state.directory.borrow().as_path(),
            destination,
            &state.thumbnail_directory,
        ) {
            Ok(_) => moved += 1,
            Err(error) => {
                failure = Some(error);
                break;
            }
        }
    }

    state.selected_clips.borrow_mut().clear();
    state.selection_mode.set(false);
    refresh_all(state);
    match failure {
        Some(error) => {
            state
                .count
                .set_text(&format!("Moved {moved} clips; stopped: {error}"));
            state.count.add_css_class("error");
        }
        None => {
            let noun = if moved == 1 { "Clip" } else { "clips" };
            state
                .count
                .set_text(&format!("{moved} {noun} moved to {collection_name}"));
            state.count.remove_css_class("error");
        }
    }
}

fn clip_card(clip: &Clip, state: &Rc<LibraryState>) -> (FlowBoxChild, PreviewWidgets) {
    let item = Overlay::new();
    item.add_css_class("clip-item");
    let is_selected = state.selected_clips.borrow().contains(&clip.path);
    if is_selected {
        item.add_css_class("selected");
    }
    item.set_hexpand(true);

    let open = Button::new();
    open.add_css_class("clip-open");
    open.set_hexpand(true);
    open.set_tooltip_text(Some(&clip.path.to_string_lossy()));
    open.update_property(&[
        gtk::accessible::Property::Label(&format!("Open {}", clip.title)),
        gtk::accessible::Property::Description(&format!(
            "{}, {}. Right-click or press Shift+F10 for clip actions.",
            clips::format_age(clip.modified),
            clips::format_size(clip.size_bytes)
        )),
        gtk::accessible::Property::KeyShortcuts("Shift+F10"),
    ]);
    open.update_state(&[gtk::accessible::State::Selected(Some(is_selected))]);
    let body = GtkBox::new(Orientation::Vertical, 0);
    body.set_hexpand(true);
    body.set_halign(Align::Fill);
    let picture = Picture::new();
    picture.add_css_class("clip-preview");
    picture.set_content_fit(ContentFit::Cover);
    picture.set_can_shrink(true);
    picture.set_hexpand(true);
    picture.set_vexpand(true);
    picture.set_halign(Align::Fill);
    picture.set_valign(Align::Fill);
    let preview = Overlay::new();
    preview.add_css_class("clip-preview-frame");
    preview.set_hexpand(true);
    preview.set_overflow(gtk::Overflow::Hidden);
    preview.set_child(Some(&picture));
    let fallback = Label::new(Some("▶"));
    fallback.add_css_class("clip-fallback");
    fallback.set_halign(Align::Start);
    fallback.set_valign(Align::Start);
    fallback.set_margin_top(12);
    fallback.set_margin_start(8);
    preview.add_overlay(&fallback);
    let selection = Label::new(Some(if is_selected { "✓" } else { "" }));
    selection.add_css_class("clip-selection");
    if is_selected {
        selection.add_css_class("selected");
    }
    selection.set_halign(Align::End);
    selection.set_valign(Align::Start);
    selection.set_margin_top(8);
    selection.set_margin_end(8);
    selection.set_visible(state.selection_mode.get());
    preview.add_overlay(&selection);
    let aspect = AspectFrame::new(0.5, 0.5, 16.0 / 9.0, false);
    aspect.add_css_class("clip-preview-aspect");
    aspect.set_margin_top(6);
    aspect.set_margin_start(6);
    aspect.set_margin_end(6);
    aspect.set_child(Some(&preview));
    body.append(&aspect);

    let text = GtkBox::new(Orientation::Vertical, 3);
    text.add_css_class("clip-info");
    let title = Label::new(Some(&clip.title));
    title.add_css_class("clip-title");
    title.set_halign(Align::Start);
    title.set_ellipsize(pango::EllipsizeMode::End);
    title.set_max_width_chars(23);
    let metadata = GtkBox::new(Orientation::Horizontal, 7);
    let age = Label::new(Some(&clips::format_age(clip.modified)));
    age.add_css_class("clip-meta");
    let separator = Label::new(Some("·"));
    separator.add_css_class("clip-meta");
    let size = Label::new(Some(&clips::format_size(clip.size_bytes)));
    size.add_css_class("clip-meta");
    metadata.append(&age);
    metadata.append(&separator);
    metadata.append(&size);
    text.append(&title);
    text.append(&metadata);
    body.append(&text);
    open.set_child(Some(&body));
    item.set_child(Some(&open));

    let selected = clip.clone();
    let open_state = state.clone();
    open.connect_clicked(move |_| {
        if open_state.selection_mode.get() {
            toggle_clip_selection(&open_state, &selected.path);
        } else {
            let return_page = open_state
                .stack
                .visible_child_name()
                .unwrap_or_else(|| "library".into());
            let scope = if return_page.as_str() == "collections" {
                SelectionScope::Collection
            } else {
                SelectionScope::Library
            };
            show_player(
                &open_state.player,
                &selected,
                visible_clips(&open_state, scope),
                return_page.as_str(),
            );
            open_state.stack.set_visible_child_name("player");
        }
    });
    let context_click = GestureClick::new();
    context_click.set_button(3);
    context_click.set_propagation_phase(gtk::PropagationPhase::Capture);
    let context_anchor = open.clone();
    let context_clip = clip.clone();
    let context_state = state.clone();
    context_click.connect_released(move |_, _, x, y| {
        if !context_state.selection_mode.get() {
            show_clip_menu(&context_anchor, &context_clip, &context_state, Some((x, y)));
        }
    });
    open.add_controller(context_click);

    let context_key = EventControllerKey::new();
    let key_anchor = open.clone();
    let key_clip = clip.clone();
    let key_state = state.clone();
    context_key.connect_key_pressed(move |_, key, _, modifiers| {
        if !key_state.selection_mode.get()
            && key == gdk::Key::F10
            && modifiers.contains(gdk::ModifierType::SHIFT_MASK)
        {
            show_clip_menu(&key_anchor, &key_clip, &key_state, None);
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    open.add_controller(context_key);

    let drag = DragSource::new();
    drag.set_actions(gdk::DragAction::MOVE);
    let dragged_path = clip.path.to_string_lossy().into_owned();
    let drag_state = state.clone();
    drag.connect_prepare(move |_, _, _| {
        (!drag_state.selection_mode.get())
            .then(|| gdk::ContentProvider::for_value(&dragged_path.to_value()))
    });
    item.add_controller(drag);

    let child = FlowBoxChild::new();
    child.set_child(Some(&item));
    (
        child,
        PreviewWidgets {
            picture,
            fallback,
            aspect,
        },
    )
}

fn set_preview_heights(frames: &[AspectFrame], flow_width: i32, columns: u32) {
    let columns = columns.max(1);
    let gaps = 12 * i32::try_from(columns.saturating_sub(1)).unwrap_or(0);
    let card_width = (flow_width - gaps).max(1) / i32::try_from(columns).unwrap_or(1);
    let preview_height = ((card_width - 12).max(1) * 9 + 8) / 16;
    for frame in frames {
        frame.set_size_request(-1, preview_height);
    }
}

fn show_clip_menu(
    button: &Button,
    clip: &Clip,
    state: &Rc<LibraryState>,
    point: Option<(f64, f64)>,
) {
    let popover = Popover::new();
    popover.add_css_class("clip-menu");
    popover.set_position(PositionType::Bottom);
    popover.set_parent(button);
    if let Some((x, y)) = point {
        popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    }
    let content = GtkBox::new(Orientation::Vertical, 2);
    content.add_css_class("clip-menu-content");

    let menu_title = Label::new(Some("Clip actions"));
    menu_title.add_css_class("menu-section-label");
    menu_title.set_halign(Align::Start);
    let clip_title = Label::new(Some(&clip.title));
    clip_title.add_css_class("clip-menu-title");
    clip_title.set_halign(Align::Start);
    clip_title.set_ellipsize(pango::EllipsizeMode::End);
    content.append(&menu_title);
    content.append(&clip_title);

    let edit = menu_action("Edit clip");
    content.append(&edit);
    let rename = menu_action("Rename");
    content.append(&rename);
    let collections = clips::collections(state.directory.borrow().as_path()).unwrap_or_default();
    if !collections.is_empty() {
        let move_label = Label::new(Some("Move to collection"));
        move_label.add_css_class("menu-section-label");
        move_label.set_halign(Align::Start);
        content.append(&move_label);
    }
    for collection in collections {
        let action = menu_action(&collection.name);
        let moved = popover.clone();
        let moved_clip = clip.clone();
        let move_state = state.clone();
        let destination = collection.path;
        action.connect_clicked(move |_| {
            match clips::move_to_collection(
                &moved_clip,
                move_state.directory.borrow().as_path(),
                &destination,
                &move_state.thumbnail_directory,
            ) {
                Ok(_) => {
                    moved.popdown();
                    move_state.count.remove_css_class("error");
                    refresh_all(&move_state);
                }
                Err(error) => {
                    move_state
                        .count
                        .set_text(&format!("Could not move clip: {error}"));
                    move_state.count.add_css_class("error");
                }
            }
        });
        content.append(&action);
    }
    let separator = gtk::Separator::new(Orientation::Horizontal);
    separator.set_margin_top(3);
    separator.set_margin_bottom(3);
    content.append(&separator);
    let delete = menu_action("Delete clip");
    delete.add_css_class("danger-menu-action");
    delete.set_margin_top(5);
    content.append(&delete);
    popover.set_child(Some(&content));
    popover.connect_closed(|popover| popover.unparent());

    let renamed_menu = popover.clone();
    let rename_anchor = button.clone();
    let renamed_clip = clip.clone();
    let rename_state = state.clone();
    rename.connect_clicked(move |_| {
        renamed_menu.popdown();
        show_rename(&rename_anchor, &renamed_clip, &rename_state);
    });
    let edited_menu = popover.clone();
    let edited_clip = clip.clone();
    let edit_state = state.clone();
    edit.connect_clicked(move |_| {
        edited_menu.popdown();
        edit_state.editor.open(&edited_clip);
    });
    let deleted_menu = popover.clone();
    let delete_anchor = button.clone();
    let deleted_clip = clip.clone();
    let delete_state = state.clone();
    delete.connect_clicked(move |_| {
        deleted_menu.popdown();
        confirm_delete(&delete_anchor, &deleted_clip, &delete_state);
    });
    popover.popup();
}

fn menu_action(label: &str) -> Button {
    let button = Button::with_label(label);
    button.add_css_class("menu-action");
    button
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
    let field_label = Label::new(Some("Clip name"));
    field_label.add_css_class("dialog-field-label");
    field_label.set_halign(Align::Start);
    let name = Entry::new();
    let current_name = clip
        .path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(&clip.title);
    name.set_text(current_name);
    name.set_max_length(80);
    name.select_region(0, -1);
    name.update_property(&[
        gtk::accessible::Property::Label("Clip name"),
        gtk::accessible::Property::KeyShortcuts("Enter Escape"),
    ]);
    let actions = GtkBox::new(Orientation::Horizontal, 8);
    actions.set_halign(Align::End);
    let cancel = Button::with_label("Cancel");
    cancel.add_css_class("popover-action");
    let save = Button::with_label("Rename");
    save.add_css_class("primary-action");
    actions.append(&cancel);
    actions.append(&save);
    content.append(&title);
    content.append(&field_label);
    content.append(&name);
    let shortcuts = Label::new(Some(
        "Ctrl+A select all · Ctrl+C/X/V · Enter confirm · Esc cancel",
    ));
    shortcuts.add_css_class("dialog-shortcuts");
    shortcuts.set_halign(Align::Start);
    content.append(&shortcuts);
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
    let submit = save.clone();
    name.connect_activate(move |_| submit.emit_clicked());
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
    let detail = Label::new(Some(&format!(
        "{} is removed permanently. This cannot be undone.",
        clip.title
    )));
    detail.add_css_class("delete-confirmation-detail");
    detail.set_halign(Align::Start);
    detail.set_ellipsize(pango::EllipsizeMode::Middle);
    detail.set_max_width_chars(32);
    let actions = GtkBox::new(Orientation::Horizontal, 8);
    actions.set_halign(Align::End);
    actions.set_margin_top(9);
    let cancel = Button::with_label("Cancel");
    cancel.add_css_class("popover-action");
    let confirm = Button::with_label("Delete clip");
    confirm.add_css_class("danger-action");
    actions.append(&cancel);
    actions.append(&confirm);
    content.append(&title);
    content.append(&detail);
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

fn empty_state() -> (GtkBox, Label) {
    let empty = GtkBox::new(Orientation::Vertical, 0);
    empty.add_css_class("empty-state");
    empty.set_hexpand(true);
    empty.set_valign(Align::Start);
    empty.set_margin_top(42);
    empty.set_margin_bottom(30);
    let label = Label::new(Some("No clips yet"));
    label.add_css_class("empty-simple");
    label.set_halign(Align::Start);
    empty.append(&label);
    (empty, label)
}

fn build_player(
    stack: &Stack,
    editor: &Rc<crate::editor::EditorController>,
    library_directory: &std::path::Path,
) -> (GtkBox, Rc<PlayerState>) {
    let page = GtkBox::new(Orientation::Vertical, 0);
    page.add_css_class("player-page");

    let header = GtkBox::new(Orientation::Horizontal, 14);
    header.set_margin_bottom(20);
    let back = Button::with_label("‹ Back");
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
    let edit = Button::with_label("Edit clip");
    edit.add_css_class("primary-action");
    header.append(&back);
    header.append(&titles);
    header.append(&edit);
    header.append(&reveal);
    page.append(&header);

    let media = MediaFile::new();
    media.set_loop(false);
    media.set_volume(1.0);
    let stage = Picture::for_paintable(&media);
    stage.add_css_class("video-stage");
    stage.set_content_fit(ContentFit::Contain);
    stage.set_can_shrink(true);
    stage.set_hexpand(true);
    stage.set_vexpand(true);

    let stage_row = GtkBox::new(Orientation::Horizontal, 12);
    stage_row.add_css_class("player-stage-row");
    stage_row.set_hexpand(true);
    stage_row.set_vexpand(true);
    let previous_icon = Image::from_icon_name("go-previous-symbolic");
    previous_icon.set_pixel_size(24);
    let previous = Button::new();
    previous.set_child(Some(&previous_icon));
    previous.add_css_class("player-step");
    previous.set_tooltip_text(Some("Previous clip"));
    previous.update_property(&[gtk::accessible::Property::Label("Previous clip")]);
    previous.set_sensitive(false);
    let next_icon = Image::from_icon_name("go-next-symbolic");
    next_icon.set_pixel_size(24);
    let next = Button::new();
    next.set_child(Some(&next_icon));
    next.add_css_class("player-step");
    next.set_tooltip_text(Some("Next clip"));
    next.update_property(&[gtk::accessible::Property::Label("Next clip")]);
    next.set_sensitive(false);
    stage_row.append(&previous);
    stage_row.append(&stage);

    let volume_column = GtkBox::new(Orientation::Vertical, 8);
    volume_column.add_css_class("player-volume");
    volume_column.set_valign(Align::Center);
    let volume_value = Label::new(Some("100%"));
    volume_value.add_css_class("player-volume-value");
    let volume = Scale::with_range(Orientation::Vertical, 0.0, 100.0, 1.0);
    volume.add_css_class("player-volume-slider");
    volume.set_draw_value(false);
    volume.set_value(100.0);
    volume.set_size_request(36, 142);
    volume.set_tooltip_text(Some("Player volume"));
    volume.update_property(&[gtk::accessible::Property::Label("Player volume")]);
    let mute_icon = Image::from_icon_name("audio-volume-high-symbolic");
    mute_icon.set_pixel_size(18);
    let mute = Button::new();
    mute.set_child(Some(&mute_icon));
    mute.add_css_class("player-media-action");
    mute.set_tooltip_text(Some("Mute player"));
    mute.update_property(&[gtk::accessible::Property::Label("Mute player")]);
    volume_column.append(&volume_value);
    volume_column.append(&volume);
    volume_column.append(&mute);
    stage_row.append(&next);
    stage_row.append(&volume_column);
    page.append(&stage_row);

    let controls = GtkBox::new(Orientation::Horizontal, 12);
    controls.add_css_class("player-controls");
    controls.set_margin_top(14);
    let play_pause_icon = Image::from_icon_name("media-playback-start-symbolic");
    play_pause_icon.set_pixel_size(20);
    let play_pause = Button::new();
    play_pause.set_child(Some(&play_pause_icon));
    play_pause.add_css_class("player-media-action");
    play_pause.set_tooltip_text(Some("Play or pause"));
    play_pause.update_property(&[
        gtk::accessible::Property::Label("Play"),
        gtk::accessible::Property::KeyShortcuts("Space"),
    ]);
    let timeline = Scale::with_range(Orientation::Horizontal, 0.0, 1.0, 0.01);
    timeline.add_css_class("player-timeline");
    timeline.set_draw_value(false);
    timeline.set_hexpand(true);
    timeline.set_tooltip_text(Some("Playback position"));
    timeline.update_property(&[gtk::accessible::Property::Label("Playback position")]);
    let fullscreen_icon = Image::from_icon_name("view-fullscreen-symbolic");
    fullscreen_icon.set_pixel_size(18);
    let fullscreen = Button::new();
    fullscreen.set_child(Some(&fullscreen_icon));
    fullscreen.add_css_class("player-media-action");
    fullscreen.set_tooltip_text(Some("Enter fullscreen"));
    fullscreen.update_property(&[
        gtk::accessible::Property::Label("Enter fullscreen"),
        gtk::accessible::Property::KeyShortcuts("Escape"),
    ]);
    let time = Label::new(Some("00:00 / 00:00"));
    time.add_css_class("player-time");
    controls.append(&play_pause);
    controls.append(&timeline);
    controls.append(&fullscreen);
    controls.append(&time);
    page.append(&controls);

    let fullscreen_root = Overlay::new();
    fullscreen_root.add_css_class("fullscreen-player");
    fullscreen_root.set_hexpand(true);
    fullscreen_root.set_vexpand(true);
    let fullscreen_stage = Picture::for_paintable(&media);
    fullscreen_stage.add_css_class("fullscreen-video");
    fullscreen_stage.set_content_fit(ContentFit::Contain);
    fullscreen_stage.set_can_shrink(true);
    fullscreen_stage.set_halign(Align::Fill);
    fullscreen_stage.set_valign(Align::Fill);
    fullscreen_stage.set_hexpand(true);
    fullscreen_stage.set_vexpand(true);
    fullscreen_root.set_child(Some(&fullscreen_stage));

    let fullscreen_controls = GtkBox::new(Orientation::Vertical, 0);
    fullscreen_controls.add_css_class("fullscreen-controls");
    fullscreen_controls.set_halign(Align::Fill);
    fullscreen_controls.set_valign(Align::End);
    fullscreen_controls.set_hexpand(true);
    fullscreen_controls.set_size_request(-1, 80);
    let fullscreen_timeline = Scale::with_range(Orientation::Horizontal, 0.0, 1.0, 0.01);
    fullscreen_timeline.add_css_class("player-timeline");
    fullscreen_timeline.add_css_class("fullscreen-timeline");
    fullscreen_timeline.set_draw_value(false);
    fullscreen_timeline.set_hexpand(true);
    fullscreen_timeline.set_tooltip_text(Some("Playback position"));
    fullscreen_timeline.update_property(&[gtk::accessible::Property::Label("Playback position")]);
    fullscreen_controls.append(&fullscreen_timeline);

    let fullscreen_row = GtkBox::new(Orientation::Horizontal, 4);
    fullscreen_row.add_css_class("fullscreen-controls-row");
    fullscreen_row.set_hexpand(true);
    fullscreen_row.set_valign(Align::Center);
    let fullscreen_play_pause_icon = Image::from_icon_name("media-playback-start-symbolic");
    fullscreen_play_pause_icon.set_pixel_size(20);
    let fullscreen_play_pause = Button::new();
    fullscreen_play_pause.set_child(Some(&fullscreen_play_pause_icon));
    fullscreen_play_pause.add_css_class("player-media-action");
    fullscreen_play_pause.set_tooltip_text(Some("Play or pause"));
    fullscreen_play_pause.update_property(&[
        gtk::accessible::Property::Label("Play"),
        gtk::accessible::Property::KeyShortcuts("Space"),
    ]);
    let fullscreen_previous_icon = Image::from_icon_name("go-previous-symbolic");
    fullscreen_previous_icon.set_pixel_size(18);
    let fullscreen_previous = Button::new();
    fullscreen_previous.set_child(Some(&fullscreen_previous_icon));
    fullscreen_previous.add_css_class("player-media-action");
    fullscreen_previous.set_tooltip_text(Some("Previous clip"));
    fullscreen_previous.update_property(&[gtk::accessible::Property::Label("Previous clip")]);
    fullscreen_previous.set_sensitive(false);
    let fullscreen_next_icon = Image::from_icon_name("go-next-symbolic");
    fullscreen_next_icon.set_pixel_size(18);
    let fullscreen_next = Button::new();
    fullscreen_next.set_child(Some(&fullscreen_next_icon));
    fullscreen_next.add_css_class("player-media-action");
    fullscreen_next.set_tooltip_text(Some("Next clip"));
    fullscreen_next.update_property(&[gtk::accessible::Property::Label("Next clip")]);
    fullscreen_next.set_sensitive(false);
    let fullscreen_mute_icon = Image::from_icon_name("audio-volume-high-symbolic");
    fullscreen_mute_icon.set_pixel_size(18);
    let fullscreen_mute = Button::new();
    fullscreen_mute.set_child(Some(&fullscreen_mute_icon));
    fullscreen_mute.add_css_class("player-media-action");
    fullscreen_mute.set_tooltip_text(Some("Mute player"));
    fullscreen_mute.update_property(&[gtk::accessible::Property::Label("Mute player")]);
    let fullscreen_volume = Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 1.0);
    fullscreen_volume.add_css_class("player-timeline");
    fullscreen_volume.add_css_class("fullscreen-volume-slider");
    fullscreen_volume.set_draw_value(false);
    fullscreen_volume.set_value(100.0);
    fullscreen_volume.set_size_request(130, -1);
    fullscreen_volume.set_tooltip_text(Some("Player volume"));
    fullscreen_volume.update_property(&[gtk::accessible::Property::Label("Player volume")]);
    let fullscreen_time = Label::new(Some("00:00 / 00:00"));
    fullscreen_time.add_css_class("player-time");
    fullscreen_time.add_css_class("fullscreen-time");
    fullscreen_time.set_margin_start(12);
    let fullscreen_row_spacer = GtkBox::new(Orientation::Horizontal, 0);
    fullscreen_row_spacer.set_hexpand(true);
    let fullscreen_exit_label = Label::new(Some("Exit fullscreen"));
    fullscreen_exit_label.add_css_class("fullscreen-exit-label");
    let fullscreen_exit_icon = Image::from_icon_name("view-restore-symbolic");
    fullscreen_exit_icon.set_pixel_size(20);
    let fullscreen_exit = Button::new();
    fullscreen_exit.set_child(Some(&fullscreen_exit_icon));
    fullscreen_exit.add_css_class("player-media-action");
    fullscreen_exit.add_css_class("fullscreen-exit-action");
    fullscreen_exit.set_tooltip_text(Some("Exit fullscreen"));
    fullscreen_exit.update_property(&[
        gtk::accessible::Property::Label("Exit fullscreen"),
        gtk::accessible::Property::KeyShortcuts("Escape"),
    ]);
    fullscreen_row.append(&fullscreen_play_pause);
    fullscreen_row.append(&fullscreen_previous);
    fullscreen_row.append(&fullscreen_next);
    fullscreen_row.append(&fullscreen_mute);
    fullscreen_row.append(&fullscreen_volume);
    fullscreen_row.append(&fullscreen_time);
    fullscreen_row.append(&fullscreen_row_spacer);
    fullscreen_row.append(&fullscreen_exit_label);
    fullscreen_row.append(&fullscreen_exit);
    fullscreen_controls.append(&fullscreen_row);
    fullscreen_root.add_overlay(&fullscreen_controls);

    let player = Rc::new(PlayerState {
        media,
        stage,
        fullscreen_root,
        fullscreen_window: RefCell::new(None),
        windowed_child: RefCell::new(None),
        title,
        meta,
        current: RefCell::new(None),
        library_directory: RefCell::new(library_directory.to_owned()),
        playlist: RefCell::new(Vec::new()),
        current_index: Cell::new(None),
        return_page: RefCell::new("library".to_owned()),
        previous,
        next,
        play_pause,
        play_pause_icon,
        timeline,
        time,
        volume,
        volume_value,
        mute,
        mute_icon,
        fullscreen,
        fullscreen_controls,
        fullscreen_previous,
        fullscreen_next,
        fullscreen_play_pause,
        fullscreen_play_pause_icon,
        fullscreen_timeline,
        fullscreen_time,
        fullscreen_volume,
        fullscreen_mute,
        fullscreen_mute_icon,
        fullscreen_hide_source: RefCell::new(None),
        updating_timeline: Cell::new(false),
        updating_volume: Cell::new(false),
        last_audible_volume: Cell::new(100.0),
    });

    let back_player = player.clone();
    let back_stack = stack.clone();
    back.connect_clicked(move |_| {
        let return_page = back_player.return_page.borrow().clone();
        back_player.stop();
        back_stack.set_visible_child_name(&return_page);
    });

    let reveal_player = player.clone();
    reveal.connect_clicked(move |_| {
        let directory = reveal_player.library_directory.borrow().clone();
        let _ = Command::new("xdg-open")
            .arg(directory)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    });
    let edit_player = player.clone();
    let edit_controller = editor.clone();
    edit.connect_clicked(move |_| {
        let Some(clip) = edit_player.current.borrow().clone() else {
            return;
        };
        let return_page = edit_player.return_page.borrow().clone();
        edit_player.media.pause();
        edit_controller.open_returning_to(&clip, &return_page);
    });

    let previous_player = player.clone();
    player
        .previous
        .connect_clicked(move |_| previous_player.step(-1));
    let next_player = player.clone();
    player.next.connect_clicked(move |_| next_player.step(1));
    let playback_player = player.clone();
    player
        .play_pause
        .connect_clicked(move |_| _ = playback_player.toggle_playback());
    let stage_player = player.clone();
    let stage_click = GestureClick::new();
    stage_click.connect_released(move |_, _, _, _| _ = stage_player.toggle_playback());
    player.stage.add_controller(stage_click);

    let seek_player = player.clone();
    player.timeline.connect_value_changed(move |scale| {
        if seek_player.updating_timeline.get() || !seek_player.media.is_seekable() {
            return;
        }
        seek_player
            .media
            .seek((scale.value() * 1_000_000.0).round() as i64);
    });
    let volume_player = player.clone();
    player.volume.connect_value_changed(move |scale| {
        let value = scale.value().clamp(0.0, 100.0);
        if value > 0.0 {
            volume_player.last_audible_volume.set(value);
        }
        volume_player.apply_audio();
        volume_player.update_controls();
    });
    let prepared_player = Rc::downgrade(&player);
    player.media.connect_prepared_notify(move |_| {
        if let Some(player) = prepared_player.upgrade() {
            player.apply_audio();
        }
    });
    let mute_player = player.clone();
    player
        .mute
        .connect_clicked(move |_| mute_player.toggle_mute());
    let fullscreen_player = player.clone();
    player.fullscreen.connect_clicked(move |_| {
        fullscreen_player.toggle_fullscreen();
        reveal_fullscreen_controls(&fullscreen_player);
    });
    let fullscreen_previous_player = player.clone();
    player.fullscreen_previous.connect_clicked(move |_| {
        fullscreen_previous_player.step(-1);
        reveal_fullscreen_controls(&fullscreen_previous_player);
    });
    let fullscreen_next_player = player.clone();
    player.fullscreen_next.connect_clicked(move |_| {
        fullscreen_next_player.step(1);
        reveal_fullscreen_controls(&fullscreen_next_player);
    });
    let fullscreen_playback_player = player.clone();
    player.fullscreen_play_pause.connect_clicked(move |_| {
        _ = fullscreen_playback_player.toggle_playback();
        reveal_fullscreen_controls(&fullscreen_playback_player);
    });
    let fullscreen_stage_player = Rc::downgrade(&player);
    let fullscreen_stage_click = GestureClick::new();
    fullscreen_stage_click.connect_released(move |_, _, _, _| {
        if let Some(player) = fullscreen_stage_player.upgrade() {
            _ = player.toggle_playback();
            reveal_fullscreen_controls(&player);
        }
    });
    fullscreen_stage.add_controller(fullscreen_stage_click);
    let fullscreen_seek_player = player.clone();
    player
        .fullscreen_timeline
        .connect_value_changed(move |scale| {
            if fullscreen_seek_player.updating_timeline.get()
                || !fullscreen_seek_player.media.is_seekable()
            {
                return;
            }
            fullscreen_seek_player
                .media
                .seek((scale.value() * 1_000_000.0).round() as i64);
            reveal_fullscreen_controls(&fullscreen_seek_player);
        });
    let fullscreen_volume_player = player.clone();
    player
        .fullscreen_volume
        .connect_value_changed(move |scale| {
            if fullscreen_volume_player.updating_volume.get() {
                return;
            }
            fullscreen_volume_player
                .volume
                .set_value(scale.value().clamp(0.0, 100.0));
            reveal_fullscreen_controls(&fullscreen_volume_player);
        });
    let fullscreen_mute_player = player.clone();
    player.fullscreen_mute.connect_clicked(move |_| {
        fullscreen_mute_player.toggle_mute();
        reveal_fullscreen_controls(&fullscreen_mute_player);
    });
    let fullscreen_exit_player = Rc::downgrade(&player);
    fullscreen_exit.connect_clicked(move |_| {
        if let Some(player) = fullscreen_exit_player.upgrade() {
            player.exit_fullscreen();
        }
    });
    let fullscreen_motion_player = Rc::downgrade(&player);
    let fullscreen_motion = EventControllerMotion::new();
    fullscreen_motion.connect_motion(move |_, _, _| {
        if let Some(player) = fullscreen_motion_player.upgrade() {
            reveal_fullscreen_controls(&player);
        }
    });
    player.fullscreen_root.add_controller(fullscreen_motion);

    let update_player = player.clone();
    let update_stack = stack.clone();
    glib::timeout_add_local(Duration::from_millis(50), move || {
        if update_stack.visible_child_name().as_deref() == Some("player") {
            update_player.update_controls();
        }
        ControlFlow::Continue
    });
    (page, player)
}

fn show_player(player: &PlayerState, clip: &Clip, playlist: Vec<Clip>, return_page: &str) {
    player.show(clip, playlist, return_page);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_time_uses_windows_minute_second_format() {
        assert_eq!(format_player_time(0.0), "00:00");
        assert_eq!(format_player_time(65.4), "01:05");
        assert_eq!(format_player_time(3_661.0), "61:01");
    }

    #[test]
    fn preview_volume_drives_both_stream_gain_and_real_mute() {
        assert_eq!(player_audio_settings(0.0), (0.0, true));
        assert_eq!(player_audio_settings(35.0), (0.35, false));
        assert_eq!(player_audio_settings(150.0), (1.0, false));
    }

    #[test]
    fn negative_media_timestamps_are_clamped() {
        assert_eq!(media_seconds(-1), 0.0);
        assert_eq!(media_seconds(1_500_000), 1.5);
    }
}
