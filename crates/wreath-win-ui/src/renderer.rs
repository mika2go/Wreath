use std::collections::{HashMap, HashSet};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::Common::{D2D_RECT_F, D2D_SIZE_U, D2D1_COLOR_F};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_HWND_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_ROUNDED_RECT, D2D1CreateFactory, ID2D1Bitmap, ID2D1Factory,
    ID2D1HwndRenderTarget,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_MEASURING_MODE_NATURAL,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
    DWRITE_WORD_WRAPPING_NO_WRAP, DWriteCreateFactory, IDWriteFactory, IDWriteFontCollection,
    IDWriteTextFormat,
};
use windows::Win32::Graphics::Gdi::{DeleteObject, HPALETTE};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, IWICImagingFactory, WICBitmapUseAlpha,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, IBindCtx};
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK, SIIGBF_THUMBNAILONLY,
};
use windows::core::{PCWSTR, w};
use windows_numerics::Vector2;

use crate::model::{Action, Page, SettingsSection, UiModel};

const CANVAS: u32 = 0x0d0d0f;
const STAGE: u32 = 0x101012;
const SURFACE: u32 = 0x17171a;
const SURFACE_HOVER: u32 = 0x202024;
const PRIMARY: u32 = 0xf4f5f9;
const SECONDARY: u32 = 0x777e8e;
const SUCCESS: u32 = 0x76d9a3;
const WIDE_SIDEBAR_BREAKPOINT: f32 = 1080.0;

#[derive(Debug, Clone, Copy)]
enum Glyph {
    Logo,
    Home,
    Library,
    Collections,
    Settings,
    ChevronDown,
    Close,
}

#[derive(Debug, Clone, Copy)]
enum SettingControl {
    Button,
    Dropdown,
    Toggle,
}

#[derive(Debug, Clone, Copy)]
pub struct LogicalRect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl LogicalRect {
    fn contains(self, x: f32, y: f32) -> bool {
        x >= self.left && x <= self.right && y >= self.top && y <= self.bottom
    }

    fn d2d(self) -> D2D_RECT_F {
        D2D_RECT_F {
            left: self.left,
            top: self.top,
            right: self.right,
            bottom: self.bottom,
        }
    }
}

pub fn player_bounds(width: u32, height: u32) -> LogicalRect {
    let width = width as f32;
    let rail = sidebar_width(width);
    let padding = if width < 820.0 {
        16.0
    } else if width < 980.0 {
        24.0
    } else {
        40.0
    };
    LogicalRect {
        left: rail + padding,
        top: 184.0,
        right: width - padding,
        bottom: (height as f32 - 106.0).max(330.0),
    }
}

#[derive(Clone)]
struct HitRegion {
    rect: LogicalRect,
    action: Action,
}

pub struct Renderer {
    d2d_factory: ID2D1Factory,
    target: Option<ID2D1HwndRenderTarget>,
    title: IDWriteTextFormat,
    heading: IDWriteTextFormat,
    section: IDWriteTextFormat,
    body: IDWriteTextFormat,
    small: IDWriteTextFormat,
    body_center: IDWriteTextFormat,
    hits: Vec<HitRegion>,
    wic_factory: IWICImagingFactory,
    thumbnails: HashMap<PathBuf, ID2D1Bitmap>,
    unavailable_thumbnails: HashSet<PathBuf>,
}

