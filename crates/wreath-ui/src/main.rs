mod editor;
mod home;
mod library;
mod settings;

use std::cell::Cell;
use std::process::ExitCode;
use std::rc::Rc;
use std::time::Duration;

use gtk::glib::{self, ControlFlow};
use gtk::prelude::*;
use gtk::{
    Align, Application, ApplicationWindow, Box as GtkBox, Button, CssProvider, EventControllerKey,
    Image, Label, Orientation, PropagationPhase, Stack,
};
use gtk::{gdk, gio};

const APP_ID: &str = "io.github.mika2go.Wreath";

fn main() -> ExitCode {
    let flags = if std::env::var_os("WREATH_UI_NON_UNIQUE").is_some() {
        gio::ApplicationFlags::NON_UNIQUE
    } else {
        gio::ApplicationFlags::empty()
    };
    let application = Application::builder()
        .application_id(APP_ID)
        .flags(flags)
        .build();
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
    if let Some(window) = application.active_window() {
        window.present();
        return;
    }
    let window = ApplicationWindow::builder()
        .application(application)
        .title("Wreath")
        .default_width(1440)
        .default_height(900)
        .resizable(true)
        .build();
    window.set_size_request(980, 680);
    window.add_css_class("wreath-window");

    let shell = GtkBox::new(Orientation::Horizontal, 0);
    let sidebar = GtkBox::new(Orientation::Vertical, 0);
    sidebar.add_css_class("sidebar");
    sidebar.set_size_request(88, -1);

    let home_nav = nav_button("Home", "user-home-symbolic");
    home_nav.add_css_class("active");
    home_nav.update_state(&[gtk::accessible::State::Selected(Some(true))]);
    let library_nav = nav_button("Library", "video-display-symbolic");
    let collections_nav = nav_button("Collections", "folder-symbolic");
    let settings_nav = nav_button("Settings", "preferences-system-symbolic");
    sidebar.append(&home_nav);
    sidebar.append(&library_nav);
    sidebar.append(&collections_nav);

    let sidebar_spacer = GtkBox::new(Orientation::Vertical, 0);
    sidebar_spacer.set_vexpand(true);
    sidebar.append(&sidebar_spacer);
    sidebar.append(&settings_nav);

    let workspace = GtkBox::new(Orientation::Vertical, 0);
    workspace.add_css_class("workspace");
    workspace.set_hexpand(true);
    workspace.set_vexpand(true);

    let topbar = GtkBox::new(Orientation::Horizontal, 0);
    topbar.add_css_class("topbar");
    let product = GtkBox::new(Orientation::Vertical, 0);
    product.add_css_class("product-mark");
    product.set_hexpand(true);
    let product_context = Label::new(Some("Local capture"));
    product_context.add_css_class("product-context");
    product_context.set_halign(Align::Start);
    let product_name = Label::new(Some("WREATH"));
    product_name.add_css_class("product-name");
    product_name.set_halign(Align::Start);
    product.append(&product_context);
    product.append(&product_name);
    topbar.append(&product);

    let content = Stack::new();
    content.add_css_class("content-area");
    content.set_hexpand(true);
    content.set_vexpand(true);
    content.set_hhomogeneous(false);
    content.set_vhomogeneous(false);
    content.set_transition_type(gtk::StackTransitionType::None);
    content.set_transition_duration(0);

    let home_view = home::build();
    let refreshed_home = home_view.clone();
    let clip_views = library::build(&content, move || refreshed_home.refresh());
    let settings_page = settings::build();

    let search_shell = GtkBox::new(Orientation::Horizontal, 8);
    search_shell.add_css_class("shell-search");
    search_shell.set_valign(Align::Center);
    search_shell.set_visible(false);
    search_shell.set_size_request(244, 38);
    let search = clip_views.search();
    search.set_placeholder_text(Some("Search your clips"));
    search.set_size_request(170, 36);
    search.update_property(&[
        gtk::accessible::Property::Label("Search clips"),
        gtk::accessible::Property::KeyShortcuts("Ctrl+K"),
    ]);
    let search_shortcut = Label::new(Some("Ctrl K"));
    search_shortcut.add_css_class("search-shortcut");
    search_shell.append(&search);
    search_shell.append(&search_shortcut);
    topbar.append(&search_shell);

    content.add_named(&home_view.page, Some("home"));
    content.add_named(&clip_views.library, Some("library"));
    content.add_named(&clip_views.collections, Some("collections"));
    content.add_named(&clip_views.player, Some("player"));
    content.add_named(&clip_views.editor, Some("editor"));
    content.add_named(&settings_page.page, Some("settings"));
    workspace.append(&topbar);
    workspace.append(&content);

    let search_visibility = search_shell.clone();
    content.connect_visible_child_name_notify(move |stack| {
        search_visibility.set_visible(stack.visible_child_name().as_deref() == Some("library"));
    });

    let playback_keys = EventControllerKey::new();
    playback_keys.set_propagation_phase(PropagationPhase::Capture);
    let playback_stack = content.clone();
    let playback_views = clip_views.clone();
    let shell_search = search.clone();
    let search_home = home_nav.clone();
    let search_library = library_nav.clone();
    let search_collections = collections_nav.clone();
    let search_settings = settings_nav.clone();
    playback_keys.connect_key_pressed(move |_, key, _, modifiers| {
        if key == gdk::Key::Escape
            && playback_stack.visible_child_name().as_deref() == Some("player")
            && playback_views.exit_player_fullscreen()
        {
            glib::Propagation::Stop
        } else if key == gdk::Key::k && modifiers.contains(gdk::ModifierType::CONTROL_MASK) {
            playback_views.clear_selection();
            playback_views.refresh();
            playback_stack.set_visible_child_name("library");
            set_active_nav(
                &search_library,
                &[&search_home, &search_collections, &search_settings],
            );
            shell_search.grab_focus();
            glib::Propagation::Stop
        } else if key == gdk::Key::space
            && playback_views.toggle_playback(
                playback_stack
                    .visible_child_name()
                    .as_deref()
                    .unwrap_or_default(),
            )
        {
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    window.add_controller(playback_keys);

    let home_stack = content.clone();
    let home_button = home_nav.clone();
    let library_button = library_nav.clone();
    let collections_button = collections_nav.clone();
    let settings_button = settings_nav.clone();
    let home_views = clip_views.clone();
    let home_status = home_view.clone();
    home_nav.connect_clicked(move |_| {
        home_views.clear_selection();
        home_status.refresh();
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
        library_views.clear_selection();
        library_views.refresh();
        library_stack.set_visible_child_name("library");
        set_active_nav(
            &library_button,
            &[&home_button, &collections_button, &settings_button],
        );
    });

    let collections_stack = content.clone();
    let home_button = home_nav.clone();
    let library_button = library_nav.clone();
    let collections_button = collections_nav.clone();
    let settings_button = settings_nav.clone();
    let collection_views = clip_views.clone();
    collections_nav.connect_clicked(move |_| {
        collection_views.clear_selection();
        collection_views.refresh();
        collections_stack.set_visible_child_name("collections");
        set_active_nav(
            &collections_button,
            &[&home_button, &library_button, &settings_button],
        );
    });

    let settings_stack = content.clone();
    let home_button = home_nav.clone();
    let library_button = library_nav.clone();
    let collections_button = collections_nav.clone();
    let settings_button = settings_nav.clone();
    let settings_views = clip_views.clone();
    settings_nav.connect_clicked(move |_| {
        settings_views.clear_selection();
        settings_stack.set_visible_child_name("settings");
        set_active_nav(
            &settings_button,
            &[&home_button, &library_button, &collections_button],
        );
    });

    shell.append(&sidebar);
    shell.append(&workspace);
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
    icon.set_pixel_size(20);
    icon.add_css_class("nav-icon");
    let button = Button::new();
    button.add_css_class("nav-button");
    button.set_halign(Align::Center);
    button.set_tooltip_text(Some(label));
    button.update_property(&[gtk::accessible::Property::Label(label)]);
    button.update_state(&[gtk::accessible::State::Selected(Some(false))]);
    button.set_child(Some(&icon));
    button
}

fn set_active_nav(active: &Button, inactive: &[&Button]) {
    active.add_css_class("active");
    active.update_state(&[gtk::accessible::State::Selected(Some(true))]);
    for button in inactive {
        button.remove_css_class("active");
        button.update_state(&[gtk::accessible::State::Selected(Some(false))]);
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
        let compact_sidebar = width < 1_080;
        let narrow_content = width < 1_080;
        // The Windows layout keeps page headings and actions on one line for
        // every supported window size (the Linux window minimum is 980 px).
        let compact_header = width < 900;
        let clip_columns = clip_columns_for_window(width);
        let collection_columns = collection_columns_for_window(width);
        let current = (
            compact_sidebar,
            narrow_content,
            compact_header,
            clip_columns * 10 + collection_columns,
        );
        clip_views.update_preview_geometry(clip_columns, collection_columns);
        home_view.set_layout(width, window.height());
        settings_view.set_layout(width, window.height());
        if previous.get() == current {
            return ControlFlow::Continue;
        }
        previous.set(current);

        sidebar.set_size_request(if compact_sidebar { 72 } else { 88 }, -1);
        set_css_class(&content, "narrow", narrow_content);
        set_css_class(&content, "very-narrow", compact_header);
        clip_views.set_layout(compact_header, clip_columns, collection_columns);
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

fn windows_page_width(window_width: i32) -> f32 {
    let rail = if window_width < 1_080 { 72.0 } else { 88.0 };
    let padding = if window_width < 1_080 {
        28.0
    } else if window_width < 1_300 {
        36.0
    } else {
        48.0
    };
    (window_width as f32 - rail - padding * 2.0).max(1.0)
}

fn clip_columns_for_width(width: f32) -> u32 {
    if width >= 1_300.0 {
        6
    } else if width >= 900.0 {
        4
    } else if width >= 650.0 {
        3
    } else if width >= 450.0 {
        2
    } else {
        1
    }
}

fn clip_columns_for_window(window_width: i32) -> u32 {
    clip_columns_for_width(windows_page_width(window_width))
}

fn collection_columns_for_window(window_width: i32) -> u32 {
    let page_width = windows_page_width(window_width);
    let sidebar_width = (page_width * 0.24).clamp(170.0, 240.0);
    clip_columns_for_width((page_width - sidebar_width - 26.0).max(1.0))
}

#[cfg(test)]
mod tests {
    use super::{clip_columns_for_window, collection_columns_for_window};

    #[test]
    fn responsive_columns_follow_the_windows_renderer_geometry() {
        assert_eq!(clip_columns_for_window(1_500), 6);
        assert_eq!(clip_columns_for_window(1_440), 4);
        assert_eq!(clip_columns_for_window(1_080), 4);
        assert_eq!(clip_columns_for_window(980), 3);

        assert_eq!(collection_columns_for_window(1_440), 4);
        assert_eq!(collection_columns_for_window(1_280), 3);
        assert_eq!(collection_columns_for_window(1_080), 3);
        assert_eq!(collection_columns_for_window(980), 2);
    }
}
