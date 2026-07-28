mod library;
mod settings;

use std::process::ExitCode;

use gtk::gdk;
use gtk::prelude::*;
use gtk::{
    Align, Application, ApplicationWindow, Box as GtkBox, Button, CssProvider, Label, Orientation,
    Stack,
};
use trace_core::paths::AppPaths;

const APP_ID: &str = "io.github.mika2go.Trace";

fn main() -> ExitCode {
    let application = Application::builder().application_id(APP_ID).build();
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
    let mark = Label::new(Some("◉"));
    mark.add_css_class("brand-mark");
    let brand_name = Label::new(Some("TRACE"));
    brand_name.add_css_class("brand-name");
    brand.append(&mark);
    brand.append(&brand_name);
    sidebar.append(&brand);

    let status = GtkBox::new(Orientation::Horizontal, 8);
    status.add_css_class("capture-status");
    let daemon_running = AppPaths::discover().socket_file.exists();
    let status_dot = Label::new(Some("●"));
    status_dot.add_css_class("status-dot");
    if !daemon_running {
        status_dot.add_css_class("inactive");
    }
    let status_text = Label::new(Some(if daemon_running {
        "Instant replay active"
    } else {
        "Recorder offline"
    }));
    status_text.add_css_class("status-text");
    status.append(&status_dot);
    status.append(&status_text);
    sidebar.append(&status);

    let nav_label = Label::new(Some("LIBRARY"));
    nav_label.add_css_class("nav-label");
    nav_label.set_halign(Align::Start);
    sidebar.append(&nav_label);

    let clips_nav = nav_button("Clips", "⌁");
    clips_nav.add_css_class("active");
    let settings_nav = nav_button("Settings", "⚙");
    sidebar.append(&clips_nav);
    sidebar.append(&settings_nav);

    let sidebar_spacer = GtkBox::new(Orientation::Vertical, 0);
    sidebar_spacer.set_vexpand(true);
    sidebar.append(&sidebar_spacer);
    let privacy = Label::new(Some("LOCAL ONLY  ·  OFFLINE"));
    privacy.add_css_class("privacy");
    privacy.set_halign(Align::Start);
    sidebar.append(&privacy);

    let content = Stack::new();
    content.set_hexpand(true);
    content.set_vexpand(true);
    content.set_transition_type(gtk::StackTransitionType::Crossfade);
    content.set_transition_duration(140);

    let clip_views = library::build(&content);
    let settings_page = settings::build();
    content.add_named(&clip_views.library, Some("clips"));
    content.add_named(&clip_views.player, Some("player"));
    content.add_named(&settings_page, Some("settings"));
    content.set_visible_child_name("clips");

    let clips_stack = content.clone();
    let clips_button = clips_nav.clone();
    let settings_button = settings_nav.clone();
    clips_nav.connect_clicked(move |_| {
        clips_stack.set_visible_child_name("clips");
        clips_button.add_css_class("active");
        settings_button.remove_css_class("active");
    });

    let settings_stack = content.clone();
    let clips_button = clips_nav.clone();
    let settings_button = settings_nav.clone();
    settings_nav.connect_clicked(move |_| {
        settings_stack.set_visible_child_name("settings");
        settings_button.add_css_class("active");
        clips_button.remove_css_class("active");
    });

    shell.append(&sidebar);
    shell.append(&content);
    window.set_child(Some(&shell));
    window.present();
}

fn nav_button(label: &str, icon: &str) -> Button {
    let row = GtkBox::new(Orientation::Horizontal, 12);
    let icon = Label::new(Some(icon));
    icon.add_css_class("nav-icon");
    let label = Label::new(Some(label));
    label.add_css_class("nav-text");
    label.set_halign(Align::Start);
    row.append(&icon);
    row.append(&label);
    let button = Button::new();
    button.add_css_class("nav-button");
    button.set_child(Some(&row));
    button
}