impl Renderer {
    pub fn new() -> Result<Self, String> {
        let d2d_factory =
            unsafe { D2D1CreateFactory::<ID2D1Factory>(D2D1_FACTORY_TYPE_SINGLE_THREADED, None) }
                .map_err(|error| error.to_string())?;
        let write_factory =
            unsafe { DWriteCreateFactory::<IDWriteFactory>(DWRITE_FACTORY_TYPE_SHARED) }
                .map_err(|error| error.to_string())?;
        let title = text_format(&write_factory, 31.0, true, false)?;
        let heading = text_format(&write_factory, 25.0, true, false)?;
        let section = text_format(&write_factory, 17.0, true, false)?;
        let body = text_format(&write_factory, 12.0, false, false)?;
        let small = text_format(&write_factory, 10.0, false, false)?;
        let body_center = text_format(&write_factory, 12.0, false, true)?;
        let wic_factory =
            unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }
                .map_err(|error| error.to_string())?;
        Ok(Self {
            d2d_factory,
            target: None,
            title,
            heading,
            section,
            body,
            small,
            body_center,
            hits: Vec::new(),
            wic_factory,
            thumbnails: HashMap::new(),
            unavailable_thumbnails: HashSet::new(),
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        let resize_failed = self
            .target
            .as_ref()
            .is_some_and(|target| unsafe { target.Resize(&D2D_SIZE_U { width, height }) }.is_err());
        if resize_failed {
            self.target = None;
            self.thumbnails.clear();
        }
    }

    pub fn hit_test(&self, x: f32, y: f32) -> Option<Action> {
        self.hits
            .iter()
            .rev()
            .find(|hit| hit.rect.contains(x, y))
            .map(|hit| hit.action.clone())
    }

    pub fn paint(
        &mut self,
        window: HWND,
        model: &UiModel,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        self.ensure_target(window, width, height)?;
        self.hits.clear();
        let target = self.target.as_ref().expect("render target exists").clone();
        unsafe {
            target.BeginDraw();
            target.Clear(Some(&color(CANVAS)));
        }
        self.render_shell(model, width as f32, height as f32)?;
        if let Some(notice) = &model.notice {
            let rail = sidebar_width(width as f32);
            let notice_area = rect(
                rail + 18.0,
                height as f32 - 62.0,
                width as f32 - 22.0,
                height as f32 - 18.0,
            );
            self.fill(notice_area, SURFACE_HOVER, 10.0)?;
            self.text(
                notice,
                rect(
                    notice_area.left + 16.0,
                    notice_area.top,
                    notice_area.right - 48.0,
                    notice_area.bottom,
                ),
                &self.body.clone(),
                PRIMARY,
            )?;
            let close = rect(
                notice_area.right - 42.0,
                notice_area.top + 5.0,
                notice_area.right - 5.0,
                notice_area.bottom - 5.0,
            );
            self.glyph(Glyph::Close, close, SECONDARY)?;
            self.hits.push(HitRegion {
                rect: close,
                action: Action::DismissNotice,
            });
        }
        unsafe { target.EndDraw(None, None) }.map_err(|error| {
            self.target = None;
            self.thumbnails.clear();
            error.to_string()
        })
    }

    fn ensure_target(&mut self, window: HWND, width: u32, height: u32) -> Result<(), String> {
        if self.target.is_some() {
            return Ok(());
        }
        let properties = D2D1_RENDER_TARGET_PROPERTIES::default();
        let window_properties = D2D1_HWND_RENDER_TARGET_PROPERTIES {
            hwnd: window,
            pixelSize: D2D_SIZE_U { width, height },
            ..Default::default()
        };
        self.target = Some(
            unsafe {
                self.d2d_factory
                    .CreateHwndRenderTarget(&properties, &window_properties)
            }
            .map_err(|error| error.to_string())?,
        );
        Ok(())
    }

    fn render_shell(&mut self, model: &UiModel, width: f32, height: f32) -> Result<(), String> {
        let wide_sidebar = width >= WIDE_SIDEBAR_BREAKPOINT;
        let rail = sidebar_width(width);
        self.fill(
            LogicalRect {
                left: 0.0,
                top: 0.0,
                right: rail,
                bottom: height,
            },
            STAGE,
            0.0,
        )?;
        self.glyph(
            Glyph::Logo,
            rect(if wide_sidebar { 20.0 } else { 22.0 }, 18.0, 50.0, 48.0),
            PRIMARY,
        )?;
        if wide_sidebar {
            self.text(
                "Wreath",
                rect(62.0, 17.0, rail - 16.0, 49.0),
                &self.section.clone(),
                PRIMARY,
            )?;
        }
        let nav = [
            (Page::Home, Glyph::Home, "Home"),
            (Page::Library, Glyph::Library, "Library"),
            (Page::Collections, Glyph::Collections, "Collections"),
            (Page::Settings, Glyph::Settings, "Settings"),
        ];
        for (offset, (page, icon, label)) in nav.iter().enumerate() {
            let top = 88.0 + offset as f32 * 56.0;
            let active =
                model.page == *page || (model.page == Page::Player && model.previous_page == *page);
            let nav_area = LogicalRect {
                left: 10.0,
                top,
                right: rail - 10.0,
                bottom: top + 44.0,
            };
            if active {
                self.fill(nav_area, SURFACE_HOVER, 10.0)?;
            }
            self.glyph(
                *icon,
                rect(
                    if wide_sidebar { 24.0 } else { 25.0 },
                    top + 12.0,
                    if wide_sidebar { 44.0 } else { 47.0 },
                    top + 32.0,
                ),
                if active { PRIMARY } else { SECONDARY },
            )?;
            if wide_sidebar {
                self.text(
                    label,
                    rect(60.0, top, rail - 16.0, top + 44.0),
                    &self.body.clone(),
                    if active { PRIMARY } else { SECONDARY },
                )?;
            }
            self.hits.push(HitRegion {
                rect: nav_area,
                action: Action::Navigate(*page),
            });
        }

        let padding = if width < 980.0 { 24.0 } else { 32.0 };
        let left = rail + padding;
        let right = width - padding;
        match model.page {
            Page::Home => self.render_home(model, left, right, height)?,
            Page::Library => self.render_library(model, left, right, height)?,
            Page::Collections => self.render_collections(model, left, right, height)?,
            Page::Settings => self.render_settings(model, left, right, height)?,
            Page::Player => self.render_player(model, left, right, height)?,
        }
        Ok(())
    }

    fn render_home(
        &mut self,
        model: &UiModel,
        left: f32,
        right: f32,
        height: f32,
    ) -> Result<(), String> {
        let greeting = greeting();
        let user = std::env::var("USERNAME").unwrap_or_else(|_| "there".into());
        self.text(
            &format!("{greeting}, {user}"),
            rect(left, 52.0, right - 156.0, 94.0),
            &self.title.clone(),
            PRIMARY,
        )?;
        self.text(
            "Your replay workspace is ready.",
            rect(left, 98.0, right, 122.0),
            &self.body.clone(),
            SECONDARY,
        )?;
        self.pill(
            rect(right - 132.0, 52.0, right, 92.0),
            SUCCESS,
            "Save replay",
            CANVAS,
            Some(Action::SaveReplay),
        )?;
        let width = right - left;
        let gap = 12.0;
        let card_width = (width - gap * 2.0) / 3.0;
        let stats = [
            ("CLIPS", model.clips.len().to_string()),
            ("COLLECTIONS", model.collections.len().to_string()),
            (
                "REPLAY",
                format!("{} sec", model.config.capture.duration_seconds),
            ),
        ];
        for (index, (label, value)) in stats.iter().enumerate() {
            let x = left + index as f32 * (card_width + gap);
            self.fill(rect(x, 150.0, x + card_width, 232.0), SURFACE, 12.0)?;
            self.text(
                label,
                rect(x + 16.0, 166.0, x + card_width, 184.0),
                &self.small.clone(),
                SECONDARY,
            )?;
            self.text(
                value,
                rect(x + 16.0, 190.0, x + card_width, 222.0),
                &self.section.clone(),
                PRIMARY,
            )?;
        }
        self.text(
            "Recent clips",
            rect(left, 274.0, right, 306.0),
            &self.section.clone(),
            PRIMARY,
        )?;
        self.clip_grid(
            model,
            &model.visible_clip_indices(8),
            left,
            right,
            318.0,
            height - 72.0,
        )
    }

    fn render_library(
        &mut self,
        model: &UiModel,
        left: f32,
        right: f32,
        height: f32,
    ) -> Result<(), String> {
        self.page_heading("Library", "Local replays", left, right)?;
        let search = rect(left, 126.0, (right - 150.0).max(left + 160.0), 166.0);
        self.fill(search, SURFACE, 10.0)?;
        self.text(
            if model.query.is_empty() {
                "Search clips"
            } else {
                &model.query
            },
            rect(search.left + 14.0, 137.0, search.right - 12.0, 158.0),
            &self.body.clone(),
            if model.query.is_empty() {
                SECONDARY
            } else {
                PRIMARY
            },
        )?;
        self.hits.push(HitRegion {
            rect: search,
            action: Action::Search,
        });
        self.pill(
            rect(right - 136.0, 126.0, right, 166.0),
            SURFACE,
            "Refresh",
            PRIMARY,
            Some(Action::Refresh),
        )?;
        self.text(
            &format!(
                "{} clips  •  {}",
                model.clips.len(),
                format_bytes(model.total_size_bytes())
            ),
            rect(left, 184.0, right, 206.0),
            &self.small.clone(),
            SECONDARY,
        )?;
        let indices = model.visible_clip_indices(200);
        if indices.is_empty() {
            self.empty_state(
                if model.query.is_empty() {
                    "No clips yet"
                } else {
                    "No matching clips"
                },
                left,
                right,
                300.0,
            )?;
            return Ok(());
        }
        self.clip_grid(model, &indices, left, right, 226.0, height - 72.0)
    }

    fn render_collections(
        &mut self,
        model: &UiModel,
        left: f32,
        right: f32,
        height: f32,
    ) -> Result<(), String> {
        self.page_heading(
            "Collections",
            "Keep clips grouped without uploads",
            left,
            right,
        )?;
        let sidebar_width = ((right - left) * 0.24).clamp(170.0, 240.0);
        self.pill(
            rect(left, 126.0, left + sidebar_width, 166.0),
            SURFACE,
            "+ New collection",
            PRIMARY,
            Some(Action::CreateCollection),
        )?;
        if model.active_collection.is_some() {
            self.pill(
                rect(left, 170.0, left + sidebar_width, 206.0),
                SURFACE,
                "Delete collection",
                0xe58b8b,
                Some(Action::DeleteActiveCollection),
            )?;
        }
        self.collection_row(
            "All clips",
            model.clips.len(),
            model.active_collection.is_none(),
            rect(left, 218.0, left + sidebar_width, 260.0),
            None,
        )?;
        for (index, collection) in model.collections.iter().take(8).enumerate() {
            let top = 266.0 + index as f32 * 46.0;
            self.collection_row(
                &collection.name,
                collection.clip_count,
                model.active_collection.as_ref() == Some(&collection.path),
                rect(left, top, left + sidebar_width, top + 40.0),
                Some(index),
            )?;
        }
        let content_left = left + sidebar_width + 26.0;
        let title = model
            .active_collection
            .as_ref()
            .and_then(|active| model.collections.iter().find(|item| &item.path == active))
            .map_or("All clips", |collection| collection.name.as_str());
        self.text(
            title,
            rect(content_left, 130.0, right, 160.0),
            &self.section.clone(),
            PRIMARY,
        )?;
        let indices = model.visible_clip_indices(200);
        if indices.is_empty() {
            self.empty_state("This collection is empty", content_left, right, 290.0)?;
        } else {
            self.clip_grid(model, &indices, content_left, right, 184.0, height - 72.0)?;
        }
        Ok(())
    }

    fn render_settings(
        &mut self,
        model: &UiModel,
        left: f32,
        right: f32,
        height: f32,
    ) -> Result<(), String> {
        self.page_heading(
            "Settings",
            "Tune capture without leaving Wreath",
            left,
            right,
        )?;
        let tabs = [
            (SettingsSection::Display, "Display"),
            (SettingsSection::Quality, "Quality"),
            (SettingsSection::Audio, "Audio"),
            (SettingsSection::Controls, "Controls"),
            (SettingsSection::Storage, "Storage"),
        ];
        let mut x = left;
        let tab_width = (((right - left) - 32.0) / 5.0).min(112.0);
        for (section, label) in tabs {
            let tab = rect(x, 126.0, x + tab_width, 164.0);
            if model.settings_section == section {
                self.fill(tab, SURFACE_HOVER, 9.0)?;
            }
            self.text(
                label,
                tab,
                &self.body_center.clone(),
                if model.settings_section == section {
                    PRIMARY
                } else {
                    SECONDARY
                },
            )?;
            self.hits.push(HitRegion {
                rect: tab,
                action: Action::SettingsSection(section),
            });
            x += tab_width + 8.0;
        }
        match model.settings_section {
            SettingsSection::Display => {
                let display_label = model
                    .selected_display()
                    .map_or("Primary display", |display| display.label.as_str());
                self.setting_row(
                    "Capture display",
                    display_label,
                    "Choose a monitor and use its current Windows refresh rate.",
                    left,
                    right,
                    196.0,
                    Action::ChooseDisplay,
                    SettingControl::Dropdown,
                )?;
                self.setting_row(
                    "Frame rate",
                    &format!("{} fps", model.config.capture.frames_per_second),
                    "Available rates follow the selected monitor.",
                    left,
                    right,
                    284.0,
                    Action::ChooseFrameRate,
                    SettingControl::Dropdown,
                )?;
                self.setting_row(
                    "Capture cursor",
                    on_off(model.config.capture.cursor),
                    "Include the pointer in saved clips.",
                    left,
                    right,
                    372.0,
                    Action::ToggleCursor,
                    SettingControl::Toggle,
                )?;
            }
            SettingsSection::Quality => {
                self.setting_row(
                    "Clip length",
                    &format!("{} seconds", model.config.capture.duration_seconds),
                    "How much encoded video stays in memory.",
                    left,
                    right,
                    196.0,
                    Action::ChooseDuration,
                    SettingControl::Dropdown,
                )?;
                self.setting_row(
                    "Codec",
                    &format!("{:?}", model.config.capture.codec),
                    "Hardware encoder selection; Auto is recommended.",
                    left,
                    right,
                    284.0,
                    Action::ChooseCodec,
                    SettingControl::Dropdown,
                )?;
                self.setting_row(
                    "Quality",
                    &format!("{}%", model.config.capture.quality),
                    "Balances image detail and replay memory.",
                    left,
                    right,
                    372.0,
                    Action::ChooseQuality,
                    SettingControl::Dropdown,
                )?;
            }
            SettingsSection::Audio => {
                let microphone_name = model
                    .config
                    .audio
                    .microphone_device
                    .as_ref()
                    .and_then(|id| {
                        model
                            .microphone_names
                            .iter()
                            .find(|(device_id, _)| device_id == id)
                    })
                    .map_or("Windows default", |(_, name)| name.as_str());
                self.setting_row(
                    "Desktop audio",
                    on_off(model.config.audio.desktop),
                    "Record game and system sound.",
                    left,
                    right,
                    196.0,
                    Action::ToggleDesktopAudio,
                    SettingControl::Toggle,
                )?;
                self.setting_row(
                    "Microphone",
                    on_off(model.config.audio.microphone),
                    "Add an input device to each replay.",
                    left,
                    right,
                    284.0,
                    Action::ToggleMicrophone,
                    SettingControl::Toggle,
                )?;
                self.setting_row(
                    "Input device",
                    microphone_name,
                    "Choose an active Windows input endpoint.",
                    left,
                    right,
                    372.0,
                    Action::ChooseMicrophone,
                    SettingControl::Dropdown,
                )?;
                self.setting_row(
                    "Recording level",
                    &format!("{}%", model.config.audio.microphone_gain_percent),
                    "Digital microphone gain from 0 to 200 percent.",
                    left,
                    right,
                    460.0,
                    Action::ChooseMicrophoneGain,
                    SettingControl::Dropdown,
                )?;
            }
            SettingsSection::Controls => {
                let shortcut = if model.hotkey_capture {
                    "Press shortcut…".into()
                } else {
                    model.config.hotkey.to_string()
                };
                self.setting_row(
                    "Save replay",
                    &shortcut,
                    if model.hotkey_capture {
                        "Press the new shortcut now, or Escape to cancel."
                    } else {
                        "Change the global shortcut; no Enter confirmation needed."
                    },
                    left,
                    right,
                    196.0,
                    Action::CaptureHotkey,
                    SettingControl::Button,
                )?;
            }
            SettingsSection::Storage => {
                self.setting_row(
                    "Save location",
                    &model.config.storage.directory.display().to_string(),
                    "Choose a local folder through the Windows picker.",
                    left,
                    right,
                    196.0,
                    Action::ChooseStorage,
                    SettingControl::Button,
                )?;
                self.setting_row(
                    "Storage limit",
                    &format!("{} MiB", model.config.storage.max_megabytes),
                    "Old clips are never uploaded.",
                    left,
                    right,
                    284.0,
                    Action::ChooseStorageLimit,
                    SettingControl::Dropdown,
                )?;
            }
        }
        self.pill(
            rect(right - 150.0, height - 70.0, right, height - 28.0),
            SUCCESS,
            "Save settings",
            CANVAS,
            Some(Action::SaveSettings),
        )
    }

    fn render_player(
        &mut self,
        model: &UiModel,
        left: f32,
        right: f32,
        height: f32,
    ) -> Result<(), String> {
        self.pill(
            rect(left, 42.0, left + 86.0, 78.0),
            SURFACE,
            "‹ Back",
            PRIMARY,
            Some(Action::Back),
        )?;
        let Some(clip) = model.active_clip() else {
            self.empty_state("Clip unavailable", left, right, 240.0)?;
            return Ok(());
        };
        self.text(
            &clip.title,
            rect(left, 96.0, right - 150.0, 132.0),
            &self.heading.clone(),
            PRIMARY,
        )?;
        self.text(
            &format!(
                "{}  •  {}",
                age(clip.modified),
                format_bytes(clip.size_bytes)
            ),
            rect(left, 136.0, right, 158.0),
            &self.small.clone(),
            SECONDARY,
        )?;
        self.pill(
            rect(right - 130.0, 96.0, right, 134.0),
            SURFACE,
            "Open folder",
            PRIMARY,
            Some(Action::OpenClipsFolder),
        )?;
        let stage = rect(left, 184.0, right, (height - 106.0).max(330.0));
        self.fill(stage, STAGE, 12.0)?;
        self.text(
            "▶",
            rect(
                (left + right) / 2.0 - 16.0,
                (stage.top + stage.bottom) / 2.0 - 18.0,
                right,
                stage.bottom,
            ),
            &self.heading.clone(),
            PRIMARY,
        )?;
        self.hits.push(HitRegion {
            rect: stage,
            action: Action::PlayPause,
        });
        self.text(
            "Native Media Foundation preview",
            rect(left, height - 80.0, right, height - 56.0),
            &self.body.clone(),
            SECONDARY,
        )
    }

    fn page_heading(
        &mut self,
        title: &str,
        subtitle: &str,
        left: f32,
        right: f32,
    ) -> Result<(), String> {
        self.text(
            title,
            rect(left, 48.0, right, 84.0),
            &self.heading.clone(),
            PRIMARY,
        )?;
        self.text(
            subtitle,
            rect(left, 90.0, right, 112.0),
            &self.body.clone(),
            SECONDARY,
        )
    }

    fn clip_grid(
        &mut self,
        model: &UiModel,
        indices: &[usize],
        left: f32,
        right: f32,
        top: f32,
        bottom: f32,
    ) -> Result<(), String> {
        let width = right - left;
        let columns = if width >= 1300.0 {
            6
        } else if width >= 900.0 {
            4
        } else if width >= 650.0 {
            3
        } else if width >= 450.0 {
            2
        } else {
            1
        };
        let gap = 12.0;
        let card_width = (width - gap * (columns - 1) as f32) / columns as f32;
        let card_height = 146.0;
        for (position, index) in indices.iter().enumerate() {
            let row = position / columns;
            let column = position % columns;
            let y = top + row as f32 * (card_height + gap);
            if y + card_height > bottom {
                break;
            }
            let x = left + column as f32 * (card_width + gap);
            let card = rect(x, y, x + card_width, y + card_height);
            self.fill(card, SURFACE, 10.0)?;
            self.fill(
                rect(x + 6.0, y + 6.0, x + card_width - 6.0, y + 94.0),
                STAGE,
                7.0,
            )?;
            let clip = &model.clips[*index];
            let preview = rect(x + 6.0, y + 6.0, x + card_width - 6.0, y + 94.0);
            if !self.draw_thumbnail(&clip.path, preview)? {
                self.text(
                    "▶",
                    rect(x + 14.0, y + 18.0, x + card_width, y + 44.0),
                    &self.section.clone(),
                    SECONDARY,
                )?;
            }
            self.text(
                &clip.title,
                rect(x + 12.0, y + 104.0, x + card_width - 8.0, y + 124.0),
                &self.body.clone(),
                PRIMARY,
            )?;
            self.text(
                &format!(
                    "{}  •  {}",
                    age(clip.modified),
                    format_bytes(clip.size_bytes)
                ),
                rect(x + 12.0, y + 126.0, x + card_width - 8.0, y + 142.0),
                &self.small.clone(),
                SECONDARY,
            )?;
            self.hits.push(HitRegion {
                rect: card,
                action: Action::OpenClip(*index),
            });
        }
        Ok(())
    }

    fn collection_row(
        &mut self,
        name: &str,
        count: usize,
        active: bool,
        area: LogicalRect,
        index: Option<usize>,
    ) -> Result<(), String> {
        if active {
            self.fill(area, SURFACE_HOVER, 9.0)?;
        }
        self.text(
            name,
            rect(
                area.left + 12.0,
                area.top + 10.0,
                area.right - 42.0,
                area.bottom,
            ),
            &self.body.clone(),
            if active { PRIMARY } else { SECONDARY },
        )?;
        self.text(
            &count.to_string(),
            rect(area.right - 32.0, area.top + 10.0, area.right, area.bottom),
            &self.small.clone(),
            SECONDARY,
        )?;
        self.hits.push(HitRegion {
            rect: area,
            action: Action::SelectCollection(index),
        });
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn setting_row(
        &mut self,
        title: &str,
        value: &str,
        description: &str,
        left: f32,
        right: f32,
        top: f32,
        action: Action,
        control: SettingControl,
    ) -> Result<(), String> {
        let area = rect(left, top, right, top + 76.0);
        self.fill(area, SURFACE, 11.0)?;
        let available = right - left;
        let control_width = (available * 0.38).clamp(190.0, 360.0);
        let control_area = rect(
            right - control_width - 16.0,
            top + 17.0,
            right - 16.0,
            top + 59.0,
        );
        let text_right = control_area.left - 18.0;
        self.text(
            title,
            rect(left + 18.0, top + 8.0, text_right, top + 34.0),
            &self.body.clone(),
            PRIMARY,
        )?;
        self.text(
            description,
            rect(left + 18.0, top + 34.0, text_right, top + 66.0),
            &self.small.clone(),
            SECONDARY,
        )?;
        let enabled_toggle = matches!(control, SettingControl::Toggle) && value == "On";
        self.fill(
            control_area,
            if enabled_toggle {
                SUCCESS
            } else {
                SURFACE_HOVER
            },
            9.0,
        )?;
        match control {
            SettingControl::Dropdown => {
                self.text(
                    value,
                    rect(
                        control_area.left + 14.0,
                        control_area.top,
                        control_area.right - 38.0,
                        control_area.bottom,
                    ),
                    &self.body.clone(),
                    PRIMARY,
                )?;
                self.glyph(
                    Glyph::ChevronDown,
                    rect(
                        control_area.right - 29.0,
                        control_area.top + 13.0,
                        control_area.right - 13.0,
                        control_area.bottom - 13.0,
                    ),
                    SECONDARY,
                )?;
            }
            SettingControl::Button | SettingControl::Toggle => self.text(
                value,
                control_area,
                &self.body_center.clone(),
                if enabled_toggle { CANVAS } else { PRIMARY },
            )?,
        }
        self.hits.push(HitRegion {
            rect: control_area,
            action,
        });
        Ok(())
    }

    fn empty_state(
        &mut self,
        message: &str,
        left: f32,
        right: f32,
        top: f32,
    ) -> Result<(), String> {
        self.text(
            message,
            rect(left, top, right, top + 34.0),
            &self.section.clone(),
            SECONDARY,
        )
    }

    fn pill(
        &mut self,
        area: LogicalRect,
        background: u32,
        label: &str,
        foreground: u32,
        action: Option<Action>,
    ) -> Result<(), String> {
        self.fill(area, background, 9.0)?;
        self.text(label, area, &self.body_center.clone(), foreground)?;
        if let Some(action) = action {
            self.hits.push(HitRegion { rect: area, action });
        }
        Ok(())
    }

    fn fill(&self, area: LogicalRect, fill: u32, radius: f32) -> Result<(), String> {
        let target = self.target.as_ref().expect("render target exists");
        let brush = unsafe { target.CreateSolidColorBrush(&color(fill), None) }
            .map_err(|error| error.to_string())?;
        unsafe {
            if radius > 0.0 {
                target.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: area.d2d(),
                        radiusX: radius,
                        radiusY: radius,
                    },
                    &brush,
                );
            } else {
                target.FillRectangle(&area.d2d(), &brush);
            }
        }
        Ok(())
    }

