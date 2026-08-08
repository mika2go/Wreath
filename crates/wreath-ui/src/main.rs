mod editor;
mod home;
mod library;
mod settings;
mod theme;

use std::cell::Cell;
use std::process::ExitCode;
use std::rc::Rc;
use std::time::Duration;

use gtk::gdk;
use gtk::glib::{self, ControlFlow};
use gtk::prelude::*;
use gtk::{
    Align, Application, ApplicationWindow, Box as GtkBox, Button, CssProvider, Image, Orientation,
    Stack,
};

const APP_ID: &str = "io.github.mika2go.Wreath";

fn main() -> ExitCode {
    let application = Application::builder().application_id(APP_ID).build();
    application.connect_startup(|_| install_css());
    application.connect_activate(build_ui);
    application.run().into()
}

fn install_css() {
    let provider = CssProvider::new();
    load_css(&provider);
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
    let provider = provider.clone();
    theme::watch_palette_changes(move || load_css(&provider));
}

fn load_css(provider: &CssProvider) {
    let stylesheet = format!(
        "{}\n{}",
        theme::Palette::discover().css_prefix(),
        include_str!("style.css")
    );
    provider.load_from_string(&stylesheet);
}

fn build_ui(application: &Application) {
    if let Some(window) = application.active_window() {
        window.present();
        return;
    }
    let window = ApplicationWindow::builder()
        .application(application)
        .title("Wreath")
        .default_width(1280)
        .default_height(760)
        .resizable(true)
        .build();
    window.add_css_class("wreath-window");

    let shell = GtkBox::new(Orientation::Horizontal, 0);
    let sidebar = GtkBox::new(Orientation::Vertical, 0);
    sidebar.add_css_class("sidebar");
    sidebar.set_size_request(62, -1);

    let brand = GtkBox::new(Orientation::Horizontal, 0);
    brand.add_css_class("brand");
    brand.set_halign(Align::Center);
    let mark = Image::from_icon_name("io.github.mika2go.Wreath-symbolic");
    mark.set_pixel_size(24);
    mark.add_css_class("brand-mark");
    brand.append(&mark);
    sidebar.append(&brand);

    let home_nav = nav_button("Home", "user-home-symbolic");
    home_nav.add_css_class("active");
    let library_nav = nav_button("Library", "video-display-symbolic");
    let collections_nav = nav_button("Collections", "folder-symbolic");
    let settings_nav = nav_button("Settings", "preferences-system-symbolic");
    sidebar.append(&home_nav);
    sidebar.append(&library_nav);
    sidebar.append(&collections_nav);
    sidebar.append(&settings_nav);

    let sidebar_spacer = GtkBox::new(Orientation::Vertical, 0);
    sidebar_spacer.set_vexpand(true);
    sidebar.append(&sidebar_spacer);

    let content = Stack::new();
    content.add_css_class("content-area");
    content.set_hexpand(true);
    content.set_vexpand(true);
    content.set_hhomogeneous(false);
    content.set_vhomogeneous(false);
    content.set_transition_type(gtk::StackTransitionType::Crossfade);
    content.set_transition_duration(140);

    let home_view = home::build();
    let clip_views = library::build(&content);
    let settings_page = settings::build();
    content.add_named(&home_view.page, Some("home"));
    content.add_named(&clip_views.library, Some("library"));
    content.add_named(&clip_views.collections, Some("collections"));
    content.add_named(&clip_views.player, Some("player"));
    content.add_named(&clip_views.editor, Some("editor"));
    content.add_named(&settings_page.page, Some("settings"));

    let home_stack = content.clone();
    let home_button = home_nav.clone();
    let library_button = library_nav.clone();
    let collections_button = collections_nav.clone();
    let settings_button = settings_nav.clone();
    home_nav.connect_clicked(move |_| {
        home_stack.set_visible_child_name("home");
        set_active_nav(
            &home_button,
            &[&library_button, &collections_button, &settings_button],
        );
    });

    let library_stack = content.clone();
    let home_button = home_nav.clone();
    let library_button = library_nav.clone();
    let collections_button = collections_nav.clone();
    let settings_button = settings_nav.clone();
    let library_views = clip_views.clone();
    library_nav.connect_clicked(move |_| {
        library_views.refresh();
        library_stack.set_visible_child_name("library");
        library_button.add_css_class("active");
        home_button.remove_css_class("active");
        collections_button.remove_css_class("active");
        settings_button.remove_css_class("active");
    });

    let collections_stack = content.clone();
    let home_button = home_nav.clone();
    let library_button = library_nav.clone();
    let collections_button = collections_nav.clone();
    let settings_button = settings_nav.clone();
    let collection_views = clip_views.clone();
    collections_nav.connect_clicked(move |_| {
        collection_views.refresh();
        collections_stack.set_visible_child_name("collections");
        collections_button.add_css_class("active");
        home_button.remove_css_class("active");
        library_button.remove_css_class("active");
        settings_button.remove_css_class("active");
    });

    let settings_stack = content.clone();
    let home_button = home_nav.clone();
    let library_button = library_nav.clone();
    let collections_button = collections_nav.clone();
    let settings_button = settings_nav.clone();
    settings_nav.connect_clicked(move |_| {
        settings_stack.set_visible_child_name("settings");
        settings_button.add_css_class("active");
        home_button.remove_css_class("active");
        library_button.remove_css_class("active");
        collections_button.remove_css_class("active");
    });

    let quick_library_stack = content.clone();
    let quick_home = home_nav.clone();
    let quick_library = library_nav.clone();
    let quick_collections = collections_nav.clone();
    let quick_settings = settings_nav.clone();
    let quick_library_views = clip_views.clone();
    home_view.open_library.connect_clicked(move |_| {
        quick_library_views.refresh();
        quick_library_stack.set_visible_child_name("library");
        set_active_nav(
            &quick_library,
            &[&quick_home, &quick_collections, &quick_settings],
        );
    });

    let quick_collections_stack = content.clone();
    let quick_home = home_nav.clone();
    let quick_library = library_nav.clone();
    let quick_collections = collections_nav.clone();
    let quick_settings = settings_nav.clone();
    let quick_collection_views = clip_views.clone();
    home_view.open_collections.connect_clicked(move |_| {
        quick_collection_views.refresh();
        quick_collections_stack.set_visible_child_name("collections");
        set_active_nav(
            &quick_collections,
            &[&quick_home, &quick_library, &quick_settings],
        );
    });

    shell.append(&sidebar);
    shell.append(&content);
    window.set_child(Some(&shell));
    content.set_visible_child_name("home");
    window.present();
    install_responsive_layout(
        &window,
        &sidebar,
        &content,
        &home_view,
        &clip_views,
        &settings_page,
    );
}

