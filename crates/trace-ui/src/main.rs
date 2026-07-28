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
    Align, Application, ApplicationWindow, Box as GtkBox, Button, CssProvider, Image, Label,
    Orientation, Stack,
};

const APP_ID: &str = "io.github.mika2go.Trace";

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
        .title("Trace")
        .default_width(1180)
        .default_height(760)
        .resizable(true)
        .build();
    window.add_css_class("trace-window");

    let shell = GtkBox::new(Orientation::Horizontal, 0);
    let sidebar = GtkBox::new(Orientation::Vertical, 0);
    sidebar.add_css_class("sidebar");
    sidebar.set_size_request(194, -1);

    let brand = GtkBox::new(Orientation::Horizontal, 10);
    brand.add_css_class("brand");
    let mark = Image::from_icon_name("io.github.mika2go.Trace-symbolic");
    mark.set_pixel_size(24);
    mark.add_css_class("brand-mark");
    let brand_name = Label::new(Some("TRACE"));
    brand_name.add_css_class("brand-name");
    brand.append(&mark);
    brand.append(&brand_name);
    sidebar.append(&brand);

    let nav_label = Label::new(Some("LIBRARY"));
    nav_label.add_css_class("nav-label");
    nav_label.set_halign(Align::Start);
    sidebar.append(&nav_label);

    let clips_nav = nav_button("Clips", "video-display-symbolic");
    clips_nav.button.add_css_class("active");
    let settings_nav = nav_button("Settings", "preferences-system-symbolic");
    sidebar.append(&clips_nav.button);
    sidebar.append(&settings_nav.button);

    let sidebar_spacer = GtkBox::new(Orientation::Vertical, 0);
    sidebar_spacer.set_vexpand(true);
    sidebar.append(&sidebar_spacer);
    let privacy = Label::new(Some("LOCAL ONLY  ·  OFFLINE"));
    privacy.add_css_class("privacy");
    privacy.set_halign(Align::Start);
    sidebar.append(&privacy);

    let content = Stack::new();
    content.add_css_class("content-area");
    content.set_hexpand(true);
    content.set_vexpand(true);
    content.set_hhomogeneous(false);
    content.set_vhomogeneous(false);
    content.set_transition_type(gtk::StackTransitionType::Crossfade);
    content.set_transition_duration(140);

    let clip_views = library::build(&content);
    let settings_page = settings::build();
    content.add_named(&clip_views.library, Some("clips"));
    content.add_named(&clip_views.player, Some("player"));
    content.add_named(&settings_page.page, Some("settings"));
    content.set_visible_child_name("clips");

    let clips_stack = content.clone();
    let clips_button = clips_nav.button.clone();
    let settings_button = settings_nav.button.clone();
    clips_nav.button.connect_clicked(move |_| {
        clips_stack.set_visible_child_name("clips");
        clips_button.add_css_class("active");
        settings_button.remove_css_class("active");
    });

    let settings_stack = content.clone();
    let clips_button = clips_nav.button.clone();
    let settings_button = settings_nav.button.clone();
    settings_nav.button.connect_clicked(move |_| {
        settings_stack.set_visible_child_name("settings");
        settings_button.add_css_class("active");
        clips_button.remove_css_class("active");
    });

    shell.append(&sidebar);
    shell.append(&content);
    window.set_child(Some(&shell));
    window.present();
    install_responsive_layout(
        &window,
        &sidebar,
        &content,
        &[&brand_name, &nav_label, &privacy],
        &[&clips_nav.label, &settings_nav.label],
        &clip_views,
        &settings_page,
    );
}

struct NavButton {
    button: Button,
    label: Label,
}

fn nav_button(label: &str, icon: &str) -> NavButton {
    let row = GtkBox::new(Orientation::Horizontal, 12);
    let icon = Image::from_icon_name(icon);
    icon.set_pixel_size(18);
    icon.add_css_class("nav-icon");
    let label_widget = Label::new(Some(label));
    label_widget.add_css_class("nav-text");
    label_widget.set_halign(Align::Start);
    row.append(&icon);
    row.append(&label_widget);
    let button = Button::new();
    button.add_css_class("nav-button");
    button.set_tooltip_text(Some(label));
    button.set_child(Some(&row));
    NavButton {
        button,
        label: label_widget,
    }
}

fn install_responsive_layout(
    window: &ApplicationWindow,
    sidebar: &GtkBox,
    content: &Stack,
    sidebar_details: &[&Label],
    nav_labels: &[&Label],
    clip_views: &library::ClipViews,
    settings_view: &settings::SettingsView,
) {
    let window = window.downgrade();
    let sidebar = sidebar.clone();
    let content = content.clone();
    let sidebar_details = sidebar_details
        .iter()
        .map(|label| (*label).clone())
        .collect::<Vec<_>>();
    let nav_labels = nav_labels
        .iter()
        .map(|label| (*label).clone())
        .collect::<Vec<_>>();
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
        let clip_columns = if width >= 1_120 {
            3
        } else if width >= 700 {
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

        sidebar.set_size_request(if compact_sidebar { 68 } else { 194 }, -1);
        if compact_sidebar {
            sidebar.add_css_class("compact");
        } else {
            sidebar.remove_css_class("compact");
        }
        for label in sidebar_details.iter().chain(nav_labels.iter()) {
            label.set_visible(!compact_sidebar);
        }
        set_css_class(&content, "narrow", narrow_content);
        set_css_class(&content, "very-narrow", compact_header);
        clip_views.set_layout(compact_header, clip_columns);
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