    fn glyph(&self, glyph: Glyph, area: LogicalRect, fill: u32) -> Result<(), String> {
        use windows::Win32::Graphics::Direct2D::ID2D1StrokeStyle;

        let target = self.target.as_ref().expect("render target exists");
        let brush = unsafe { target.CreateSolidColorBrush(&color(fill), None) }
            .map_err(|error| error.to_string())?;
        let width = area.right - area.left;
        let height = area.bottom - area.top;
        let point = |x: f32, y: f32| Vector2 {
            X: area.left + width * x / 24.0,
            Y: area.top + height * y / 24.0,
        };
        let stroke = width.min(height).mul_add(0.085, 0.25).clamp(1.4, 2.2);
        let line = |from_x: f32, from_y: f32, to_x: f32, to_y: f32| unsafe {
            target.DrawLine(
                point(from_x, from_y),
                point(to_x, to_y),
                &brush,
                stroke,
                None::<&ID2D1StrokeStyle>,
            );
        };
        match glyph {
            Glyph::Logo => {
                line(8.0, 3.0, 3.0, 3.0);
                line(3.0, 3.0, 3.0, 8.0);
                line(16.0, 3.0, 21.0, 3.0);
                line(21.0, 3.0, 21.0, 8.0);
                line(3.0, 16.0, 3.0, 21.0);
                line(3.0, 21.0, 8.0, 21.0);
                line(21.0, 16.0, 21.0, 21.0);
                line(21.0, 21.0, 16.0, 21.0);
                line(9.0, 8.0, 9.0, 16.0);
                line(15.0, 8.0, 15.0, 16.0);
            }
            Glyph::Home => {
                line(3.0, 11.0, 12.0, 3.5);
                line(12.0, 3.5, 21.0, 11.0);
                line(5.5, 9.0, 5.5, 20.0);
                line(5.5, 20.0, 18.5, 20.0);
                line(18.5, 20.0, 18.5, 9.0);
                line(10.0, 20.0, 10.0, 14.0);
                line(10.0, 14.0, 14.0, 14.0);
                line(14.0, 14.0, 14.0, 20.0);
            }
            Glyph::Library => {
                line(4.0, 4.0, 20.0, 4.0);
                line(20.0, 4.0, 20.0, 20.0);
                line(20.0, 20.0, 4.0, 20.0);
                line(4.0, 20.0, 4.0, 4.0);
                line(10.0, 8.5, 16.0, 12.0);
                line(16.0, 12.0, 10.0, 15.5);
                line(10.0, 15.5, 10.0, 8.5);
            }
            Glyph::Collections => {
                line(3.0, 7.0, 9.0, 7.0);
                line(9.0, 7.0, 11.0, 4.5);
                line(11.0, 4.5, 20.0, 4.5);
                line(20.0, 4.5, 21.0, 19.5);
                line(21.0, 19.5, 3.0, 19.5);
                line(3.0, 19.5, 3.0, 7.0);
                line(3.0, 9.5, 21.0, 9.5);
            }
            Glyph::Settings => {
                for (y, knob) in [(6.0, 9.0), (12.0, 16.0), (18.0, 7.0)] {
                    line(3.0, y, 21.0, y);
                    unsafe {
                        target.FillEllipse(
                            &D2D1_ELLIPSE {
                                point: point(knob, y),
                                radiusX: stroke * 1.45,
                                radiusY: stroke * 1.45,
                            },
                            &brush,
                        );
                    }
                }
            }
            Glyph::ChevronDown => {
                line(5.0, 9.0, 12.0, 16.0);
                line(12.0, 16.0, 19.0, 9.0);
            }
            Glyph::Close => {
                line(7.0, 7.0, 17.0, 17.0);
                line(17.0, 7.0, 7.0, 17.0);
            }
        }
        Ok(())
    }

