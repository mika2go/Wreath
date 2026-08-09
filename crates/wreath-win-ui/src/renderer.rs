use std::collections::{HashMap, HashSet, VecDeque};
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
    DWRITE_TEXT_METRICS, DWRITE_WORD_WRAPPING_NO_WRAP, DWriteCreateFactory, IDWriteFactory,
    IDWriteFontCollection, IDWriteTextFormat,
};
use windows::Win32::Graphics::Gdi::{DeleteObject, HPALETTE};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, IWICImagingFactory, WICBitmapIgnoreAlpha,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, IBindCtx};
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK,
};
use windows::core::{PCWSTR, w};
use windows_numerics::Vector2;

use crate::model::{Action, DeleteTarget, Page, SettingsSection, UiModel, quality_label};

const CANVAS: u32 = 0x0d0d0f;
const STAGE: u32 = 0x101012;
const SURFACE: u32 = 0x17171a;
const SURFACE_HOVER: u32 = 0x202024;
const PRIMARY: u32 = 0xf4f5f9;
const SECONDARY: u32 = 0x777e8e;
const SUCCESS: u32 = 0x76d9a3;
const SELECTION: u32 = 0x2f4a6b;

#[derive(Debug, Clone, Copy)]
enum Glyph {
    Logo,
    Home,
    Library,
    Collections,
    Settings,
    PanelExpand,
    PanelCollapse,
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

pub fn player_bounds(
    width: u32,
    height: u32,
    aspect_ratio: f32,
    sidebar_expanded: bool,
) -> LogicalRect {
    let width = width as f32;
    let rail = sidebar_width(width, sidebar_expanded);
    let padding = if width < 820.0 {
        16.0
    } else if width < 980.0 {
        24.0
    } else {
        40.0
    };
    fit_aspect(
        rect(
            rail + padding,
            184.0,
            width - padding,
            (height as f32 - 142.0).max(330.0),
        ),
        aspect_ratio,
    )
}

pub fn editor_player_bounds(
    width: u32,
    height: u32,
    aspect_ratio: f32,
    sidebar_expanded: bool,
) -> LogicalRect {
    let width = width as f32;
    let rail = sidebar_width(width, sidebar_expanded);
    let padding = if width < 980.0 { 24.0 } else { 32.0 };
    fit_aspect(
        rect(
            rail + padding,
            144.0,
            width - padding,
            (height as f32 - 286.0).max(330.0),
        ),
        aspect_ratio,
    )
}

pub fn editor_timeline_rail(
    width: u32,
    height: u32,
    aspect_ratio: f32,
    sidebar_expanded: bool,
) -> LogicalRect {
    let width_f = width as f32;
    let padding = if width_f < 980.0 { 24.0 } else { 32.0 };
    let left = sidebar_width(width_f, sidebar_expanded) + padding;
    let right = width_f - padding;
    let stage = editor_player_bounds(width, height, aspect_ratio, sidebar_expanded);
    let timeline_top = (stage.bottom + 18.0).min(height as f32 - 220.0);
    rect(
        left + 20.0,
        timeline_top + 60.0,
        right - 20.0,
        timeline_top + 72.0,
    )
}

pub fn editor_timeline_fraction(rail: LogicalRect, x: f32) -> u16 {
    (((x - rail.left) / (rail.right - rail.left).max(1.0)).clamp(0.0, 1.0) * 1000.0).round() as u16
}

fn fit_aspect(area: LogicalRect, aspect_ratio: f32) -> LogicalRect {
    let aspect_ratio = if aspect_ratio.is_finite() && aspect_ratio > 0.1 {
        aspect_ratio
    } else {
        16.0 / 9.0
    };
    let available_width = (area.right - area.left).max(1.0);
    let available_height = (area.bottom - area.top).max(1.0);
    let (width, height) = if available_width / available_height > aspect_ratio {
        (available_height * aspect_ratio, available_height)
    } else {
        (available_width, available_width / aspect_ratio)
    };
    let left = area.left + (available_width - width) / 2.0;
    let top = area.top + (available_height - height) / 2.0;
    rect(left, top, left + width, top + height)
}

#[derive(Clone)]
struct HitRegion {
    rect: LogicalRect,
    action: Action,
}

pub struct Renderer {
    d2d_factory: ID2D1Factory,
    write_factory: IDWriteFactory,
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
    /// Least recently drawn first, so the cache can be bounded.
    thumbnail_order: VecDeque<PathBuf>,
    unavailable_thumbnails: HashSet<PathBuf>,
    consecutive_failures: u32,
}

impl Renderer {
    /// Decoded thumbnails kept resident.
    ///
    /// The cache used to grow for the lifetime of the window: every clip
    /// scrolled past left a bitmap behind and nothing ever removed it, so a
    /// large library turned into hundreds of megabytes that were never drawn
    /// again. A few screens' worth is all that is ever needed at once.
    const MAX_THUMBNAILS: usize = 96;

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
            write_factory,
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
            thumbnail_order: VecDeque::new(),
            unavailable_thumbnails: HashSet::new(),
            consecutive_failures: 0,
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
            self.release_cached_images();
        }
    }

    pub fn retry_unavailable_thumbnails(&mut self) {
        self.unavailable_thumbnails.clear();
    }

    /// Drops the decoded thumbnails while the library is not on screen. They
    /// cost nothing to rebuild and everything to keep for a hidden window.
    pub fn release_cached_images(&mut self) {
        self.thumbnails.clear();
        self.thumbnail_order.clear();
    }

    fn touch_thumbnail(&mut self, path: &Path) {
        if let Some(index) = self
            .thumbnail_order
            .iter()
            .position(|cached| cached.as_path() == path)
        {
            let cached = self.thumbnail_order.remove(index).expect("index found");
            self.thumbnail_order.push_back(cached);
        }
    }

    fn evict_cold_thumbnails(&mut self) {
        while self.thumbnail_order.len() > Self::MAX_THUMBNAILS {
            if let Some(cold) = self.thumbnail_order.pop_front() {
                self.thumbnails.remove(&cold);
            }
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
        let drawn = self.render_frame(model, width, height);
        let ended = unsafe { target.EndDraw(None, None) }.map_err(|error| error.to_string());
        let outcome = drawn.and(ended);
        if outcome.is_err() {
            self.discard_target();
            self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        } else {
            self.consecutive_failures = 0;
        }
        outcome
    }

    pub fn wants_recovery_repaint(&self) -> bool {
        self.consecutive_failures == 1
    }

    pub fn is_failing(&self) -> bool {
        self.consecutive_failures > 1
    }

    fn measure(&self, value: &str, format: &IDWriteTextFormat) -> f32 {
        if value.is_empty() {
            return 0.0;
        }
        let wide = value.encode_utf16().collect::<Vec<_>>();
        let Ok(layout) = (unsafe {
            self.write_factory
                .CreateTextLayout(&wide, format, 4096.0, 4096.0)
        }) else {
            return 0.0;
        };
        let mut metrics = DWRITE_TEXT_METRICS::default();
        if unsafe { layout.GetMetrics(&mut metrics) }.is_err() {
            return 0.0;
        }
        metrics.widthIncludingTrailingWhitespace
    }

    fn discard_target(&mut self) {
        self.target = None;
        self.release_cached_images();
        self.unavailable_thumbnails.clear();
    }

    fn render_frame(&mut self, model: &UiModel, width: u32, height: u32) -> Result<(), String> {
        self.render_shell(model, width as f32, height as f32)?;
        if model.pending_delete.is_some() {
            self.render_delete_modal(model, width as f32, height as f32)?;
        }
        if model.prompt.is_some() {
            self.render_prompt_modal(model, width as f32, height as f32)?;
        }
        if let Some(notice) = &model.notice {
            let rail = sidebar_width(width as f32, model.sidebar_expanded);
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
                notice_area.right - 34.0,
                notice_area.top + 6.0,
                notice_area.right - 6.0,
                notice_area.bottom - 6.0,
            );
            self.glyph(
                Glyph::Close,
                rect(
                    close.left + 6.0,
                    close.top + 6.0,
                    close.right - 6.0,
                    close.bottom - 6.0,
                ),
                SECONDARY,
            )?;
            self.hits.push(HitRegion {
                rect: close,
                action: Action::DismissNotice,
            });
        }
        Ok(())
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
        let wide_sidebar = model.sidebar_expanded && width >= 900.0;
        let rail = sidebar_width(width, model.sidebar_expanded);
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
            let active = model.page == *page
                || (matches!(model.page, Page::Player | Page::Editor)
                    && model.previous_page == *page);
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

        let toggle = rect(14.0, height - 54.0, rail - 14.0, height - 18.0);
        self.fill(toggle, SURFACE_HOVER, 9.0)?;
        let icon_left = if wide_sidebar {
            toggle.right - 32.0
        } else {
            toggle.left + 10.0
        };
        self.glyph(
            if wide_sidebar {
                Glyph::PanelCollapse
            } else {
                Glyph::PanelExpand
            },
            rect(
                icon_left,
                toggle.top + 8.0,
                icon_left + 20.0,
                toggle.bottom - 8.0,
            ),
            SECONDARY,
        )?;
        if wide_sidebar {
            self.text(
                "Collapse sidebar",
                rect(
                    toggle.left + 12.0,
                    toggle.top,
                    toggle.right - 40.0,
                    toggle.bottom,
                ),
                &self.small.clone(),
                SECONDARY,
            )?;
        }
        self.hits.push(HitRegion {
            rect: toggle,
            action: Action::ToggleSidebar,
        });

        let padding = if width < 980.0 { 24.0 } else { 32.0 };
        let left = rail + padding;
        let right = width - padding;
        match model.page {
            Page::Home => self.render_home(model, left, right, height)?,
            Page::Library => self.render_library(model, left, right, height)?,
            Page::Collections => self.render_collections(model, left, right, height)?,
            Page::Settings => self.render_settings(model, left, right, height)?,
            Page::Player => self.render_player(model, left, right, height)?,
            Page::Editor => self.render_editor(model, left, right, height)?,
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
                    &quality_label(model.config.capture.quality),
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
                    372.0,
                    Action::ToggleMicrophone,
                    SettingControl::Toggle,
                )?;
                self.setting_row(
                    "Desktop level",
                    &format!("{}%", model.config.audio.desktop_gain_percent),
                    "Changes only the recorded system sound, not Windows volume.",
                    left,
                    right,
                    284.0,
                    Action::ChooseDesktopGain,
                    SettingControl::Dropdown,
                )?;
                self.setting_row(
                    "Input device",
                    microphone_name,
                    "Choose an active Windows input endpoint.",
                    left,
                    right,
                    460.0,
                    Action::ChooseMicrophone,
                    SettingControl::Dropdown,
                )?;
                self.setting_row(
                    "Microphone level",
                    &format!("{}%", model.config.audio.microphone_gain_percent),
                    "Clean input level without digital noise boost.",
                    left,
                    right,
                    548.0,
                    Action::ChooseMicrophoneGain,
                    SettingControl::Dropdown,
                )?;
            }
            SettingsSection::Controls => {
                let shortcut = if model.hotkey_capture {
                    "Press shortcut…".into()
                } else {
                    wreath_windows::hotkey::localized_hotkey_label(&model.config.hotkey)
                };
                self.setting_row(
                    "Save replay",
                    &shortcut,
                    if model.hotkey_capture {
                        "Press any key or combination now; Escape cancels."
                    } else {
                        "Uses this keyboard's Windows key names; no confirmation needed."
                    },
                    left,
                    right - 54.0,
                    196.0,
                    Action::CaptureHotkey,
                    SettingControl::Button,
                )?;
                let clear = rect(right - 46.0, 213.0, right, 255.0);
                self.fill(clear, SURFACE_HOVER, 9.0)?;
                self.glyph(
                    Glyph::Close,
                    rect(right - 33.0, 226.0, right - 13.0, 246.0),
                    SECONDARY,
                )?;
                self.hits.push(HitRegion {
                    rect: clear,
                    action: Action::ClearHotkey,
                });
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
                    &format_storage_limit(model.config.storage.max_megabytes),
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
            rect(right - 264.0, 96.0, right - 138.0, 134.0),
            SUCCESS,
            "Edit clip",
            CANVAS,
            Some(Action::EditActiveClip),
        )?;
        self.pill(
            rect(right - 130.0, 96.0, right, 134.0),
            SURFACE,
            "Open folder",
            PRIMARY,
            Some(Action::OpenClipsFolder),
        )?;
        let stage = fit_aspect(
            rect(left, 184.0, right, (height - 142.0).max(330.0)),
            model.player_aspect_ratio,
        );
        self.fill(stage, STAGE, 12.0)?;
        self.hits.push(HitRegion {
            rect: stage,
            action: Action::PlayPause,
        });
        let controls_top = height - 112.0;
        self.pill(
            rect(left, controls_top, left + 42.0, controls_top + 38.0),
            SURFACE,
            if model.player_playing { "Ⅱ" } else { "▶" },
            PRIMARY,
            Some(Action::PlayPause),
        )?;
        let rail = rect(
            left + 58.0,
            controls_top + 16.0,
            right - 112.0,
            controls_top + 22.0,
        );
        self.fill(rail, SURFACE_HOVER, 3.0)?;
        let progress = if model.player_duration_seconds > 0.0 {
            (model.player_position_seconds / model.player_duration_seconds).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        if progress > 0.0 {
            let playhead = rail.left + (rail.right - rail.left) * progress;
            self.fill(
                rect(rail.left, rail.top, playhead, rail.bottom),
                SUCCESS,
                3.0,
            )?;
            self.fill(
                rect(
                    playhead - 4.0,
                    rail.top - 4.0,
                    playhead + 4.0,
                    rail.bottom + 4.0,
                ),
                SURFACE_HOVER,
                4.0,
            )?;
            self.fill(
                rect(
                    playhead - 1.5,
                    rail.top - 5.0,
                    playhead + 1.5,
                    rail.bottom + 5.0,
                ),
                PRIMARY,
                1.5,
            )?;
        }
        for percent in 0..100_u8 {
            let segment = (rail.right - rail.left) / 100.0;
            self.hits.push(HitRegion {
                rect: rect(
                    rail.left + segment * f32::from(percent),
                    controls_top,
                    rail.left + segment * f32::from(percent + 1),
                    controls_top + 38.0,
                ),
                action: Action::SeekPercent(percent.saturating_add(1)),
            });
        }
        self.text(
            &format!(
                "{} / {}",
                format_player_time(model.player_position_seconds),
                format_player_time(model.player_duration_seconds)
            ),
            rect(right - 100.0, controls_top, right, controls_top + 38.0),
            &self.small.clone(),
            SECONDARY,
        )
    }

    fn render_editor(
        &mut self,
        model: &UiModel,
        left: f32,
        right: f32,
        height: f32,
    ) -> Result<(), String> {
        self.pill(
            rect(left, 36.0, left + 86.0, 72.0),
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
            &format!("Edit {}", clip.title),
            rect(left, 88.0, right - 170.0, 122.0),
            &self.heading.clone(),
            PRIMARY,
        )?;
        self.text(
            if model.editor_loading {
                "Reading duration and keyframes…"
            } else {
                "Choose the moment to keep"
            },
            rect(left, 122.0, right, 142.0),
            &self.small.clone(),
            SECONDARY,
        )?;

        let stage = fit_aspect(
            rect(left, 144.0, right, (height - 286.0).max(330.0)),
            model.player_aspect_ratio,
        );
        self.fill(stage, STAGE, 11.0)?;
        self.hits.push(HitRegion {
            rect: stage,
            action: Action::PlayPause,
        });

        let timeline_top = (stage.bottom + 18.0).min(height - 220.0);
        let timeline = rect(left, timeline_top, right, timeline_top + 104.0);
        self.fill(timeline, SURFACE, 11.0)?;
        self.text(
            "KEEP THIS MOMENT",
            rect(
                left + 16.0,
                timeline_top + 10.0,
                right - 180.0,
                timeline_top + 28.0,
            ),
            &self.small.clone(),
            SECONDARY,
        )?;
        self.text(
            &format!(
                "{} — {}  ·  {} kept",
                format_editor_time(model.editor_start),
                format_editor_time(model.editor_end),
                format_editor_time(model.editor_selected_duration())
            ),
            rect(
                right - 300.0,
                timeline_top + 10.0,
                right - 16.0,
                timeline_top + 28.0,
            ),
            &self.small.clone(),
            PRIMARY,
        )?;
        self.trim_rail(model, timeline_top + 54.0, left + 20.0, right - 20.0)?;

        let status = if model.editor_working {
            "Cutting on a background worker…"
        } else if let Some(timing) = &model.editor_timing {
            if timing.keyframes.is_empty() {
                "No clean cut points found · exact start will be re-encoded"
            } else {
                "Handles snap to nearby keyframes for a lossless cut"
            }
        } else {
            "Finding clean cut points"
        };
        self.text(
            status,
            rect(
                left,
                timeline.bottom + 14.0,
                right - 170.0,
                timeline.bottom + 50.0,
            ),
            &self.small.clone(),
            SECONDARY,
        )?;
        self.pill(
            rect(
                right - 154.0,
                timeline.bottom + 10.0,
                right,
                timeline.bottom + 50.0,
            ),
            if model.editor_timing.is_some() && !model.editor_working {
                SUCCESS
            } else {
                SURFACE_HOVER
            },
            if model.editor_working {
                "Cutting…"
            } else {
                "Save new clip"
            },
            if model.editor_timing.is_some() && !model.editor_working {
                CANVAS
            } else {
                SECONDARY
            },
            (model.editor_timing.is_some() && !model.editor_working).then_some(Action::SaveCut),
        )
    }

    fn trim_rail(
        &mut self,
        model: &UiModel,
        top: f32,
        left: f32,
        right: f32,
    ) -> Result<(), String> {
        let duration = model
            .editor_timing
            .as_ref()
            .map_or(0.0, |timing| timing.duration.as_secs_f64());
        let rail = rect(left, top + 6.0, right, top + 18.0);
        self.fill(rail, SURFACE_HOVER, 6.0)?;
        if duration > 0.0 {
            if let Some(timing) = &model.editor_timing {
                let stride = timing.keyframes.len().div_ceil(80).max(1);
                for keyframe in timing.keyframes.iter().step_by(stride) {
                    let fraction = (keyframe.as_secs_f64() / duration).clamp(0.0, 1.0) as f32;
                    let x = rail.left + (rail.right - rail.left) * fraction;
                    self.fill(rect(x, top + 22.0, x + 1.0, top + 27.0), SUCCESS, 0.0)?;
                }
            }
            let start_fraction =
                (model.editor_start.as_secs_f64() / duration).clamp(0.0, 1.0) as f32;
            let end_fraction = (model.editor_end.as_secs_f64() / duration).clamp(0.0, 1.0) as f32;
            let start_x = rail.left + (rail.right - rail.left) * start_fraction;
            let end_x = rail.left + (rail.right - rail.left) * end_fraction;
            self.fill(rect(start_x, rail.top, end_x, rail.bottom), SUCCESS, 0.0)?;
            let start_handle = rect(start_x - 6.0, top - 6.0, start_x + 6.0, top + 30.0);
            let end_handle = rect(end_x - 6.0, top - 6.0, end_x + 6.0, top + 30.0);
            let playhead_fraction =
                (model.player_position_seconds / duration).clamp(0.0, 1.0) as f32;
            let playhead_x = rail.left + (rail.right - rail.left) * playhead_fraction;
            if playhead_x >= start_x && playhead_x <= end_x {
                self.fill(
                    rect(playhead_x - 4.0, top - 2.0, playhead_x + 4.0, top + 26.0),
                    SURFACE_HOVER,
                    4.0,
                )?;
                self.fill(
                    rect(playhead_x - 1.5, top - 4.0, playhead_x + 1.5, top + 28.0),
                    PRIMARY,
                    1.5,
                )?;
            }
            self.fill(start_handle, PRIMARY, 4.0)?;
            self.fill(end_handle, PRIMARY, 4.0)?;
            self.hits.push(HitRegion {
                rect: rect(start_x - 13.0, top - 10.0, start_x + 13.0, top + 34.0),
                action: Action::DragEditorStart,
            });
            self.hits.push(HitRegion {
                rect: rect(end_x - 13.0, top - 10.0, end_x + 13.0, top + 34.0),
                action: Action::DragEditorEnd,
            });
        }
        Ok(())
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
            rect(area.left + 12.0, area.top, area.right - 42.0, area.bottom),
            &self.body.clone(),
            if active { PRIMARY } else { SECONDARY },
        )?;
        self.text(
            &count.to_string(),
            rect(area.right - 32.0, area.top, area.right, area.bottom),
            &self.small.clone(),
            SECONDARY,
        )?;
        self.hits.push(HitRegion {
            rect: area,
            action: Action::SelectCollection(index),
        });
        Ok(())
    }

    fn render_delete_modal(
        &mut self,
        model: &UiModel,
        width: f32,
        height: f32,
    ) -> Result<(), String> {
        let Some(target) = &model.pending_delete else {
            return Ok(());
        };
        let overlay = rect(0.0, 0.0, width, height);
        self.fill_alpha(overlay, CANVAS, 0.82, 0.0)?;
        self.hits.push(HitRegion {
            rect: overlay,
            action: Action::CancelDelete,
        });
        let modal_width = 460.0_f32.min(width - 40.0);
        let modal_height = 224.0;
        let left = (width - modal_width) / 2.0;
        let top = (height - modal_height) / 2.0;
        let modal = rect(left, top, left + modal_width, top + modal_height);
        self.fill(modal, SURFACE, 14.0)?;
        let (title, detail, confirmation) = match target {
            DeleteTarget::Clip(index) => {
                let name = model
                    .clips
                    .get(*index)
                    .map_or("this clip", |clip| clip.title.as_str());
                (
                    "Delete clip?",
                    format!("{name} is removed permanently. This cannot be undone."),
                    "Delete clip",
                )
            }
            DeleteTarget::Collection(path) => {
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("this collection");
                (
                    "Delete collection?",
                    format!("{name} is removed; its clips move safely back to Library."),
                    "Delete collection",
                )
            }
        };
        self.text(
            title,
            rect(left + 28.0, top + 24.0, modal.right - 28.0, top + 58.0),
            &self.section.clone(),
            PRIMARY,
        )?;
        self.text(
            &detail,
            rect(left + 28.0, top + 66.0, modal.right - 28.0, top + 108.0),
            &self.body.clone(),
            SECONDARY,
        )?;
        self.pill(
            rect(
                modal.right - 292.0,
                modal.bottom - 62.0,
                modal.right - 178.0,
                modal.bottom - 22.0,
            ),
            SURFACE_HOVER,
            "Cancel",
            PRIMARY,
            Some(Action::CancelDelete),
        )?;
        self.pill(
            rect(
                modal.right - 170.0,
                modal.bottom - 62.0,
                modal.right - 22.0,
                modal.bottom - 22.0,
            ),
            0xe58b8b,
            confirmation,
            CANVAS,
            Some(Action::ConfirmDelete),
        )
    }

    fn render_prompt_modal(
        &mut self,
        model: &UiModel,
        width: f32,
        height: f32,
    ) -> Result<(), String> {
        let Some(prompt) = &model.prompt else {
            return Ok(());
        };
        let overlay = rect(0.0, 0.0, width, height);
        self.fill_alpha(overlay, CANVAS, 0.82, 0.0)?;
        self.hits.push(HitRegion {
            rect: overlay,
            action: Action::CancelPrompt,
        });
        let modal_width = 460.0_f32.min(width - 40.0);
        let modal_height = 232.0;
        let left = (width - modal_width) / 2.0;
        let top = (height - modal_height) / 2.0;
        let modal = rect(left, top, left + modal_width, top + modal_height);
        self.fill(modal, SURFACE, 14.0)?;
        self.hits.push(HitRegion {
            rect: modal,
            action: Action::DismissNotice,
        });
        self.text(
            prompt.title(),
            rect(left + 28.0, top + 24.0, modal.right - 28.0, top + 58.0),
            &self.section.clone(),
            PRIMARY,
        )?;
        self.text(
            prompt.label(),
            rect(left + 28.0, top + 64.0, modal.right - 28.0, top + 84.0),
            &self.small.clone(),
            SECONDARY,
        )?;
        let field = rect(left + 28.0, top + 90.0, modal.right - 28.0, top + 134.0);
        self.fill(field, STAGE, 10.0)?;
        let body = self.body.clone();
        let text_left = field.left + 14.0;
        let (start, end) = prompt.selection();
        if end > start {
            let before: String = prompt.value.chars().take(start).collect();
            let selected: String = prompt.value.chars().skip(start).take(end - start).collect();
            let offset = self.measure(&before, &body);
            let width = self.measure(&selected, &body);
            self.fill(
                rect(
                    text_left + offset,
                    field.top + 9.0,
                    text_left + offset + width,
                    field.bottom - 9.0,
                ),
                SELECTION,
                3.0,
            )?;
        }
        self.text(
            &prompt.value,
            rect(
                text_left,
                field.top + 11.0,
                field.right - 14.0,
                field.bottom - 9.0,
            ),
            &body,
            PRIMARY,
        )?;
        let caret_prefix: String = prompt.value.chars().take(prompt.caret).collect();
        let caret_x = text_left + self.measure(&caret_prefix, &body);
        self.fill(
            rect(
                caret_x,
                field.top + 10.0,
                caret_x + 1.5,
                field.bottom - 10.0,
            ),
            PRIMARY,
            0.0,
        )?;
        self.text(
            "Ctrl+A selects all · Enter confirms · Esc cancels",
            rect(left + 28.0, top + 142.0, modal.right - 28.0, top + 162.0),
            &self.small.clone(),
            SECONDARY,
        )?;
        self.pill(
            rect(
                modal.right - 292.0,
                modal.bottom - 62.0,
                modal.right - 178.0,
                modal.bottom - 22.0,
            ),
            SURFACE_HOVER,
            "Cancel",
            PRIMARY,
            Some(Action::CancelPrompt),
        )?;
        self.pill(
            rect(
                modal.right - 170.0,
                modal.bottom - 62.0,
                modal.right - 22.0,
                modal.bottom - 22.0,
            ),
            PRIMARY,
            prompt.confirm(),
            CANVAS,
            Some(Action::ConfirmPrompt),
        )
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

    fn fill_alpha(
        &self,
        area: LogicalRect,
        fill: u32,
        alpha: f32,
        radius: f32,
    ) -> Result<(), String> {
        let target = self.target.as_ref().expect("render target exists");
        let mut fill = color(fill);
        fill.a = alpha.clamp(0.0, 1.0);
        let brush = unsafe { target.CreateSolidColorBrush(&fill, None) }
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
                line(8.5, 8.5, 15.5, 15.5);
                line(15.5, 8.5, 8.5, 15.5);
            }
            Glyph::PanelExpand => {
                line(4.0, 4.0, 4.0, 20.0);
                line(8.0, 6.0, 18.0, 12.0);
                line(18.0, 12.0, 8.0, 18.0);
            }
            Glyph::PanelCollapse => {
                line(20.0, 4.0, 20.0, 20.0);
                line(16.0, 6.0, 6.0, 12.0);
                line(6.0, 12.0, 16.0, 18.0);
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
                    self.thumbnail_order.push_back(path.to_path_buf());
                    self.evict_cold_thumbnails();
                }
                Err(_) => {
                    self.unavailable_thumbnails.insert(path.to_path_buf());
                }
            }
        } else {
            self.touch_thumbnail(path);
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
                SIIGBF_BIGGERSIZEOK,
            )
        }
        .map_err(|error| error.to_string())?;
        let wic = unsafe {
            self.wic_factory.CreateBitmapFromHBITMAP(
                bitmap,
                HPALETTE::default(),
                WICBitmapIgnoreAlpha,
            )
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

fn sidebar_width(width: f32, expanded: bool) -> f32 {
    if expanded && width >= 900.0 {
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
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    }
}

fn format_storage_limit(megabytes: u32) -> String {
    if megabytes >= 1_024 && megabytes % 1_024 == 0 {
        format!("{} GB", megabytes / 1_024)
    } else {
        format!("{megabytes} MB")
    }
}

fn format_player_time(seconds: f64) -> String {
    let seconds = seconds.max(0.0).round() as u64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

fn format_editor_time(value: Duration) -> String {
    let total_millis = value.as_millis();
    let minutes = total_millis / 60_000;
    let seconds = total_millis % 60_000 / 1_000;
    let millis = total_millis % 1_000;
    format!("{minutes:02}:{seconds:02}.{millis:03}")
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

#[cfg(test)]
mod tests {
    use super::{format_bytes, format_storage_limit};

    #[test]
    fn storage_sizes_use_only_mb_and_gb_labels() {
        assert_eq!(format_bytes(512 * 1_024), "0.5 MB");
        assert_eq!(format_bytes(20 * 1_048_576), "20.0 MB");
        assert_eq!(format_bytes(5 * 1_073_741_824), "5.0 GB");
        assert_eq!(format_storage_limit(512), "512 MB");
        assert_eq!(format_storage_limit(10_240), "10 GB");
    }
}
