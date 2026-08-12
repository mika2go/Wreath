use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use gtk::gdk;
use gtk::glib::{self, ControlFlow};
use gtk::prelude::*;
use gtk::{
    Align, Box as GtkBox, Button, DrawingArea, EventControllerKey, GestureDrag, Label, Orientation,
    Stack, Video, gio,
};
use wreath_core::clips::Clip;
use wreath_core::paths::AppPaths;
use wreath_core::trim::{self, ClipTiming, TrimMode, TrimOutput, TrimReport, TrimRequest};

enum EditorUpdate {
    Timing {
        source: PathBuf,
        result: Result<ClipTiming, String>,
    },
    Finished {
        source: PathBuf,
        replacing: bool,
        result: Result<TrimReport, String>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Handle {
    Start,
    End,
    Playhead,
}

pub struct EditorView {
    pub page: GtkBox,
    pub controller: Rc<EditorController>,
}

pub struct EditorController {
    stack: Stack,
    video: Video,
    title: Label,
    detail: Label,
    timeline: DrawingArea,
    start: Rc<Cell<f64>>,
    end: Rc<Cell<f64>>,
    playhead: Rc<Cell<f64>>,
    selection: Label,
    feedback: Label,
    save: Button,
    replace: Button,
    source: RefCell<Option<PathBuf>>,
    return_page: RefCell<String>,
    timing: Rc<RefCell<Option<ClipTiming>>>,
    on_complete: RefCell<Option<Box<dyn Fn()>>>,
    updates: mpsc::Sender<EditorUpdate>,
}

impl EditorController {
    pub fn open(&self, clip: &Clip) {
        let return_page = self
            .stack
            .visible_child_name()
            .unwrap_or_else(|| "library".into());
        self.open_returning_to(clip, return_page.as_str());
    }

    pub fn open_returning_to(&self, clip: &Clip, return_page: &str) {
        self.return_page.replace(return_page.to_owned());
        self.source.replace(Some(clip.path.clone()));
        self.timing.replace(None);
        self.title.set_text(&format!("Edit {}", clip.title));
        self.detail.set_text("Reading duration and keyframes…");
        self.feedback.set_text("Finding clean cut points");
        self.feedback.remove_css_class("success");
        self.feedback.remove_css_class("error");
        self.timeline.set_sensitive(false);
        self.save.set_sensitive(false);
        self.save.set_label("Save as new clip");
        self.replace.set_sensitive(false);
        self.replace.set_label("Save as original");
        self.start.set(0.0);
        self.end.set(0.0);
        self.playhead.set(0.0);
        self.timeline.queue_draw();
        self.video.set_file(Some(&gio::File::for_path(&clip.path)));
        self.stack.set_visible_child_name("editor");

        let source = clip.path.clone();
        let updates = self.updates.clone();
        let _ = std::thread::Builder::new()
            .name("wreath-editor-timing".into())
            .spawn(move || {
                let backend = wreath_core::trim_ffmpeg::FfmpegTrimmer;
                let result = trim::timing(&backend, &source).map_err(|error| error.to_string());
                let _ = updates.send(EditorUpdate::Timing { source, result });
            });
    }

    pub fn set_on_complete(&self, callback: impl Fn() + 'static) {
        self.on_complete.replace(Some(Box::new(callback)));
    }

    pub fn stop(&self) {
        if let Some(stream) = self.video.media_stream() {
            stream.pause();
        }
        self.video.set_file(None::<&gio::File>);
        self.source.replace(None);
        self.timing.replace(None);
        self.timeline.set_sensitive(false);
        self.save.set_sensitive(false);
        self.replace.set_sensitive(false);
    }

    pub fn toggle_playback(&self) -> bool {
        let Some(stream) = self.video.media_stream() else {
            return false;
        };
        if stream.is_playing() {
            stream.pause();
        } else {
            stream.play();
        }
        true
    }

    fn apply_timing(&self, timing: ClipTiming) {
        let seconds = timing.duration.as_secs_f64().max(0.001);
        self.start.set(0.0);
        self.end.set(seconds);
        self.timeline.set_sensitive(true);
        self.save.set_sensitive(true);
        self.replace.set_sensitive(true);
        self.detail.set_text("Choose the moment to keep");
        self.feedback
            .set_text("Handles snap to nearby keyframes for a lossless cut");
        self.timing.replace(Some(timing));
        self.update_selection();
        self.restart_preview();
    }

    fn update_selection(&self) {
        let start = duration(self.start.get());
        let end = duration(self.end.get());
        let value = format!(
            "{} — {}  ·  {} kept",
            format_time(start),
            format_time(end),
            format_time(end.saturating_sub(start))
        );
        self.selection.set_text(&value);
        self.timeline
            .update_property(&[gtk::accessible::Property::ValueText(&value)]);
        self.timeline.queue_draw();
    }

    fn snap(&self, value: f64) -> f64 {
        let position = duration(value);
        self.timing
            .borrow()
            .as_ref()
            .and_then(|timing| timing.nearest_keyframe(position))
            .filter(|keyframe| keyframe.abs_diff(position) <= trim::SNAP_TOLERANCE)
            .map_or(value, |keyframe| keyframe.as_secs_f64())
    }

    fn move_handle(&self, handle: Handle, value: f64) {
        let Some(timing) = self.timing.borrow().as_ref().cloned() else {
            return;
        };
        let value = value.clamp(0.0, timing.duration.as_secs_f64());
        if matches!(handle, Handle::Playhead) {
            let value = value.clamp(self.start.get(), self.end.get());
            self.playhead.set(value);
            if let Some(stream) = self.video.media_stream() {
                stream.seek((value * 1_000_000.0).round() as i64);
            }
            self.timeline.queue_draw();
            return;
        }
        let value = self.snap(value);
        let minimum = trim::MINIMUM_LENGTH.as_secs_f64();
        match handle {
            Handle::Start => self
                .start
                .set(value.min((self.end.get() - minimum).max(0.0))),
            Handle::End => self.end.set(
                value
                    .max(self.start.get() + minimum)
                    .min(timing.duration.as_secs_f64()),
            ),
            Handle::Playhead => unreachable!(),
        }
        self.update_selection();
        self.restart_preview();
    }

    fn restart_preview(&self) {
        self.playhead.set(self.start.get());
        if let Some(stream) = self.video.media_stream() {
            stream.seek((self.start.get() * 1_000_000.0).round() as i64);
            stream.play();
        }
    }

    fn keep_preview_inside_selection(&self) {
        let Some(stream) = self.video.media_stream() else {
            return;
        };
        let position = stream.timestamp() as f64 / 1_000_000.0;
        self.playhead.set(position);
        self.timeline.queue_draw();
        if position + 0.02 < self.start.get() || position >= self.end.get() - 0.02 {
            stream.seek((self.start.get() * 1_000_000.0).round() as i64);
            stream.play();
        }
    }

    fn save(&self, output: TrimOutput) {
        let Some(source) = self.source.borrow().clone() else {
            return;
        };
        let start = duration(self.start.get());
        let end = duration(self.end.get());
        if end.saturating_sub(start) < trim::MINIMUM_LENGTH {
            self.feedback.set_text("Keep at least 0.3 seconds");
            self.feedback.add_css_class("error");
            return;
        }
        self.feedback.remove_css_class("error");
        let replacing = matches!(output, TrimOutput::Replace);
        self.feedback.set_text(if replacing {
            "Replacing the original clip on a background worker…"
        } else {
            "Cutting on a background worker…"
        });
        self.save.set_sensitive(false);
        self.replace.set_sensitive(false);
        if replacing {
            self.replace.set_label("Replacing…");
            if let Some(stream) = self.video.media_stream() {
                stream.pause();
            }
            self.video.set_file(None::<&gio::File>);
        } else {
            self.save.set_label("Cutting…");
        }

        let thumbnails = AppPaths::discover().thumbnail_dir;
        let updates = self.updates.clone();
        let _ = std::thread::Builder::new()
            .name("wreath-editor-cut".into())
            .spawn(move || {
                let backend = wreath_core::trim_ffmpeg::FfmpegTrimmer;
                let request = TrimRequest {
                    source: source.clone(),
                    start,
                    end,
                    mode: TrimMode::Auto,
                    output,
                };
                let result =
                    trim::trim(&backend, &request, &thumbnails).map_err(|error| error.to_string());
                let _ = updates.send(EditorUpdate::Finished {
                    source,
                    replacing,
                    result,
                });
            });
    }

    fn handle_update(&self, update: EditorUpdate) {
        let current = self.source.borrow().clone();
        match update {
            EditorUpdate::Timing { source, result } if current.as_ref() == Some(&source) => {
                match result {
                    Ok(timing) if !timing.duration.is_zero() => self.apply_timing(timing),
                    Ok(_) => self.fail("This clip has no readable duration"),
                    Err(error) => self.fail(&error),
                }
            }
            EditorUpdate::Finished {
                source,
                replacing,
                result,
            } if current.as_ref() == Some(&source) => {
                self.save.set_sensitive(true);
                self.save.set_label("Save as new clip");
                self.replace.set_sensitive(true);
                self.replace.set_label("Save as original");
                match result {
                    Ok(report) => {
                        let name = report.path.file_name().map_or_else(
                            || report.path.display().to_string(),
                            |name| name.to_string_lossy().into_owned(),
                        );
                        self.feedback.remove_css_class("error");
                        self.feedback.add_css_class("success");
                        self.feedback.set_text(&format!(
                            "{} · {name}",
                            if report.reencoded && replacing {
                                "Original replaced and re-encoded for an exact start"
                            } else if replacing {
                                "Original replaced with a lossless cut"
                            } else if report.reencoded {
                                "Re-encoded for an exact start"
                            } else {
                                "Losslessly cut"
                            }
                        ));
                        if let Some(callback) = self.on_complete.borrow().as_ref() {
                            callback();
                        }
                        if replacing {
                            let return_page = self.return_page.borrow().clone();
                            let metadata = std::fs::metadata(&report.path).ok();
                            let clip = Clip {
                                path: report.path,
                                title: self.title.text().to_string(),
                                size_bytes: metadata.as_ref().map_or(0, std::fs::Metadata::len),
                                modified: metadata
                                    .and_then(|value| value.modified().ok())
                                    .unwrap_or_else(std::time::SystemTime::now),
                            };
                            self.open_returning_to(&clip, &return_page);
                        }
                    }
                    Err(error) => {
                        if replacing {
                            self.video.set_file(Some(&gio::File::for_path(&source)));
                        }
                        self.fail(&error);
                    }
                }
            }
            _ => {}
        }
    }

    fn fail(&self, message: &str) {
        self.feedback.remove_css_class("success");
        self.feedback.add_css_class("error");
        self.feedback
            .set_text(&format!("Could not cut clip: {message}"));
        self.save.set_label("Save as new clip");
        self.save.set_sensitive(self.timing.borrow().is_some());
        self.replace.set_label("Save as original");
        self.replace.set_sensitive(self.timing.borrow().is_some());
    }
}

pub fn build(stack: &Stack) -> EditorView {
    let (updates, receiver) = mpsc::channel();
    let page = GtkBox::new(Orientation::Vertical, 0);
    page.add_css_class("editor-page");

    let header = GtkBox::new(Orientation::Horizontal, 14);
    header.set_margin_bottom(18);
    let back = Button::with_label("‹ Back");
    back.add_css_class("back-action");
    let heading = GtkBox::new(Orientation::Vertical, 2);
    heading.set_hexpand(true);
    let title = Label::new(Some("Edit clip"));
    title.add_css_class("player-title");
    title.set_halign(Align::Start);
    let detail = Label::new(Some("Choose the moment to keep"));
    detail.add_css_class("page-subtitle");
    detail.set_halign(Align::Start);
    heading.append(&title);
    heading.append(&detail);
    let save = Button::with_label("Save as new clip");
    save.add_css_class("primary-action");
    save.set_sensitive(false);
    let replace = Button::with_label("Save as original");
    replace.add_css_class("primary-action");
    replace.set_sensitive(false);
    header.append(&back);
    header.append(&heading);
    header.append(&save);
    header.append(&replace);
    page.append(&header);

    let video = Video::new();
    video.add_css_class("editor-video");
    video.set_hexpand(true);
    video.set_vexpand(true);
    video.set_autoplay(false);
    video.set_loop(false);
    page.append(&video);

    let timeline = GtkBox::new(Orientation::Vertical, 9);
    timeline.add_css_class("editor-timeline");
    timeline.set_margin_top(16);
    let timeline_title = Label::new(Some("Keep this moment"));
    timeline_title.add_css_class("editor-kicker");
    timeline_title.set_hexpand(true);
    timeline_title.set_halign(Align::Start);
    let selection = Label::new(Some("— selected"));
    selection.add_css_class("editor-selection");
    selection.set_halign(Align::Start);
    selection.set_wrap(true);
    selection.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    timeline.append(&timeline_title);

    let start = Rc::new(Cell::new(0.0));
    let end = Rc::new(Cell::new(0.0));
    let playhead = Rc::new(Cell::new(0.0));
    let timing = Rc::new(RefCell::new(None::<ClipTiming>));
    let trim_bar = DrawingArea::new();
    trim_bar.add_css_class("editor-trim-bar");
    trim_bar.set_content_height(64);
    trim_bar.set_hexpand(true);
    trim_bar.set_sensitive(false);
    trim_bar.set_focusable(true);
    trim_bar.update_property(&[
        gtk::accessible::Property::Label("Clip trim timeline"),
        gtk::accessible::Property::Description(
            "Arrow keys move the playhead. Shift plus arrow adjusts the start; Control plus arrow adjusts the end.",
        ),
        gtk::accessible::Property::KeyShortcuts(
            "Left Right Shift+Left Shift+Right Control+Left Control+Right",
        ),
    ]);
    {
        let start = start.clone();
        let end = end.clone();
        let playhead = playhead.clone();
        let timing = timing.clone();
        trim_bar.set_draw_func(move |_area, context, width, height| {
            draw_trim_bar(
                context,
                width,
                height,
                start.get(),
                end.get(),
                playhead.get(),
                timing.borrow().as_ref(),
            );
        });
    }
    timeline.append(&trim_bar);
    selection.set_halign(Align::Center);
    timeline.append(&selection);
    page.append(&timeline);

    let footer = GtkBox::new(Orientation::Horizontal, 14);
    footer.set_margin_top(14);
    let feedback = Label::new(Some("Open a clip to read its cut points"));
    feedback.add_css_class("feedback");
    feedback.set_halign(Align::Start);
    feedback.set_hexpand(true);
    feedback.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    footer.append(&feedback);
    page.append(&footer);

    let controller = Rc::new(EditorController {
        stack: stack.clone(),
        video,
        title,
        detail,
        timeline: trim_bar,
        start,
        end,
        playhead,
        selection,
        feedback,
        save,
        replace,
        source: RefCell::new(None),
        return_page: RefCell::new("library".to_owned()),
        timing,
        on_complete: RefCell::new(None),
        updates,
    });

    let back_stack = stack.clone();
    let back_editor = controller.clone();
    back.connect_clicked(move |_| {
        let return_page = back_editor.return_page.borrow().clone();
        back_editor.stop();
        back_stack.set_visible_child_name(&return_page);
    });

    let drag = GestureDrag::new();
    let active_handle = Rc::new(Cell::new(Handle::Start));
    let drag_origin = Rc::new(Cell::new(0.0));
    {
        let editor = controller.clone();
        let active_handle = active_handle.clone();
        let drag_origin = drag_origin.clone();
        drag.connect_drag_begin(move |_gesture, x, _y| {
            let width = f64::from(editor.timeline.width()).max(1.0);
            let duration = editor
                .timing
                .borrow()
                .as_ref()
                .map_or(0.0, |timing| timing.duration.as_secs_f64());
            let (value, handle) =
                drag_target(x, width, duration, editor.start.get(), editor.end.get());
            active_handle.set(handle);
            drag_origin.set(x);
            editor.move_handle(handle, value);
        });
    }
    {
        let editor = controller.clone();
        let active_handle = active_handle.clone();
        let drag_origin = drag_origin.clone();
        drag.connect_drag_update(move |_gesture, offset_x, _offset_y| {
            let width = f64::from(editor.timeline.width()).max(1.0);
            let duration = editor
                .timing
                .borrow()
                .as_ref()
                .map_or(0.0, |timing| timing.duration.as_secs_f64());
            let x = drag_origin.get() + offset_x;
            let value = ((x - 12.0) / (width - 24.0)).clamp(0.0, 1.0) * duration;
            editor.move_handle(active_handle.get(), value);
        });
    }
    controller.timeline.add_controller(drag);

    let keyboard = EventControllerKey::new();
    let keyboard_editor = controller.clone();
    keyboard.connect_key_pressed(move |_, key, _, modifiers| {
        let direction = if key == gdk::Key::Left {
            -1.0
        } else if key == gdk::Key::Right {
            1.0
        } else {
            return glib::Propagation::Proceed;
        };
        if keyboard_editor.timing.borrow().is_none() {
            return glib::Propagation::Proceed;
        }
        let handle = if modifiers.contains(gdk::ModifierType::SHIFT_MASK) {
            Handle::Start
        } else if modifiers.contains(gdk::ModifierType::CONTROL_MASK) {
            Handle::End
        } else {
            Handle::Playhead
        };
        let current = match handle {
            Handle::Start => keyboard_editor.start.get(),
            Handle::End => keyboard_editor.end.get(),
            Handle::Playhead => keyboard_editor.playhead.get(),
        };
        keyboard_editor.move_handle(handle, current + direction * 0.1);
        glib::Propagation::Stop
    });
    controller.timeline.add_controller(keyboard);

    let save_editor = controller.clone();
    controller
        .save
        .connect_clicked(move |_| save_editor.save(TrimOutput::NewClip(None)));
    let replace_editor = controller.clone();
    controller
        .replace
        .connect_clicked(move |_| replace_editor.save(TrimOutput::Replace));

    let update_editor = controller.clone();
    glib::timeout_add_local(Duration::from_millis(33), move || {
        while let Ok(update) = receiver.try_recv() {
            update_editor.handle_update(update);
        }
        if update_editor.stack.visible_child_name().as_deref() == Some("editor")
            && update_editor.timing.borrow().is_some()
        {
            update_editor.keep_preview_inside_selection();
        }
        ControlFlow::Continue
    });

    EditorView { page, controller }
}

fn draw_trim_bar(
    context: &gtk::cairo::Context,
    width: i32,
    height: i32,
    start: f64,
    end: f64,
    playhead: f64,
    timing: Option<&ClipTiming>,
) {
    let left = 12.0;
    let right = f64::from(width) - 12.0;
    let rail_width = (right - left).max(1.0);
    let center = f64::from(height) / 2.0;
    context.set_source_rgb(0.125, 0.125, 0.141);
    context.rectangle(left, center - 6.0, rail_width, 12.0);
    let _ = context.fill();
    let Some(timing) = timing else { return };
    let duration = timing.duration.as_secs_f64().max(0.001);
    let start_x = left + start / duration * rail_width;
    let end_x = left + end / duration * rail_width;
    context.set_source_rgb(0.463, 0.851, 0.639);
    context.rectangle(start_x, center - 6.0, (end_x - start_x).max(0.0), 12.0);
    let _ = context.fill();

    context.set_source_rgba(0.463, 0.851, 0.639, 0.55);
    let stride = timing.keyframes.len().div_ceil(80).max(1);
    for keyframe in timing.keyframes.iter().step_by(stride) {
        let x = left + keyframe.as_secs_f64() / duration * rail_width;
        context.rectangle(x, center + 10.0, 1.0, 5.0);
    }
    let _ = context.fill();

    if playhead >= start && playhead <= end {
        let x = left + playhead / duration * rail_width;
        context.set_source_rgba(0.125, 0.125, 0.141, 0.9);
        context.rectangle(x - 4.0, center - 15.0, 8.0, 30.0);
        let _ = context.fill();
        context.set_source_rgb(0.957, 0.961, 0.976);
        context.rectangle(x - 1.5, center - 17.0, 3.0, 34.0);
        let _ = context.fill();
    }

    context.set_source_rgb(0.957, 0.961, 0.976);
    for x in [start_x, end_x] {
        context.rectangle(x - 5.0, center - 18.0, 10.0, 36.0);
        let _ = context.fill();
    }
}

fn duration(seconds: f64) -> Duration {
    Duration::from_secs_f64(seconds.max(0.0))
}

fn drag_target(x: f64, width: f64, duration: f64, start: f64, end: f64) -> (f64, Handle) {
    let rail_width = (width - 24.0).max(1.0);
    let value = ((x - 12.0) / rail_width).clamp(0.0, 1.0) * duration;
    let pixels_per_second = rail_width / duration.max(0.001);
    let start_distance = (value - start).abs() * pixels_per_second;
    let end_distance = (value - end).abs() * pixels_per_second;
    let handle = if start_distance <= 14.0 {
        Handle::Start
    } else if end_distance <= 14.0 {
        Handle::End
    } else {
        Handle::Playhead
    };
    (value, handle)
}

fn format_time(value: Duration) -> String {
    let total_millis = value.as_millis();
    let minutes = total_millis / 60_000;
    let seconds = total_millis % 60_000 / 1_000;
    let millis = total_millis % 1_000;
    format!("{minutes:02}:{seconds:02}.{millis:03}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_drag_distinguishes_handles_from_playhead() {
        assert_eq!(
            drag_target(12.0, 1024.0, 100.0, 0.0, 100.0).1,
            Handle::Start
        );
        assert_eq!(
            drag_target(1012.0, 1024.0, 100.0, 0.0, 100.0).1,
            Handle::End
        );
        assert_eq!(
            drag_target(512.0, 1024.0, 100.0, 0.0, 100.0).1,
            Handle::Playhead
        );
    }
}