    fn text(
        &self,
        value: &str,
        area: LogicalRect,
        format: &IDWriteTextFormat,
        fill: u32,
    ) -> Result<(), String> {
        let target = self.target.as_ref().expect("render target exists");
        let brush = unsafe { target.CreateSolidColorBrush(&color(fill), None) }
            .map_err(|error| error.to_string())?;
        let value = value.encode_utf16().collect::<Vec<_>>();
        unsafe {
            target.DrawText(
                &value,
                format,
                &area.d2d(),
                &brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            )
        };
        Ok(())
    }

    fn draw_thumbnail(&mut self, path: &Path, destination: LogicalRect) -> Result<bool, String> {
        if !self.thumbnails.contains_key(path) && !self.unavailable_thumbnails.contains(path) {
            match self.load_thumbnail(path) {
                Ok(bitmap) => {
                    self.thumbnails.insert(path.to_path_buf(), bitmap);
                }
                Err(_) => {
                    self.unavailable_thumbnails.insert(path.to_path_buf());
                }
            }
        }
        let Some(bitmap) = self.thumbnails.get(path) else {
            return Ok(false);
        };
        let target = self.target.as_ref().expect("render target exists");
        unsafe {
            target.DrawBitmap(
                bitmap,
                Some(&destination.d2d()),
                1.0,
                D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                None,
            );
        }
        Ok(true)
    }