fn nav_button(label: &str, icon: &str) -> Button {
    let icon = Image::from_icon_name(icon);
    icon.set_pixel_size(18);
    icon.add_css_class("nav-icon");
    let button = Button::new();
    button.add_css_class("nav-button");
    button.set_halign(Align::Center);
    button.set_tooltip_text(Some(label));
    button.set_child(Some(&icon));
    button
}

fn set_active_nav(active: &Button, inactive: &[&Button]) {
    active.add_css_class("active");
    for button in inactive {
        button.remove_css_class("active");
    }
}

fn install_responsive_layout(
    window: &ApplicationWindow,
    sidebar: &GtkBox,
    content: &Stack,
    home_view: &home::HomeView,
    clip_views: &library::ClipViews,
    settings_view: &settings::SettingsView,
) {
    let window = window.downgrade();
    let sidebar = sidebar.clone();
    let content = content.clone();
    let home_view = home_view.clone();
    let clip_views = clip_views.clone();
    let settings_view = settings_view.clone();
    let previous = Rc::new(Cell::new((false, false, false, 0_u32)));

    glib::timeout_add_local(Duration::from_millis(100), move || {
        let Some(window) = window.upgrade() else {
            return ControlFlow::Break;
        };
        let width = window.width();
        let compact_sidebar = width < 760;
        let narrow_content = width < 980;
        let compact_header = width < 820;
        let clip_columns = if width >= 1_500 {
            6
        } else if width >= 1_100 {
            4
        } else if width >= 820 {
            3
        } else if width >= 620 {
            2
        } else {
            1
        };
        let current = (
            compact_sidebar,
            narrow_content,
            compact_header,
            clip_columns,
        );
        if previous.get() == current {
            return ControlFlow::Continue;
        }
        previous.set(current);

        sidebar.set_size_request(if compact_sidebar { 54 } else { 62 }, -1);
        set_css_class(&content, "narrow", narrow_content);
        set_css_class(&content, "very-narrow", compact_header);
        clip_views.set_layout(compact_header, clip_columns);
        home_view.set_compact(compact_header);
        settings_view.set_compact(compact_header);
        content.set_hhomogeneous(compact_header);
        ControlFlow::Continue
    });
}

fn set_css_class(widget: &impl IsA<gtk::Widget>, class_name: &str, enabled: bool) {
    if enabled {
        widget.add_css_class(class_name);
    } else {
        widget.remove_css_class(class_name);
    }
}