    fn load_thumbnail(&self, path: &Path) -> Result<ID2D1Bitmap, String> {
        let path = path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let shell: IShellItemImageFactory =
            unsafe { SHCreateItemFromParsingName(PCWSTR(path.as_ptr()), None::<&IBindCtx>) }
                .map_err(|error| error.to_string())?;
        let bitmap = unsafe {
            shell.GetImage(
                windows::Win32::Foundation::SIZE { cx: 640, cy: 360 },
                SIIGBF_THUMBNAILONLY | SIIGBF_BIGGERSIZEOK,
            )
        }
        .map_err(|error| error.to_string())?;
        let wic = unsafe {
            self.wic_factory
                .CreateBitmapFromHBITMAP(bitmap, HPALETTE::default(), WICBitmapUseAlpha)
        }
        .map_err(|error| error.to_string());
        let _ = unsafe { DeleteObject(bitmap.into()) };
        let wic = wic?;
        let target = self.target.as_ref().expect("render target exists");
        unsafe { target.CreateBitmapFromWicBitmap(&wic, None) }.map_err(|error| error.to_string())
    }
}

fn text_format(
    factory: &IDWriteFactory,
    size: f32,
    semibold: bool,
    centered: bool,
) -> Result<IDWriteTextFormat, String> {
    let format = unsafe {
        factory.CreateTextFormat(
            w!("Segoe UI Variable"),
            None::<&IDWriteFontCollection>,
            if semibold {
                DWRITE_FONT_WEIGHT_SEMI_BOLD
            } else {
                DWRITE_FONT_WEIGHT_NORMAL
            },
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            size,
            w!("en-US"),
        )
    }
    .map_err(|error| error.to_string())?;
    unsafe {
        format
            .SetTextAlignment(if centered {
                DWRITE_TEXT_ALIGNMENT_CENTER
            } else {
                DWRITE_TEXT_ALIGNMENT_LEADING
            })
            .map_err(|error| error.to_string())?;
        format
            .SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)
            .map_err(|error| error.to_string())?;
        format
            .SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)
            .map_err(|error| error.to_string())?;
    }
    Ok(format)
}

fn rect(left: f32, top: f32, right: f32, bottom: f32) -> LogicalRect {
    LogicalRect {
        left,
        top,
        right,
        bottom,
    }
}

fn sidebar_width(width: f32) -> f32 {
    if width >= WIDE_SIDEBAR_BREAKPOINT {
        214.0
    } else {
        72.0
    }
}

fn color(rgb: u32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: ((rgb >> 16) & 0xff) as f32 / 255.0,
        g: ((rgb >> 8) & 0xff) as f32 / 255.0,
        b: (rgb & 0xff) as f32 / 255.0,
        a: 1.0,
    }
}

fn greeting() -> &'static str {
    let seconds = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        % 86_400;
    match seconds / 3_600 {
        5..=11 => "Good morning",
        12..=17 => "Good afternoon",
        _ => "Good evening",
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GiB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1} MiB", bytes as f64 / 1_048_576.0)
    } else {
        format!("{:.0} KiB", bytes as f64 / 1_024.0)
    }
}

fn age(modified: SystemTime) -> String {
    let elapsed = SystemTime::now()
        .duration_since(modified)
        .unwrap_or(Duration::ZERO);
    if elapsed.as_secs() < 60 {
        "now".into()
    } else if elapsed.as_secs() < 3_600 {
        format!("{}m ago", elapsed.as_secs() / 60)
    } else if elapsed.as_secs() < 86_400 {
        format!("{}h ago", elapsed.as_secs() / 3_600)
    } else {
        format!("{}d ago", elapsed.as_secs() / 86_400)
    }
}

fn on_off(value: bool) -> &'static str {
    if value { "On" } else { "Off" }
}
