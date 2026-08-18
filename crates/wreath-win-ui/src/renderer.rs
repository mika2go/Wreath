use std::collections::{HashMap, HashSet, VecDeque};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use windows::Win32::Foundation::{HWND, PROPERTYKEY};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_F, D2D_SIZE_U, D2D1_COLOR_F, D2D1_FIGURE_BEGIN, D2D1_FIGURE_BEGIN_FILLED,
    D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_ALIASED, D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_SMALL,
    D2D1_BITMAP_BRUSH_PROPERTIES, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_CAP_STYLE_ROUND,
    D2D1_DASH_STYLE_SOLID, D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_ELLIPSE, D2D1_EXTEND_MODE_CLAMP,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_LINE_JOIN_ROUND,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_ROUNDED_RECT, D2D1_STROKE_STYLE_PROPERTIES,
    D2D1_SWEEP_DIRECTION_CLOCKWISE, D2D1CreateFactory, ID2D1Bitmap, ID2D1Factory,
    ID2D1HwndRenderTarget, ID2D1PathGeometry, ID2D1SolidColorBrush, ID2D1StrokeStyle,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_MEASURING_MODE_NATURAL,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
    DWRITE_TEXT_ALIGNMENT_TRAILING, DWRITE_TEXT_METRICS, DWRITE_WORD_WRAPPING_NO_WRAP,
    DWriteCreateFactory, IDWriteFactory, IDWriteFontCollection, IDWriteTextFormat,
};
use windows::Win32::Graphics::Gdi::{DeleteObject, HPALETTE};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, IWICImagingFactory, WICBitmapIgnoreAlpha,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, IBindCtx};
use windows::Win32::UI::Shell::PropertiesSystem::{
    GPS_DEFAULT, IPropertyStore, SHGetPropertyStoreFromParsingName,
};
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK,
};
use windows::Win32::UI::WindowsAndMessaging::{
    SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
};
use windows::core::{GUID, PCWSTR, w};
use windows_numerics::{Matrix3x2, Vector2};

use wreath_core::config::{HoverStyle, Language, Theme};

use crate::model::{
    Action, ClipGroup, ClipTab, DeleteTarget, Page, SettingsMenuKind, TextInput, UiModel,
    hover_strength_label, hover_style_label, language_label, quality_label, theme_label,
};
use crate::text::Strings;

#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub canvas: u32,
    pub rail: u32,
    pub stage: u32,
    pub surface: u32,
    pub surface_raised: u32,
    pub surface_hover: u32,
    pub border: u32,
    pub hairline: u32,
    pub card: u32,
    pub primary: u32,
    pub secondary: u32,
    pub muted: u32,
    pub accent: u32,
    pub accent_hover: u32,
    pub accent_text: u32,
    pub selection: u32,
    pub destructive: u32,
    /// Colour of live indicators: the replay dot and the microphone meter. The
    /// café palette spends its single accent here, the others stay neutral.
    pub live: u32,
}

const DARK_PALETTE: Palette = Palette {
    canvas: 0x0a0a0b,
    rail: 0x0d0d0e,
    stage: 0x0e0e0f,
    surface: 0x111113,
    surface_raised: 0x141416,
    surface_hover: 0x171719,
    border: 0x242426,
    hairline: 0x1b1b1d,
    card: 0x0e0e0f,
    primary: 0xf2f2f2,
    secondary: 0x99999f,
    muted: 0x6f6f75,
    accent: 0xf2f2f2,
    accent_hover: 0xffffff,
    accent_text: 0x0a0a0b,
    selection: 0x3a3a40,
    destructive: 0xd9d9dc,
    live: 0xf2f2f2,
};

const LIGHT_PALETTE: Palette = Palette {
    canvas: 0xecedf0,
    rail: 0xe5e7ea,
    stage: 0xd8dade,
    surface: 0xffffff,
    surface_raised: 0xffffff,
    surface_hover: 0xe3e5ea,
    border: 0xcbced5,
    hairline: 0xdfe1e6,
    card: 0xffffff,
    primary: 0x16171a,
    secondary: 0x54585f,
    muted: 0x6f747d,
    accent: 0x16171a,
    accent_hover: 0x2b2d33,
    accent_text: 0xffffff,
    selection: 0xc3c7d0,
    destructive: 0x3a3c42,
    live: 0x16171a,
};

const CAFE_PALETTE: Palette = Palette {
    canvas: 0x0d0c0b,
    rail: 0x100f0d,
    stage: 0x100f0e,
    surface: 0x151412,
    surface_raised: 0x191715,
    surface_hover: 0x1e1b18,
    border: 0x2a2621,
    hairline: 0x201d1a,
    card: 0x121110,
    primary: 0xf0ece4,
    secondary: 0xa9a29a,
    muted: 0x7c766d,
    accent: 0xe9e2d4,
    accent_hover: 0xf7f2e8,
    accent_text: 0x0d0c0b,
    selection: 0x3a352e,
    destructive: 0xd8d2c8,
    live: 0x7f9b6f,
};

pub const fn palette_for(theme: Theme) -> Palette {
    match theme {
        Theme::Dark => DARK_PALETTE,
        Theme::Light => LIGHT_PALETTE,
        Theme::Cafe => CAFE_PALETTE,
    }
}

const RADIUS: f32 = 6.0;
const RADIUS_SMALL: f32 = 4.0;
const RADIUS_LARGE: f32 = 8.0;
const SIDEBAR_WIDTH: f32 = 165.0;
const SIDEBAR_COLLAPSED_WIDTH: f32 = 60.0;
const CONTENT_PADDING: f32 = 24.0;
const TOOLBAR_HEIGHT: f32 = 96.0;
const TOOLBAR_TOP: f32 = 18.0;
const STATUS_BAR_HEIGHT: f32 = 84.0;
const FILTER_PANEL_WIDTH: f32 = 272.0;
const CLIP_COLUMN_GAP: f32 = 16.0;
const CLIP_ROW_GAP: f32 = 18.0;
const CLIP_META_HEIGHT: f32 = 46.0;
const CLIP_SECTION_HEADER: f32 = 38.0;
const CLIP_LIST_ROW_HEIGHT: f32 = 62.0;
const CLIP_GROUP_GAP: f32 = 22.0;
const CLIP_SCROLL_RESERVE: f32 = 14.0;
const FILTER_ROW_PITCH: f32 = 62.0;
const FOLDER_COLUMN_WIDTH: f32 = 236.0;
const FOLDER_COLUMN_GAP: f32 = 24.0;
const FOLDER_ROW_HEIGHT: f32 = 34.0;
const SETTINGS_PANEL_HEADER: f32 = 36.0;
const SETTINGS_GENERAL_ROWS: usize = 6;
const SETTINGS_CAPTURE_ROWS: usize = 6;
const SETTINGS_AUDIO_ROWS: usize = 6;
const SETTINGS_STORAGE_ROWS: usize = 2;
const METER_WIDTH: f32 = 112.0;
const LEGACY_PAGE_TOP: f32 = 56.0;
const NAVIGATION_TOP: f32 = 86.0;
const NAVIGATION_HEIGHT: f32 = 40.0;
const NAVIGATION_PITCH: f32 = 44.0;
const EDITOR_BOTTOM_RESERVE: f32 = 226.0;
const EDITOR_TIMELINE_HEIGHT: f32 = 118.0;

#[derive(Debug, Clone, Copy)]
enum Glyph {
    Library,
    Collections,
    Settings,
    Folder,
    Search,
    Grid,
    List,
    More,
    Clock,
    Monitor,
    Audio,
    Quality,
    Filter,
    Microphone,
    Star,
    StarFilled,
    External,
    Pencil,
    Play,
    Pause,
    ChevronLeft,
    ChevronRight,
    Fullscreen,
    ChevronDown,
    Close,
}

#[derive(Debug, Clone, Copy)]
enum SettingControl {
    Button,
    Dropdown,
    Toggle,
}

#[derive(Clone, Copy)]
enum TextInputTarget {
    Search,
    Prompt,
}

#[derive(Clone, Copy)]
enum FloatingIconSize {
    Media,
    Navigation,
    Fullscreen,
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

pub fn player_bounds(model: &UiModel, width: u32, height: u32) -> LogicalRect {
    let aspect_ratio = model.player_aspect_ratio;
    let width = width as f32;
    let left = sidebar_width(model.sidebar_collapsed) + CONTENT_PADDING;
    let right = width - CONTENT_PADDING;
    let detail = if right - left >= 960.0 { 300.0 } else { 0.0 };
    fit_aspect(
        rect(
            left,
            176.0,
            right - detail,
            (height as f32 - 178.0).max(390.0),
        ),
        aspect_ratio,
    )
}

pub fn editor_player_bounds(model: &UiModel, width: u32, height: u32) -> LogicalRect {
    let aspect_ratio = model.player_aspect_ratio;
    let width = width as f32;
    let left = sidebar_width(model.sidebar_collapsed) + CONTENT_PADDING;
    let right = width - CONTENT_PADDING;
    let detail = if right - left >= 960.0 { 300.0 } else { 0.0 };
    fit_aspect(
        rect(
            left,
            160.0,
            right - detail,
            (height as f32 - EDITOR_BOTTOM_RESERVE).max(360.0),
        ),
        aspect_ratio,
    )
}

pub fn editor_timeline_rail(model: &UiModel, width: u32, height: u32) -> LogicalRect {
    let left = sidebar_width(model.sidebar_collapsed) + CONTENT_PADDING;
    let right = width as f32 - CONTENT_PADDING;
    let stage = editor_player_bounds(model, width, height);
    let timeline_top = stage.bottom + 92.0;
    rect(
        left + 24.0,
        timeline_top + 34.0,
        right - 24.0,
        timeline_top + 94.0,
    )
}

pub fn editor_timeline_fraction(rail: LogicalRect, x: f32) -> u16 {
    (((x - rail.left) / (rail.right - rail.left).max(1.0)).clamp(0.0, 1.0) * 1000.0).round() as u16
}

pub fn player_timeline_rail(model: &UiModel, width: u32, height: u32) -> LogicalRect {
    let stage = player_bounds(model, width, height);
    rect(
        stage.left + 260.0,
        stage.bottom + 35.0,
        stage.right - 64.0,
        stage.bottom + 41.0,
    )
}

pub fn player_volume_rail(model: &UiModel, width: u32, height: u32) -> LogicalRect {
    let stage = player_bounds(model, width, height);
    let switch_top = (stage.top + stage.bottom) / 2.0 - 22.0;
    let volume_x = stage.right + 78.0;
    let volume_top = switch_top - 76.0;
    rect(
        volume_x - 2.5,
        volume_top + 38.0,
        volume_x + 2.5,
        switch_top + 120.0,
    )
}

pub fn fullscreen_timeline_rail(width: u32, height: u32) -> LogicalRect {
    let width = width as f32;
    let _ = height;
    rect(42.0, 16.0, (width - 42.0).max(43.0), 22.0)
}

pub fn fullscreen_volume_rail(width: u32, height: u32) -> LogicalRect {
    let width = width as f32;
    let height = height as f32;
    rect(204.0, height - 34.0, width.min(334.0), height - 28.0)
}

pub fn settings_audio_gain_rail(
    model: &UiModel,
    width: u32,
    height: u32,
    row: usize,
) -> LogicalRect {
    // the audio panel is the third box of the settings grid
    let left = sidebar_width(model.sidebar_collapsed) + CONTENT_PADDING;
    let right = width as f32 - CONTENT_PADDING;
    let offset = content_top() - LEGACY_PAGE_TOP;
    let virtual_height = content_bottom(height as f32, true) - offset + 22.0;
    let [_, _, audio, _] = settings_panel_rects(left, right, virtual_height);
    let rail = settings_gain_rail_in_panel(audio, SETTINGS_AUDIO_ROWS, row);
    rect(
        rail.left,
        rail.top + offset,
        rail.right,
        rail.bottom + offset,
    )
}

pub fn settings_gain_percent(rail: LogicalRect, x: f32) -> u16 {
    (((x - rail.left) / (rail.right - rail.left).max(1.0)).clamp(0.0, 1.0) * 200.0).round() as u16
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
    round_stroke: ID2D1StrokeStyle,
    palette: Palette,
    strings: &'static Strings,
    hover_style: HoverStyle,
    hover_strength: f32,
    write_factory: IDWriteFactory,
    target: Option<ID2D1HwndRenderTarget>,
    page_title: IDWriteTextFormat,
    section: IDWriteTextFormat,
    brand: IDWriteTextFormat,
    strong: IDWriteTextFormat,
    body: IDWriteTextFormat,
    small: IDWriteTextFormat,
    label: IDWriteTextFormat,
    small_center: IDWriteTextFormat,
    small_right: IDWriteTextFormat,
    body_center: IDWriteTextFormat,
    strong_center: IDWriteTextFormat,
    button: IDWriteTextFormat,
    media_icon: IDWriteTextFormat,
    navigation_icon: IDWriteTextFormat,
    fullscreen_icon: IDWriteTextFormat,
    hits: Vec<HitRegion>,
    wic_factory: IWICImagingFactory,
    thumbnails: HashMap<PathBuf, ID2D1Bitmap>,
    clip_durations: HashMap<PathBuf, Option<u64>>,
    thumbnail_order: VecDeque<PathBuf>,
    unavailable_thumbnails: HashSet<PathBuf>,
    consecutive_failures: u32,
    hovered: Option<Action>,
    hover_progress: f32,
    reduced_motion: bool,
}

impl Renderer {
    const MAX_THUMBNAILS: usize = 96;

    pub fn new() -> Result<Self, String> {
        let d2d_factory =
            unsafe { D2D1CreateFactory::<ID2D1Factory>(D2D1_FACTORY_TYPE_SINGLE_THREADED, None) }
                .map_err(|error| error.to_string())?;
        let write_factory =
            unsafe { DWriteCreateFactory::<IDWriteFactory>(DWRITE_FACTORY_TYPE_SHARED) }
                .map_err(|error| error.to_string())?;
        let page_title = text_format(
            &write_factory,
            w!("Segoe UI Variable Display"),
            25.0,
            true,
            false,
        )?;
        let section = text_format(
            &write_factory,
            w!("Segoe UI Variable Display"),
            16.0,
            true,
            false,
        )?;
        let brand = text_format(
            &write_factory,
            w!("Segoe UI Variable Display"),
            15.0,
            true,
            false,
        )?;
        let strong = text_format(
            &write_factory,
            w!("Segoe UI Variable Text"),
            14.0,
            true,
            false,
        )?;
        let body = text_format(
            &write_factory,
            w!("Segoe UI Variable Text"),
            13.0,
            false,
            false,
        )?;
        let small = text_format(
            &write_factory,
            w!("Segoe UI Variable Text"),
            11.5,
            false,
            false,
        )?;
        let label = text_format(
            &write_factory,
            w!("Segoe UI Variable Text"),
            11.0,
            true,
            false,
        )?;
        let small_center = text_format(
            &write_factory,
            w!("Segoe UI Variable Text"),
            11.0,
            true,
            true,
        )?;
        let small_right = text_format_trailing(&write_factory, w!("Segoe UI Variable Text"), 11.5)?;
        let body_center = text_format(
            &write_factory,
            w!("Segoe UI Variable Text"),
            13.0,
            false,
            true,
        )?;
        let strong_center = text_format(
            &write_factory,
            w!("Segoe UI Variable Text"),
            12.0,
            true,
            true,
        )?;
        let button = text_format(
            &write_factory,
            w!("Segoe UI Variable Text"),
            12.5,
            true,
            true,
        )?;
        let navigation_icon = text_format(
            &write_factory,
            w!("Segoe UI Variable Display"),
            25.0,
            true,
            true,
        )?;
        let media_icon = text_format(
            &write_factory,
            w!("Segoe UI Variable Display"),
            18.0,
            true,
            true,
        )?;
        let fullscreen_icon = text_format(
            &write_factory,
            w!("Segoe UI Variable Display"),
            20.0,
            true,
            true,
        )?;
        let round_stroke = unsafe {
            d2d_factory.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES {
                    startCap: D2D1_CAP_STYLE_ROUND,
                    endCap: D2D1_CAP_STYLE_ROUND,
                    dashCap: D2D1_CAP_STYLE_ROUND,
                    lineJoin: D2D1_LINE_JOIN_ROUND,
                    miterLimit: 10.0,
                    dashStyle: D2D1_DASH_STYLE_SOLID,
                    dashOffset: 0.0,
                },
                None,
            )
        }
        .map_err(|error| error.to_string())?;
        let wic_factory =
            unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }
                .map_err(|error| error.to_string())?;
        let mut animations_enabled = 1_i32;
        let motion_setting_available = unsafe {
            SystemParametersInfoW(
                SPI_GETCLIENTAREAANIMATION,
                0,
                Some((&mut animations_enabled as *mut i32).cast()),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
        }
        .is_ok();
        Ok(Self {
            d2d_factory,
            round_stroke,
            palette: palette_for(Theme::default()),
            strings: crate::text::strings(Language::default()),
            hover_style: HoverStyle::default(),
            hover_strength: 1.0,
            write_factory,
            target: None,
            page_title,
            section,
            brand,
            strong,
            body,
            small,
            label,
            small_center,
            small_right,
            body_center,
            strong_center,
            button,
            media_icon,
            navigation_icon,
            fullscreen_icon,
            hits: Vec::new(),
            wic_factory,
            thumbnails: HashMap::new(),
            clip_durations: HashMap::new(),
            thumbnail_order: VecDeque::new(),
            unavailable_thumbnails: HashSet::new(),
            consecutive_failures: 0,
            hovered: None,
            hover_progress: 0.0,
            reduced_motion: motion_setting_available && animations_enabled == 0,
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

    pub fn update_hover(&mut self, x: f32, y: f32) -> bool {
        let next = self.hit_test(x, y);
        if next == self.hovered {
            return false;
        }
        self.hovered = next;
        self.hover_progress = if self.reduced_motion { 1.0 } else { 0.0 };
        true
    }

    pub fn clear_hover(&mut self) -> bool {
        if self.hovered.take().is_none() {
            return false;
        }
        self.hover_progress = 0.0;
        true
    }

    pub fn advance_motion(&mut self) -> bool {
        if self.hovered.is_none() || self.hover_progress >= 1.0 {
            return false;
        }
        self.hover_progress = (self.hover_progress + 0.25).min(1.0);
        true
    }

    fn apply_appearance(&mut self, model: &UiModel) {
        let config = &model.config;
        self.palette = palette_for(config.appearance.theme);
        self.strings = model.strings();
        self.hover_style = config.appearance.hover;
        self.hover_strength = config.appearance.hover_strength.factor();
    }

    /// Hover fill blend, following the personalised style and strength.
    fn hover_fill(&self, base: u32, target: u32) -> u32 {
        if !self.hover_style.fills() {
            return base;
        }
        mix(base, target, self.hover_amount(1.0))
    }

    /// Hover outline blend, following the same settings.
    fn hover_edge(&self, base: u32, target: u32) -> u32 {
        if !self.hover_style.outlines() {
            return base;
        }
        mix(base, target, self.hover_amount(1.0))
    }

    fn hover_amount(&self, weight: f32) -> f32 {
        hover_blend_amount(self.hover_progress, self.hover_strength, weight)
    }

    fn is_hovered(&self, action: &Action) -> bool {
        self.hovered.as_ref() == Some(action)
    }

    pub fn paint(
        &mut self,
        window: HWND,
        model: &UiModel,
        width: u32,
        height: u32,
        fullscreen: bool,
    ) -> Result<(), String> {
        self.apply_appearance(model);
        self.ensure_target(window, width, height)?;
        self.hits.clear();
        let target = self.target.as_ref().expect("render target exists").clone();
        unsafe {
            target.BeginDraw();
            target.Clear(Some(&color(self.palette.canvas)));
        }
        let drawn = self.render_frame(model, width, height, fullscreen);
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

    pub fn paint_fullscreen_controls(
        &mut self,
        window: HWND,
        model: &UiModel,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        self.apply_appearance(model);
        self.ensure_target(window, width, height)?;
        self.hits.clear();
        let target = self.target.as_ref().expect("render target exists").clone();
        unsafe {
            target.BeginDraw();
            target.Clear(Some(&color(self.palette.canvas)));
        }
        let drawn = self.render_fullscreen_controls(model, width, height);
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

    fn shorten(&self, value: &str, format: &IDWriteTextFormat, max_width: f32) -> String {
        if max_width <= 0.0 || self.measure(value, format) <= max_width {
            return value.to_owned();
        }
        let characters = value.chars().count();
        let mut fits = 0;
        let mut too_long = characters;
        while too_long - fits > 1 {
            let middle = (fits + too_long) / 2;
            let candidate = value
                .chars()
                .take(middle)
                .chain(std::iter::once('…'))
                .collect::<String>();
            if self.measure(&candidate, format) <= max_width {
                fits = middle;
            } else {
                too_long = middle;
            }
        }
        value
            .chars()
            .take(fits)
            .chain(std::iter::once('…'))
            .collect()
    }

    fn discard_target(&mut self) {
        self.target = None;
        self.release_cached_images();
        self.unavailable_thumbnails.clear();
    }

    fn render_frame(
        &mut self,
        model: &UiModel,
        width: u32,
        height: u32,
        fullscreen: bool,
    ) -> Result<(), String> {
        if fullscreen && model.page == Page::Player {
            self.render_fullscreen_header(width as f32)?;
            return Ok(());
        }
        self.render_shell(model, width as f32, height as f32)?;
        if model.settings_menu.is_some() {
            self.render_settings_menu(model, width as f32, height as f32)?;
        }
        if model.context_menu.is_some() {
            self.render_context_menu(model, width as f32, height as f32)?;
        }
        if model.pending_delete.is_some() {
            self.render_delete_modal(model, width as f32, height as f32)?;
        }
        if model.prompt.is_some() {
            self.render_prompt_modal(model, width as f32, height as f32)?;
        }
        if let Some(notice) = &model.notice {
            let notice_bottom = if page_has_chrome(model.page) {
                height as f32 - STATUS_BAR_HEIGHT - 14.0
            } else {
                height as f32 - 18.0
            };
            let notice_area = rect(
                sidebar_width(model.sidebar_collapsed) + CONTENT_PADDING,
                notice_bottom - 44.0,
                width as f32 - CONTENT_PADDING,
                notice_bottom,
            );
            self.fill(notice_area, self.palette.surface_raised, RADIUS)?;
            self.stroke(notice_area, self.palette.border, RADIUS, 1.0)?;
            self.fill(
                rect(
                    notice_area.left + 7.0,
                    notice_area.top + 12.0,
                    notice_area.left + 10.0,
                    notice_area.bottom - 12.0,
                ),
                if model.hotkey_capture {
                    self.palette.primary
                } else {
                    self.palette.secondary
                },
                1.5,
            )?;
            self.text(
                notice,
                rect(
                    notice_area.left + 20.0,
                    notice_area.top,
                    notice_area.right - 48.0,
                    notice_area.bottom,
                ),
                &self.body.clone(),
                self.palette.primary,
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
                self.palette.secondary,
            )?;
            self.hits.push(HitRegion {
                rect: close,
                action: Action::DismissNotice,
            });
        }
        if let Some(drag) = &model.clip_drag_preview {
            let label = self.strings.move_drag(drag.count);
            let chip_width = 138.0;
            let chip_height = 36.0;
            let left = (drag.x + 14.0).clamp(12.0, width as f32 - chip_width - 12.0);
            let top = (drag.y + 14.0).clamp(12.0, height as f32 - chip_height - 12.0);
            let chip = rect(left, top, left + chip_width, top + chip_height);
            self.fill(chip, self.palette.surface_raised, RADIUS)?;
            self.stroke(chip, self.palette.accent, RADIUS, 1.0)?;
            self.text(
                &label,
                chip,
                &self.body_center.clone(),
                self.palette.primary,
            )?;
        }
        Ok(())
    }

    fn render_fullscreen_header(&mut self, width: f32) -> Result<(), String> {
        self.fill(rect(0.0, 0.0, width, 78.0), self.palette.canvas, 0.0)?;
        self.pill(
            rect(18.0, 18.0, 208.0, 60.0),
            self.palette.surface,
            &format!("←  {}", self.strings.back_to_preview),
            self.palette.primary,
            Some(Action::ToggleFullscreen),
        )?;
        self.pill(
            rect(width - 228.0, 18.0, width - 18.0, 60.0),
            self.palette.surface,
            self.strings.original_size_hint,
            self.palette.primary,
            Some(Action::ToggleFullscreen),
        )
    }

    fn render_fullscreen_controls(
        &mut self,
        model: &UiModel,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        let width = width as f32;
        let height = height as f32;
        self.fill(rect(0.0, 0.0, width, height), self.palette.canvas, 0.0)?;
        let timeline = rect(42.0, 16.0, width - 42.0, 22.0);
        self.draw_progress_rail(model, timeline)?;
        self.hits.push(HitRegion {
            rect: rect(timeline.left, 2.0, timeline.right, 36.0),
            action: Action::DragPlayerSeek,
        });
        let row_top = 30.0;
        let row_bottom = 82.0;
        self.floating_icon(
            rect(24.0, row_top, 66.0, row_bottom),
            if model.player_playing { "Ⅱ" } else { "▶" },
            self.palette.primary,
            Some(Action::PlayPause),
            FloatingIconSize::Media,
        )?;
        self.floating_icon(
            rect(72.0, row_top, 112.0, row_bottom),
            "‹",
            self.palette.secondary,
            model.adjacent_clip(-1).map(|_| Action::PreviousClip),
            FloatingIconSize::Navigation,
        )?;
        self.floating_icon(
            rect(116.0, row_top, 156.0, row_bottom),
            "›",
            self.palette.secondary,
            model.adjacent_clip(1).map(|_| Action::NextClip),
            FloatingIconSize::Navigation,
        )?;
        self.text(
            &format!(
                "{} / {}",
                format_player_time(model.player_position_seconds),
                format_player_time(model.player_duration_seconds)
            ),
            rect(174.0, row_top, 302.0, row_bottom),
            &self.body.clone(),
            self.palette.secondary,
        )?;
        self.floating_icon(
            rect(width - 108.0, row_top, width - 66.0, row_bottom),
            if model.player_volume_percent == 0 {
                "🔇"
            } else {
                "🔊"
            },
            self.palette.primary,
            Some(Action::ToggleMute),
            FloatingIconSize::Media,
        )?;
        self.floating_icon(
            rect(width - 60.0, row_top, width - 18.0, row_bottom),
            "⛶",
            self.palette.primary,
            Some(Action::ToggleFullscreen),
            FloatingIconSize::Fullscreen,
        )?;
        let info = rect(18.0, 92.0, width - 18.0, height - 10.0);
        self.fill(info, self.palette.surface, RADIUS)?;
        self.stroke(info, self.palette.border, RADIUS, 1.0)?;
        if let Some(clip) = model.active_clip() {
            let preview = rect(
                info.left + 18.0,
                info.top + 14.0,
                info.left + 108.0,
                info.bottom - 14.0,
            );
            self.fill(preview, self.palette.stage, RADIUS_SMALL)?;
            let _ = self.draw_thumbnail(&clip.path, preview)?;
            self.text(
                &clip.title,
                rect(
                    info.left + 124.0,
                    info.top + 10.0,
                    info.left + 440.0,
                    info.top + 42.0,
                ),
                &self.section.clone(),
                self.palette.primary,
            )?;
            self.text(
                &format!(
                    "{}  ·  {}  ·  {}×{}",
                    age(clip.modified),
                    format_bytes(clip.size_bytes),
                    model.player_video_width,
                    model.player_video_height
                ),
                rect(
                    info.left + 124.0,
                    info.top + 42.0,
                    info.left + 480.0,
                    info.bottom - 8.0,
                ),
                &self.small.clone(),
                self.palette.secondary,
            )?;
            self.pill(
                rect(
                    width / 2.0 - 228.0,
                    info.top + 18.0,
                    width / 2.0 - 72.0,
                    info.bottom - 18.0,
                ),
                self.palette.stage,
                self.strings.edit_clip,
                self.palette.primary,
                Some(Action::EditActiveClip),
            )?;
            self.pill(
                rect(
                    width / 2.0 - 56.0,
                    info.top + 18.0,
                    width / 2.0 + 104.0,
                    info.bottom - 18.0,
                ),
                self.palette.stage,
                self.strings.open_folder,
                self.palette.primary,
                Some(Action::OpenClipsFolder),
            )?;
            if let Some(index) = model.active_clip {
                self.pill(
                    rect(
                        info.right - 172.0,
                        info.top + 18.0,
                        info.right - 16.0,
                        info.bottom - 18.0,
                    ),
                    self.palette.stage,
                    self.strings.delete_clip,
                    self.palette.destructive,
                    Some(Action::DeleteClip(index)),
                )?;
            }
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
        self.render_sidebar(model, height)?;
        let left = sidebar_width(model.sidebar_collapsed) + CONTENT_PADDING;
        let right = width - CONTENT_PADDING;
        let chrome = page_has_chrome(model.page);
        let top = if chrome { content_top() } else { 0.0 };
        let bottom = content_bottom(height, chrome);
        if chrome {
            self.render_recording_toolbar(model, left, right)?;
        }
        match model.page {
            Page::Library => self.render_library(model, left, right, top, bottom)?,
            Page::Collections => self.render_collections(model, left, right, top, bottom)?,
            Page::Settings => {
                let offset = top - LEGACY_PAGE_TOP;
                self.with_offset(offset, |renderer| {
                    renderer.render_settings(model, left, right, bottom - offset + 22.0)
                })?;
            }
            Page::Player => self.render_player(model, left, right, height)?,
            Page::Editor => self.render_editor(model, left, right, height)?,
        }
        if chrome {
            self.render_status_bar(model, width, height)?;
        }
        Ok(())
    }

    fn render_sidebar(&mut self, model: &UiModel, height: f32) -> Result<(), String> {
        let rail = sidebar_width(model.sidebar_collapsed);
        let collapsed = model.sidebar_collapsed;
        self.fill(rect(0.0, 0.0, rail, height), self.palette.rail, 0.0)?;
        self.fill(
            rect(rail - 1.0, 0.0, rail, height),
            self.palette.border,
            0.0,
        )?;

        if collapsed {
            self.draw_wreath_logo(
                rect(rail / 2.0 - 12.0, 26.0, rail / 2.0 + 12.0, 50.0),
                self.palette.primary,
            )?;
            self.rail_button(
                rect(rail / 2.0 - 14.0, 60.0, rail / 2.0 + 14.0, 88.0),
                Glyph::ChevronRight,
                Action::ToggleSidebar,
            )?;
        } else {
            self.draw_wreath_logo(rect(20.0, 26.0, 44.0, 50.0), self.palette.primary)?;
            self.text(
                "wreath",
                rect(52.0, 26.0, rail - 42.0, 50.0),
                &self.brand.clone(),
                self.palette.primary,
            )?;
            self.rail_button(
                rect(rail - 38.0, 24.0, rail - 10.0, 52.0),
                Glyph::ChevronLeft,
                Action::ToggleSidebar,
            )?;
        }

        let navigation = [
            (Some(Page::Library), Glyph::Library, self.strings.clips),
            (
                Some(Page::Collections),
                Glyph::Collections,
                self.strings.collections,
            ),
            (None, Glyph::Folder, self.strings.open_folder),
        ];
        let navigation_top = if collapsed {
            NAVIGATION_TOP + 18.0
        } else {
            NAVIGATION_TOP
        };
        for (offset, (page, glyph, label)) in navigation.iter().enumerate() {
            let active = page.is_some_and(|page| {
                model.page == page
                    || (matches!(model.page, Page::Player | Page::Editor)
                        && model.previous_page == page)
            });
            let action = page.map_or(Action::OpenClipsFolder, Action::Navigate);
            self.sidebar_item(
                rail,
                navigation_top + offset as f32 * NAVIGATION_PITCH,
                *glyph,
                label,
                active,
                collapsed,
                action,
            )?;
        }

        self.sidebar_item(
            rail,
            height - 92.0,
            Glyph::Settings,
            self.strings.settings,
            model.page == Page::Settings,
            collapsed,
            Action::Navigate(Page::Settings),
        )?;
        if !collapsed {
            self.text(
                &format!("wreath v{}", env!("CARGO_PKG_VERSION")),
                rect(22.0, height - 44.0, rail - 12.0, height - 22.0),
                &self.small.clone(),
                self.palette.muted,
            )?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn sidebar_item(
        &mut self,
        rail: f32,
        top: f32,
        glyph: Glyph,
        label: &str,
        active: bool,
        collapsed: bool,
        action: Action,
    ) -> Result<(), String> {
        let area = if collapsed {
            rect(8.0, top, rail - 9.0, top + NAVIGATION_HEIGHT)
        } else {
            rect(10.0, top, rail - 10.0, top + NAVIGATION_HEIGHT)
        };
        if active {
            self.fill(area, self.palette.surface_hover, RADIUS)?;
        } else if self.is_hovered(&action) {
            self.fill(
                area,
                self.hover_fill(self.palette.rail, self.palette.surface_hover),
                RADIUS,
            )?;
        }
        let tone = if active {
            self.palette.primary
        } else {
            self.palette.secondary
        };
        let icon_left = if collapsed {
            (area.left + area.right) / 2.0 - 9.0
        } else {
            area.left + 12.0
        };
        self.glyph(
            glyph,
            rect(
                icon_left,
                area.top + 11.0,
                icon_left + 18.0,
                area.bottom - 11.0,
            ),
            tone,
        )?;
        if !collapsed {
            self.text(
                label,
                rect(area.left + 40.0, area.top, area.right - 8.0, area.bottom),
                &self.body.clone(),
                tone,
            )?;
        }
        self.hits.push(HitRegion { rect: area, action });
        Ok(())
    }

    fn rail_button(
        &mut self,
        area: LogicalRect,
        glyph: Glyph,
        action: Action,
    ) -> Result<(), String> {
        if self.is_hovered(&action) {
            self.fill(
                area,
                self.hover_fill(self.palette.rail, self.palette.surface_hover),
                RADIUS_SMALL,
            )?;
        }
        let center_x = (area.left + area.right) / 2.0;
        let center_y = (area.top + area.bottom) / 2.0;
        self.glyph(
            glyph,
            rect(
                center_x - 8.0,
                center_y - 8.0,
                center_x + 8.0,
                center_y + 8.0,
            ),
            if self.is_hovered(&action) {
                self.palette.primary
            } else {
                self.palette.muted
            },
        )?;
        self.hits.push(HitRegion { rect: area, action });
        Ok(())
    }

    fn render_recording_toolbar(
        &mut self,
        model: &UiModel,
        left: f32,
        right: f32,
    ) -> Result<(), String> {
        let bar = rect(left, TOOLBAR_TOP, right, TOOLBAR_TOP + TOOLBAR_HEIGHT);
        self.fill(bar, self.palette.surface, RADIUS)?;
        self.stroke(bar, self.palette.border, RADIUS, 1.0)?;

        let narrow = bar.right - bar.left < 900.0;
        let action_width = 152.0;
        let gear_width = 40.0;
        let action_block = action_width + 14.0 + gear_width + 20.0 + 28.0;
        let status_width = if narrow { 214.0 } else { 244.0 };
        let display = model.selected_display().map_or_else(
            || self.strings.automatic.to_owned(),
            |display| display.short_label.clone(),
        );
        let quality = self.strings.resolution_line(
            model
                .selected_display()
                .map_or(1_080, |display| display.height),
            model.config.capture.frames_per_second,
        );
        let audio = match (model.config.audio.desktop, model.config.audio.microphone) {
            (true, true) => self.strings.audio_system_and_microphone,
            (true, false) => self.strings.audio_system,
            (false, true) => self.strings.audio_microphone,
            (false, false) => self.strings.audio_none,
        };
        let settings = [
            (
                Glyph::Clock,
                self.strings.clip_length_label,
                self.strings.seconds(model.config.capture.duration_seconds),
                Action::ChooseDuration,
                160.0,
            ),
            (
                Glyph::Monitor,
                self.strings.display_label,
                display,
                Action::ChooseDisplay,
                164.0,
            ),
            (
                Glyph::Quality,
                self.strings.quality_label,
                quality,
                Action::ChooseQuality,
                170.0,
            ),
            (
                Glyph::Audio,
                self.strings.audio_label,
                audio.to_owned(),
                Action::ChooseAudioMode,
                200.0,
            ),
        ];
        let preferred = settings.iter().map(|entry| entry.4).collect::<Vec<_>>();
        let available = (bar.right - bar.left - status_width - action_block).max(0.0);
        let widths = section_widths(available, &preferred, if narrow { 132.0 } else { 150.0 });

        self.replay_indicator(
            rect(bar.left, bar.top, bar.left + status_width, bar.bottom),
            model,
            narrow,
        )?;
        let mut x = bar.left + status_width;
        for (index, width) in widths.iter().enumerate() {
            let (glyph, label, value, action, _) = &settings[index];
            self.toolbar_separator(bar, x)?;
            self.toolbar_setting(
                rect(x, bar.top, x + width, bar.bottom),
                *glyph,
                label,
                value,
                action.clone(),
            )?;
            x += width;
        }

        let gear = rect(
            bar.right - 20.0 - gear_width,
            bar.top + 28.0,
            bar.right - 20.0,
            bar.bottom - 28.0,
        );
        let button = rect(
            gear.left - 14.0 - action_width,
            bar.top + 20.0,
            gear.left - 14.0,
            bar.top + 56.0,
        );
        self.toolbar_separator(bar, (x + 14.0).min(button.left - 20.0))?;
        self.action_button(button, self.strings.save_clip, Action::SaveReplay)?;
        self.text(
            &wreath_windows::hotkey::localized_hotkey_label(&model.config.hotkey),
            rect(
                button.left,
                button.bottom + 2.0,
                button.right,
                bar.bottom - 20.0,
            ),
            &self.body_center.clone(),
            self.palette.muted,
        )?;
        self.icon_button(gear, Glyph::Settings, Action::Navigate(Page::Settings))?;
        Ok(())
    }

    fn toolbar_separator(&self, bar: LogicalRect, x: f32) -> Result<(), String> {
        self.fill(
            rect(x, bar.top + 22.0, x + 1.0, bar.bottom - 22.0),
            self.palette.border,
            0.0,
        )
    }

    fn replay_indicator(
        &mut self,
        area: LogicalRect,
        model: &UiModel,
        narrow: bool,
    ) -> Result<(), String> {
        let live = model.daemon.is_recording();
        self.status_dot(area.left + 20.0, area.top + 38.0, live)?;
        self.text(
            model.daemon.toolbar_headline(self.strings),
            rect(
                area.left + 36.0,
                area.top + 28.0,
                area.right - 12.0,
                area.top + 48.0,
            ),
            &self.label.clone(),
            if live {
                self.palette.primary
            } else {
                self.palette.secondary
            },
        )?;
        let seconds = model.config.capture.duration_seconds;
        self.text(
            &if narrow {
                self.strings.buffered_seconds(seconds)
            } else {
                self.strings.saves_last_seconds(seconds)
            },
            rect(
                area.left + 36.0,
                area.top + 48.0,
                area.right - 12.0,
                area.top + 70.0,
            ),
            &self.small.clone(),
            self.palette.muted,
        )
    }

    fn status_dot(&self, x: f32, y: f32, live: bool) -> Result<(), String> {
        let target = self.target.as_ref().expect("render target exists");
        let brush = unsafe {
            target.CreateSolidColorBrush(
                &color(if live {
                    self.palette.live
                } else {
                    self.palette.muted
                }),
                None,
            )
        }
        .map_err(|error| error.to_string())?;
        unsafe {
            target.FillEllipse(
                &D2D1_ELLIPSE {
                    point: Vector2 { X: x, Y: y },
                    radiusX: 4.0,
                    radiusY: 4.0,
                },
                &brush,
            );
        }
        Ok(())
    }

    fn toolbar_setting(
        &mut self,
        area: LogicalRect,
        glyph: Glyph,
        label: &str,
        value: &str,
        action: Action,
    ) -> Result<(), String> {
        let hovered = self.is_hovered(&action);
        if hovered {
            self.fill(
                rect(
                    area.left + 6.0,
                    area.top + 12.0,
                    area.right - 6.0,
                    area.bottom - 12.0,
                ),
                self.hover_fill(self.palette.surface, self.palette.surface_hover),
                RADIUS,
            )?;
        }
        let compact = area.right - area.left < 152.0;
        let text_left = if compact {
            area.left + 14.0
        } else {
            self.glyph(
                glyph,
                rect(
                    area.left + 18.0,
                    area.top + 38.0,
                    area.left + 34.0,
                    area.top + 54.0,
                ),
                self.palette.secondary,
            )?;
            area.left + 44.0
        };
        self.text(
            label,
            rect(
                text_left,
                area.top + 28.0,
                area.right - 14.0,
                area.top + 47.0,
            ),
            &self.label.clone(),
            self.palette.muted,
        )?;
        let chevron = rect(
            area.right - 24.0,
            area.top + 52.0,
            area.right - 12.0,
            area.top + 62.0,
        );
        let value_area = rect(
            text_left,
            area.top + 47.0,
            chevron.left - 6.0,
            area.top + 70.0,
        );
        self.text(
            &self.shorten(value, &self.strong, value_area.right - value_area.left),
            value_area,
            &self.strong.clone(),
            self.palette.primary,
        )?;
        self.glyph(
            Glyph::ChevronDown,
            chevron,
            if hovered {
                self.palette.secondary
            } else {
                self.palette.muted
            },
        )?;
        self.hits.push(HitRegion { rect: area, action });
        Ok(())
    }

    fn action_button(
        &mut self,
        area: LogicalRect,
        label: &str,
        action: Action,
    ) -> Result<(), String> {
        let background = if self.is_hovered(&action) {
            mix(
                self.palette.accent,
                self.palette.accent_hover,
                self.hover_amount(1.0),
            )
        } else {
            self.palette.accent
        };
        self.fill(area, background, RADIUS_SMALL)?;
        self.text(label, area, &self.button.clone(), self.palette.accent_text)?;
        self.hits.push(HitRegion { rect: area, action });
        Ok(())
    }

    fn render_status_bar(
        &mut self,
        model: &UiModel,
        width: f32,
        height: f32,
    ) -> Result<(), String> {
        let bar = rect(
            sidebar_width(model.sidebar_collapsed),
            height - STATUS_BAR_HEIGHT,
            width,
            height,
        );
        self.fill(bar, self.palette.canvas, 0.0)?;
        self.fill(
            rect(bar.left, bar.top, bar.right, bar.top + 1.0),
            self.palette.border,
            0.0,
        )?;

        let used = model.total_size_bytes();
        let limit = u64::from(model.config.storage.max_megabytes).saturating_mul(1_048_576);
        let fraction = if limit == 0 {
            0.0
        } else {
            (used as f32 / limit as f32).clamp(0.0, 1.0)
        };
        let storage = format!(
            "{} / {} ({}%)",
            format_bytes(used),
            format_storage_limit(model.config.storage.max_megabytes),
            (fraction * 100.0).round() as u32
        );
        let preferred = [232.0_f32, 268.0, 196.0, 214.0];
        let inner_left = bar.left + CONTENT_PADDING;
        let widths = section_widths(
            (bar.right - CONTENT_PADDING - inner_left).max(0.0),
            &preferred,
            176.0,
        );

        let mut x = inner_left;
        for (index, width) in widths.iter().enumerate() {
            if index > 0 {
                self.fill(
                    rect(x - 12.0, bar.top + 22.0, x - 11.0, bar.bottom - 22.0),
                    self.palette.border,
                    0.0,
                )?;
            }
            let area = rect(x, bar.top, x + width, bar.bottom);
            match index {
                0 => {
                    let live = model.daemon.is_recording();
                    self.status_dot(area.left + 4.0, area.top + 33.0, live)?;
                    self.status_entry(
                        area,
                        16.0,
                        model.daemon.status_headline(self.strings),
                        &self.strings.buffered_seconds(model.daemon.buffered_seconds),
                    )?;
                }
                1 => {
                    self.status_entry(area, 0.0, self.strings.storage_caps, &storage)?;
                    let track = rect(
                        area.left,
                        area.bottom - 24.0,
                        area.right - 24.0,
                        area.bottom - 21.0,
                    );
                    self.fill(track, self.palette.surface_hover, 1.5)?;
                    self.fill(
                        rect(
                            track.left,
                            track.top,
                            track.left + (track.right - track.left) * fraction,
                            track.bottom,
                        ),
                        self.palette.secondary,
                        1.5,
                    )?;
                }
                2 => {
                    self.status_entry(
                        area,
                        0.0,
                        self.strings.hotkey_caps,
                        &wreath_windows::hotkey::localized_hotkey_label(&model.config.hotkey),
                    )?;
                }
                _ => {
                    let action = Action::ToggleMicrophoneTest;
                    let hovered = self.is_hovered(&action);
                    self.status_entry(area, 0.0, self.strings.microphone_caps, "")?;
                    self.text(
                        &model.microphone_readout(),
                        rect(
                            area.left + 80.0,
                            area.top + 22.0,
                            area.right - 12.0,
                            area.top + 42.0,
                        ),
                        &self.small_right.clone(),
                        if model.microphone_test {
                            self.palette.primary
                        } else if hovered {
                            self.palette.secondary
                        } else {
                            self.palette.muted
                        },
                    )?;
                    self.glyph(
                        Glyph::Microphone,
                        rect(
                            area.left,
                            area.top + 44.0,
                            area.left + 16.0,
                            area.top + 60.0,
                        ),
                        if model.microphone_test {
                            self.palette.primary
                        } else if model.config.audio.microphone {
                            self.palette.secondary
                        } else {
                            self.palette.muted
                        },
                    )?;
                    self.level_meter(
                        rect(
                            area.left + 26.0,
                            area.top + 44.0,
                            (area.left + 26.0 + METER_WIDTH).min(area.right - 12.0),
                            area.top + 60.0,
                        ),
                        model.config.audio.microphone || model.microphone_test,
                        model.microphone_level,
                        model.microphone_peak_hold,
                    )?;
                    self.hits.push(HitRegion {
                        rect: rect(
                            area.left,
                            area.top + 14.0,
                            area.right - 12.0,
                            area.bottom - 14.0,
                        ),
                        action,
                    });
                }
            }
            x += width;
        }
        Ok(())
    }

    fn status_entry(
        &self,
        area: LogicalRect,
        indent: f32,
        label: &str,
        value: &str,
    ) -> Result<(), String> {
        self.text(
            label,
            rect(
                area.left + indent,
                area.top + 22.0,
                area.right - 12.0,
                area.top + 42.0,
            ),
            &self.label.clone(),
            self.palette.muted,
        )?;
        if value.is_empty() {
            return Ok(());
        }
        self.text(
            value,
            rect(
                area.left + indent,
                area.top + 42.0,
                area.right - 12.0,
                area.top + 64.0,
            ),
            &self.body.clone(),
            self.palette.primary,
        )
    }

    fn level_meter(
        &self,
        area: LogicalRect,
        enabled: bool,
        level: u8,
        hold: u8,
    ) -> Result<(), String> {
        const BARS: usize = 16;
        let pitch = (area.right - area.left) / BARS as f32;
        let bars = |value: u8| (f32::from(value) / 100.0 * BARS as f32).round() as usize;
        let lit = if enabled { bars(level) } else { 0 };
        let marker = if enabled { bars(hold) } else { 0 };
        for index in 0..BARS {
            let x = area.left + index as f32 * pitch;
            let scale = 0.45 + 0.55 * (index as f32 / (BARS - 1) as f32);
            let height = (area.bottom - area.top) * scale;
            let bar = rect(
                x,
                area.bottom - height,
                x + (pitch - 2.0).max(1.5),
                area.bottom,
            );
            self.fill(
                bar,
                if index < lit {
                    self.palette.live
                } else if marker > 0 && index + 1 == marker {
                    self.palette.secondary
                } else if enabled {
                    self.palette.border
                } else {
                    mix(self.palette.canvas, self.palette.border, 0.6)
                },
                1.0,
            )?;
        }
        Ok(())
    }

    fn render_library(
        &mut self,
        model: &UiModel,
        left: f32,
        right: f32,
        top: f32,
        bottom: f32,
    ) -> Result<(), String> {
        let today = crate::clock::now();
        self.text(
            self.strings.clips,
            rect(left, top, left + 260.0, top + 34.0),
            &self.page_title.clone(),
            self.palette.primary,
        )?;

        let mut tab_left = left;
        for tab in [ClipTab::All, ClipTab::Favorites] {
            let active = model.clip_tab == tab;
            let format = if active {
                self.strong.clone()
            } else {
                self.body.clone()
            };
            let width = self.measure(tab.label(self.strings), &format);
            let area = rect(tab_left, top + 44.0, tab_left + width, top + 70.0);
            self.text(
                tab.label(self.strings),
                area,
                &format,
                if active {
                    self.palette.primary
                } else {
                    self.palette.secondary
                },
            )?;
            if active {
                self.fill(
                    rect(area.left, area.bottom, area.right, area.bottom + 1.5),
                    self.palette.primary,
                    0.0,
                )?;
            }
            self.hits.push(HitRegion {
                rect: rect(
                    area.left - 8.0,
                    area.top,
                    area.right + 8.0,
                    area.bottom + 4.0,
                ),
                action: Action::SetClipTab(tab),
            });
            tab_left = area.right + 26.0;
        }

        let tools_top = top + 4.0;
        let tools_bottom = tools_top + 34.0;
        let list = rect(right - 34.0, tools_top, right, tools_bottom);
        let grid = rect(list.left - 42.0, tools_top, list.left - 8.0, tools_bottom);
        let filter = rect(grid.left - 42.0, tools_top, grid.left - 8.0, tools_bottom);
        let search = rect(
            (filter.left - 250.0).max(tab_left + 16.0),
            tools_top,
            filter.left - 10.0,
            tools_bottom,
        );
        if search.right - search.left >= 120.0 {
            self.search_field(model, search, self.strings.search_clips)?;
        }
        self.view_button(
            filter,
            Glyph::Filter,
            model.filter_panel_open || model.filters_are_active(),
            Action::ToggleFilterPanel,
        )?;
        self.view_button(
            grid,
            Glyph::Grid,
            model.library_grid,
            Action::SetLibraryGrid(true),
        )?;
        self.view_button(
            list,
            Glyph::List,
            !model.library_grid,
            Action::SetLibraryGrid(false),
        )?;

        self.fill(
            rect(left, top + 88.0, right, top + 89.0),
            self.palette.border,
            0.0,
        )?;

        let body_top = top + 100.0;
        let selecting = model.selection_mode && !model.selected_clips.is_empty();
        let area = rect(
            left,
            body_top,
            right,
            if selecting { bottom - 60.0 } else { bottom },
        );

        let indices = model.visible_clip_indices_at(usize::MAX, today);
        if indices.is_empty() {
            self.empty_state(
                if !model.search.value.is_empty() {
                    self.strings.empty_no_match
                } else if model.clip_tab == ClipTab::Favorites {
                    self.strings.empty_no_favorites
                } else if model.filters_are_active() {
                    self.strings.empty_no_filter_match
                } else {
                    self.strings.empty_no_clips
                },
                area.left,
                area.right,
                area.top + 8.0,
            )?;
        } else {
            let groups = model.clip_day_groups(&indices, today);
            let counts = groups
                .iter()
                .map(|group| group.indices.len())
                .collect::<Vec<_>>();
            let layout = library_layout(
                &counts,
                area.right - area.left - CLIP_SCROLL_RESERVE,
                model.library_grid,
            );
            let viewport_height = (area.bottom - area.top).max(0.0);
            let overflow = (layout.height - viewport_height).max(0.0);
            let scroll = model.library_scroll.clamp(0.0, overflow);

            self.push_clip(area)?;
            let painted = self.render_clip_sections(model, &groups, &layout, area, scroll, today);
            self.pop_clip();
            painted?;

            if overflow > 0.0 {
                let track = rect(area.right - 4.0, area.top, area.right - 1.0, area.bottom);
                let visible = (viewport_height / layout.height).clamp(0.1, 1.0);
                let thumb_height = viewport_height * visible;
                let thumb_top = area.top + (viewport_height - thumb_height) * (scroll / overflow);
                self.fill(
                    rect(track.left, thumb_top, track.right, thumb_top + thumb_height),
                    self.palette.border,
                    1.5,
                )?;
            }
        }

        if selecting {
            self.selection_toolbar(model, area.left, area.right, bottom - 44.0)?;
        }
        if model.filter_panel_open {
            self.render_filter_panel(model, filter, bottom + STATUS_BAR_HEIGHT + 2.0)?;
        }
        if model.collection_picker_open {
            self.render_collection_picker(model, right, top + 44.0)?;
        }
        Ok(())
    }

    fn render_filter_panel(
        &mut self,
        model: &UiModel,
        anchor: LogicalRect,
        bottom: f32,
    ) -> Result<(), String> {
        let entries: [(&str, String, Action); 5] = [
            (
                self.strings.filter_time,
                model.filter_time.label(self.strings).to_owned(),
                Action::ChooseTimeFilter,
            ),
            (
                self.strings.filter_game,
                model.filter_collection_label().to_owned(),
                Action::ChooseCollectionFilter,
            ),
            (
                self.strings.filter_type,
                model.filter_type.label(self.strings).to_owned(),
                Action::ChooseTypeFilter,
            ),
            (
                self.strings.filter_size,
                model.filter_size.label(self.strings).to_owned(),
                Action::ChooseSizeFilter,
            ),
            (
                self.strings.filter_sort,
                model.sort_label().to_owned(),
                Action::ChooseClipSort,
            ),
        ];
        let width = FILTER_PANEL_WIDTH;
        let panel_height = (44.0 + entries.len() as f32 * FILTER_ROW_PITCH + 40.0)
            .min((bottom - anchor.bottom - 16.0).max(200.0));
        let panel = rect(
            anchor.right - width,
            anchor.bottom + 8.0,
            anchor.right,
            anchor.bottom + 8.0 + panel_height,
        );
        // the panel floats over the clip grid, so it has to swallow every click
        // inside it before the cards register their own
        self.hits.push(HitRegion {
            rect: panel,
            action: Action::Ignore,
        });
        self.fill(panel, self.palette.surface_raised, RADIUS_LARGE)?;
        self.stroke(panel, self.palette.border, RADIUS_LARGE, 1.0)?;
        self.text(
            self.strings.filter_caps,
            rect(
                panel.left + 18.0,
                panel.top + 12.0,
                panel.right - 60.0,
                panel.top + 34.0,
            ),
            &self.label.clone(),
            self.palette.muted,
        )?;
        let active = model.filters_are_active();
        let reset = rect(
            panel.right - 130.0,
            panel.top + 10.0,
            panel.right - 16.0,
            panel.top + 34.0,
        );
        self.text(
            self.strings.reset,
            reset,
            &self.small_right.clone(),
            if !active {
                self.palette.muted
            } else if self.is_hovered(&Action::ResetFilters) {
                self.palette.primary
            } else {
                self.palette.secondary
            },
        )?;
        if active {
            self.hits.push(HitRegion {
                rect: reset,
                action: Action::ResetFilters,
            });
        }

        for (index, (label, value, action)) in entries.into_iter().enumerate() {
            let top = panel.top + 44.0 + index as f32 * FILTER_ROW_PITCH;
            if top + FILTER_ROW_PITCH > panel.bottom {
                break;
            }
            self.text(
                label,
                rect(panel.left + 18.0, top, panel.right - 18.0, top + 18.0),
                &self.small.clone(),
                self.palette.secondary,
            )?;
            self.dropdown(
                rect(
                    panel.left + 18.0,
                    top + 20.0,
                    panel.right - 18.0,
                    top + 54.0,
                ),
                &value,
                action,
            )?;
        }
        Ok(())
    }

    fn render_clip_sections(
        &mut self,
        model: &UiModel,
        groups: &[ClipGroup],
        layout: &LibraryLayout,
        area: LogicalRect,
        scroll: f32,
        today: crate::clock::Civil,
    ) -> Result<(), String> {
        for (section, group) in groups.iter().enumerate() {
            let header_top = area.top + layout.sections[section] - scroll;
            let rows_top = header_top + CLIP_SECTION_HEADER;
            if rows_top > area.bottom {
                break;
            }
            if header_top + CLIP_SECTION_HEADER > area.top {
                self.text(
                    &group.label,
                    rect(area.left, header_top, area.left + 300.0, header_top + 26.0),
                    &self.section.clone(),
                    self.palette.primary,
                )?;
                let count = group.indices.len();
                self.text(
                    &self.strings.clip_count(count),
                    rect(
                        area.right - 160.0 - CLIP_SCROLL_RESERVE,
                        header_top + 3.0,
                        area.right - CLIP_SCROLL_RESERVE,
                        header_top + 24.0,
                    ),
                    &self.small_right.clone(),
                    self.palette.muted,
                )?;
            }
            for (position, index) in group.indices.iter().copied().enumerate() {
                let row = position / layout.columns;
                let column = position % layout.columns;
                let card_top = rows_top + row as f32 * layout.row_pitch;
                if card_top > area.bottom {
                    break;
                }
                if card_top + layout.card_height < area.top {
                    continue;
                }
                let card_left = area.left + column as f32 * (layout.card_width + CLIP_COLUMN_GAP);
                let card = rect(
                    card_left,
                    card_top,
                    card_left + layout.card_width,
                    card_top + layout.card_height,
                );
                if model.library_grid {
                    self.clip_card(model, index, card, area, today)?;
                } else {
                    self.clip_row(model, index, card, area, today)?;
                }
            }
        }
        Ok(())
    }

    fn clip_card(
        &mut self,
        model: &UiModel,
        index: usize,
        card: LogicalRect,
        viewport: LogicalRect,
        today: crate::clock::Civil,
    ) -> Result<(), String> {
        let Some(clip) = model.clips.get(index) else {
            return Ok(());
        };
        let open = if model.selection_mode {
            Action::ToggleClipSelection(index)
        } else {
            Action::OpenClip(index)
        };
        let favorite = Action::ToggleFavorite(index);
        let external = Action::OpenClipExternally(index);
        let menu = Action::OpenClipMenu(index);
        let selected = model.clip_is_selected(index);
        let starred = model.is_favorite(index);
        let hovered = [&open, &favorite, &external, &menu]
            .into_iter()
            .any(|action| self.is_hovered(action));

        // the pointer target for the card is registered first so the overlay
        // buttons drawn on top of it keep their own targets
        self.push_clipped_hit(card, viewport, open);

        self.fill(
            card,
            if selected {
                self.palette.surface_hover
            } else if hovered {
                self.hover_fill(self.palette.card, self.palette.surface_hover)
            } else {
                self.palette.card
            },
            RADIUS,
        )?;
        self.stroke(
            card,
            if selected {
                self.palette.secondary
            } else if hovered {
                self.hover_edge(self.palette.hairline, self.palette.secondary)
            } else {
                self.palette.hairline
            },
            RADIUS,
            1.0,
        )?;

        let preview = rect(
            card.left + 1.0,
            card.top + 1.0,
            card.right - 1.0,
            card.bottom - CLIP_META_HEIGHT,
        );
        self.fill(preview, self.palette.stage, RADIUS_SMALL)?;
        if !self.draw_thumbnail(&clip.path, preview)? {
            self.glyph(
                Glyph::Play,
                rect(
                    (preview.left + preview.right) / 2.0 - 10.0,
                    (preview.top + preview.bottom) / 2.0 - 10.0,
                    (preview.left + preview.right) / 2.0 + 10.0,
                    (preview.top + preview.bottom) / 2.0 + 10.0,
                ),
                self.palette.muted,
            )?;
        }

        if let Some(duration) = self.clip_duration(&clip.path) {
            let label = format_clip_badge_duration(duration);
            let badge_width = self.measure(&label, &self.small_center) + 14.0;
            let badge = rect(
                preview.left + 7.0,
                preview.bottom - 24.0,
                preview.left + 7.0 + badge_width,
                preview.bottom - 7.0,
            );
            self.fill_alpha(badge, 0x000000, 0.7, RADIUS_SMALL)?;
            self.text(
                &label,
                badge,
                &self.small_center.clone(),
                self.palette.primary,
            )?;
        }

        if model.selection_mode {
            let check = rect(
                preview.right - 30.0,
                preview.top + 8.0,
                preview.right - 8.0,
                preview.top + 30.0,
            );
            self.fill(
                check,
                if selected {
                    self.palette.accent
                } else {
                    self.palette.surface_raised
                },
                11.0,
            )?;
            self.stroke(
                check,
                if selected {
                    self.palette.accent
                } else {
                    self.palette.border
                },
                11.0,
                1.0,
            )?;
            if selected {
                self.text(
                    "✓",
                    check,
                    &self.strong_center.clone(),
                    self.palette.accent_text,
                )?;
            }
        } else if hovered {
            self.fill_alpha(
                preview,
                self.palette.canvas,
                0.4 * self.hover_amount(1.0),
                RADIUS_SMALL,
            )?;
            let play = rect(
                (preview.left + preview.right) / 2.0 - 18.0,
                (preview.top + preview.bottom) / 2.0 - 18.0,
                (preview.left + preview.right) / 2.0 + 18.0,
                (preview.top + preview.bottom) / 2.0 + 18.0,
            );
            self.fill(play, self.palette.accent, RADIUS_SMALL)?;
            self.glyph(
                Glyph::Play,
                rect(
                    play.left + 10.0,
                    play.top + 9.0,
                    play.right - 8.0,
                    play.bottom - 9.0,
                ),
                self.palette.accent_text,
            )?;
            let star = rect(
                preview.right - 34.0,
                preview.top + 7.0,
                preview.right - 8.0,
                preview.top + 33.0,
            );
            self.overlay_button(
                star,
                if starred {
                    Glyph::StarFilled
                } else {
                    Glyph::Star
                },
                favorite,
                viewport,
            )?;
            self.overlay_button(
                rect(star.left - 32.0, star.top, star.left - 6.0, star.bottom),
                Glyph::External,
                external,
                viewport,
            )?;
        } else if starred {
            let star = rect(
                preview.right - 30.0,
                preview.top + 7.0,
                preview.right - 8.0,
                preview.top + 29.0,
            );
            self.fill_alpha(star, 0x000000, 0.5, RADIUS_SMALL)?;
            self.glyph(
                Glyph::StarFilled,
                rect(
                    star.left + 4.0,
                    star.top + 4.0,
                    star.right - 4.0,
                    star.bottom - 4.0,
                ),
                self.palette.primary,
            )?;
        }

        let text_left = card.left + 12.0;
        let more = rect(
            card.right - 34.0,
            card.bottom - CLIP_META_HEIGHT + 10.0,
            card.right - 8.0,
            card.bottom - 10.0,
        );
        self.text(
            &self.shorten(&clip.title, &self.strong, more.left - text_left - 8.0),
            rect(
                text_left,
                card.bottom - CLIP_META_HEIGHT + 6.0,
                more.left - 8.0,
                card.bottom - CLIP_META_HEIGHT + 26.0,
            ),
            &self.strong.clone(),
            self.palette.primary,
        )?;
        self.text(
            &format!(
                "{}  ·  {}",
                crate::clock::stamp_label(crate::clock::local(clip.modified), today, self.strings),
                format_bytes(clip.size_bytes)
            ),
            rect(
                text_left,
                card.bottom - CLIP_META_HEIGHT + 25.0,
                more.left - 8.0,
                card.bottom - 8.0,
            ),
            &self.small.clone(),
            self.palette.muted,
        )?;
        self.glyph(
            Glyph::More,
            rect(
                more.left + 4.0,
                more.top + 8.0,
                more.right - 4.0,
                more.bottom - 8.0,
            ),
            if self.is_hovered(&menu) {
                self.palette.primary
            } else {
                self.palette.muted
            },
        )?;
        if !model.selection_mode {
            self.push_clipped_hit(more, viewport, menu);
        }
        Ok(())
    }

    fn clip_row(
        &mut self,
        model: &UiModel,
        index: usize,
        row: LogicalRect,
        viewport: LogicalRect,
        today: crate::clock::Civil,
    ) -> Result<(), String> {
        let Some(clip) = model.clips.get(index) else {
            return Ok(());
        };
        let open = if model.selection_mode {
            Action::ToggleClipSelection(index)
        } else {
            Action::OpenClip(index)
        };
        let favorite = Action::ToggleFavorite(index);
        let menu = Action::OpenClipMenu(index);
        let selected = model.clip_is_selected(index);
        if selected {
            self.fill(row, self.palette.surface_hover, RADIUS_SMALL)?;
        } else if self.is_hovered(&open) {
            self.fill(
                row,
                self.hover_fill(self.palette.canvas, self.palette.surface),
                RADIUS_SMALL,
            )?;
        }
        self.fill(
            rect(row.left, row.bottom - 1.0, row.right, row.bottom),
            self.palette.border,
            0.0,
        )?;
        let preview = rect(
            row.left + 8.0,
            row.top + 6.0,
            row.left + 96.0,
            row.bottom - 6.0,
        );
        self.fill(preview, self.palette.stage, RADIUS_SMALL)?;
        let _ = self.draw_thumbnail(&clip.path, preview)?;
        if let Some(duration) = self.clip_duration(&clip.path) {
            let label = format_clip_badge_duration(duration);
            let badge = rect(
                preview.right - 14.0 - label.chars().count() as f32 * 6.5,
                preview.bottom - 20.0,
                preview.right - 4.0,
                preview.bottom - 4.0,
            );
            self.fill_alpha(badge, 0x000000, 0.72, RADIUS_SMALL)?;
            self.text(
                &label,
                badge,
                &self.strong_center.clone(),
                self.palette.primary,
            )?;
        }
        self.text(
            &clip.title,
            rect(preview.right + 16.0, row.top, row.right - 300.0, row.bottom),
            &self.strong.clone(),
            self.palette.primary,
        )?;
        self.text(
            &crate::clock::stamp_label(crate::clock::local(clip.modified), today, self.strings),
            rect(row.right - 296.0, row.top, row.right - 150.0, row.bottom),
            &self.small.clone(),
            self.palette.secondary,
        )?;
        self.text(
            &format_bytes(clip.size_bytes),
            rect(row.right - 146.0, row.top, row.right - 74.0, row.bottom),
            &self.small.clone(),
            self.palette.muted,
        )?;
        let star = rect(
            row.right - 68.0,
            row.top + 18.0,
            row.right - 42.0,
            row.bottom - 18.0,
        );
        self.glyph(
            Glyph::Star,
            rect(
                star.left + 3.0,
                star.top + 3.0,
                star.right - 3.0,
                star.bottom - 3.0,
            ),
            if model.is_favorite(index) {
                self.palette.primary
            } else if self.is_hovered(&favorite) {
                self.palette.secondary
            } else {
                self.palette.muted
            },
        )?;
        if model.is_favorite(index) {
            self.glyph(
                Glyph::StarFilled,
                rect(
                    star.left + 3.0,
                    star.top + 3.0,
                    star.right - 3.0,
                    star.bottom - 3.0,
                ),
                self.palette.primary,
            )?;
        }
        let more = rect(
            row.right - 34.0,
            row.top + 18.0,
            row.right - 8.0,
            row.bottom - 18.0,
        );
        self.glyph(
            Glyph::More,
            rect(
                more.left + 3.0,
                more.top + 7.0,
                more.right - 3.0,
                more.bottom - 7.0,
            ),
            if self.is_hovered(&menu) {
                self.palette.primary
            } else {
                self.palette.muted
            },
        )?;
        self.push_clipped_hit(row, viewport, open);
        if !model.selection_mode {
            self.push_clipped_hit(star, viewport, favorite);
            self.push_clipped_hit(more, viewport, menu);
        }
        Ok(())
    }

    fn overlay_button(
        &mut self,
        area: LogicalRect,
        glyph: Glyph,
        action: Action,
        viewport: LogicalRect,
    ) -> Result<(), String> {
        let hovered = self.is_hovered(&action);
        self.fill_alpha(
            area,
            if hovered {
                self.palette.surface_hover
            } else {
                0x000000
            },
            if hovered { 0.95 } else { 0.6 },
            RADIUS_SMALL,
        )?;
        self.glyph(
            glyph,
            rect(
                area.left + 5.0,
                area.top + 5.0,
                area.right - 5.0,
                area.bottom - 5.0,
            ),
            self.palette.primary,
        )?;
        self.push_clipped_hit(area, viewport, action);
        Ok(())
    }

    fn dropdown(&mut self, area: LogicalRect, value: &str, action: Action) -> Result<(), String> {
        let hovered = self.is_hovered(&action);
        self.fill(
            area,
            if hovered {
                self.hover_fill(self.palette.surface, self.palette.surface_hover)
            } else {
                self.palette.surface
            },
            RADIUS_SMALL,
        )?;
        self.stroke(
            area,
            if hovered {
                self.hover_edge(self.palette.border, self.palette.secondary)
            } else {
                self.palette.border
            },
            RADIUS_SMALL,
            1.0,
        )?;
        let chevron = rect(
            area.right - 26.0,
            (area.top + area.bottom) / 2.0 - 6.0,
            area.right - 14.0,
            (area.top + area.bottom) / 2.0 + 6.0,
        );
        let value_area = rect(area.left + 12.0, area.top, chevron.left - 6.0, area.bottom);
        self.text(
            &self.shorten(value, &self.body, value_area.right - value_area.left),
            value_area,
            &self.body.clone(),
            self.palette.primary,
        )?;
        self.glyph(Glyph::ChevronDown, chevron, self.palette.muted)?;
        self.hits.push(HitRegion { rect: area, action });
        Ok(())
    }

    fn view_button(
        &mut self,
        area: LogicalRect,
        glyph: Glyph,
        active: bool,
        action: Action,
    ) -> Result<(), String> {
        let hovered = self.is_hovered(&action);
        self.fill(
            area,
            if active {
                self.palette.surface_hover
            } else if hovered {
                self.hover_fill(self.palette.surface, self.palette.surface_hover)
            } else {
                self.palette.surface
            },
            RADIUS_SMALL,
        )?;
        self.stroke(area, self.palette.border, RADIUS_SMALL, 1.0)?;
        self.glyph(
            glyph,
            rect(
                area.left + 9.0,
                area.top + 9.0,
                area.right - 9.0,
                area.bottom - 9.0,
            ),
            if active || hovered {
                self.palette.primary
            } else {
                self.palette.secondary
            },
        )?;
        self.hits.push(HitRegion { rect: area, action });
        Ok(())
    }

    fn push_clip(&self, area: LogicalRect) -> Result<(), String> {
        let target = self.target.as_ref().expect("render target exists");
        unsafe { target.PushAxisAlignedClip(&area.d2d(), D2D1_ANTIALIAS_MODE_ALIASED) };
        Ok(())
    }

    fn with_offset<T>(
        &mut self,
        offset: f32,
        body: impl FnOnce(&mut Self) -> Result<T, String>,
    ) -> Result<T, String> {
        let first_hit = self.hits.len();
        self.set_translation(offset);
        let painted = body(self);
        self.set_translation(0.0);
        for hit in &mut self.hits[first_hit..] {
            hit.rect.top += offset;
            hit.rect.bottom += offset;
        }
        painted
    }

    fn set_translation(&self, offset: f32) {
        if let Some(target) = self.target.as_ref() {
            unsafe {
                target.SetTransform(&Matrix3x2 {
                    M11: 1.0,
                    M12: 0.0,
                    M21: 0.0,
                    M22: 1.0,
                    M31: 0.0,
                    M32: offset,
                });
            }
        }
    }

    fn pop_clip(&self) {
        if let Some(target) = self.target.as_ref() {
            unsafe { target.PopAxisAlignedClip() };
        }
    }

    fn push_clipped_hit(&mut self, area: LogicalRect, viewport: LogicalRect, action: Action) {
        let clipped = rect(
            area.left.max(viewport.left),
            area.top.max(viewport.top),
            area.right.min(viewport.right),
            area.bottom.min(viewport.bottom),
        );
        if clipped.right > clipped.left && clipped.bottom > clipped.top {
            self.hits.push(HitRegion {
                rect: clipped,
                action,
            });
        }
    }

    fn render_collections(
        &mut self,
        model: &UiModel,
        left: f32,
        right: f32,
        top: f32,
        bottom: f32,
    ) -> Result<(), String> {
        let today = crate::clock::now();
        self.text(
            self.strings.collections,
            rect(left, top, left + 320.0, top + 34.0),
            &self.page_title.clone(),
            self.palette.primary,
        )?;
        self.text(
            self.strings.collections_subtitle,
            rect(left, top + 44.0, left + 420.0, top + 68.0),
            &self.body.clone(),
            self.palette.muted,
        )?;

        let create = rect(right - 176.0, top + 4.0, right, top + 38.0);
        self.action_button(
            create,
            self.strings.new_collection_button,
            Action::CreateCollection,
        )?;
        let search = rect(
            (create.left - 246.0).max(left + 340.0),
            top + 4.0,
            create.left - 12.0,
            top + 38.0,
        );
        if search.right - search.left >= 120.0 {
            self.search_field(model, search, self.strings.search_collections)?;
        }
        self.fill(
            rect(left, top + 88.0, right, top + 89.0),
            self.palette.border,
            0.0,
        )?;

        let body_top = top + 100.0;
        let column = rect(left, body_top, left + FOLDER_COLUMN_WIDTH, bottom);
        self.render_folder_column(model, column)?;
        self.fill(
            rect(column.right + 12.0, body_top, column.right + 13.0, bottom),
            self.palette.border,
            0.0,
        )?;

        let area_left = column.right + 12.0 + FOLDER_COLUMN_GAP;
        let active = model.active_collection.as_ref().and_then(|path| {
            model
                .collections
                .iter()
                .find(|collection| &collection.path == path)
        });
        let title = active.map_or(self.strings.all_clips, |collection| {
            collection.name.as_str()
        });
        self.text(
            &self.shorten(title, &self.section, right - area_left - 240.0),
            rect(area_left, body_top - 4.0, right - 240.0, body_top + 22.0),
            &self.section.clone(),
            self.palette.primary,
        )?;
        if active.is_some() {
            let delete = rect(right - 84.0, body_top - 6.0, right, body_top + 22.0);
            let rename = rect(
                delete.left - 116.0,
                body_top - 6.0,
                delete.left - 12.0,
                body_top + 22.0,
            );
            for (area, label, action, destructive) in [
                (
                    rename,
                    self.strings.rename,
                    Action::RenameActiveCollection,
                    false,
                ),
                (
                    delete,
                    self.strings.delete,
                    Action::DeleteActiveCollection,
                    true,
                ),
            ] {
                let hovered = self.is_hovered(&action);
                self.text(
                    label,
                    area,
                    &self.small_right.clone(),
                    if destructive && hovered {
                        self.palette.destructive
                    } else if hovered {
                        self.palette.primary
                    } else {
                        self.palette.secondary
                    },
                )?;
                self.hits.push(HitRegion { rect: area, action });
            }
        }

        let clips_top = body_top + 34.0;
        let selecting = model.selection_mode && !model.selected_clips.is_empty();
        let area = rect(
            area_left,
            clips_top,
            right,
            if selecting { bottom - 60.0 } else { bottom },
        );
        let indices = model.visible_clip_indices_at(usize::MAX, today);
        if indices.is_empty() {
            self.empty_state(
                if active.is_some() {
                    self.strings.empty_collection
                } else {
                    self.strings.empty_no_clips
                },
                area.left,
                area.right,
                area.top + 8.0,
            )?;
        } else {
            let groups = model.clip_day_groups(&indices, today);
            let counts = groups
                .iter()
                .map(|group| group.indices.len())
                .collect::<Vec<_>>();
            let layout = library_layout(
                &counts,
                area.right - area.left - CLIP_SCROLL_RESERVE,
                model.library_grid,
            );
            let viewport_height = (area.bottom - area.top).max(0.0);
            let overflow = (layout.height - viewport_height).max(0.0);
            let scroll = model.library_scroll.clamp(0.0, overflow);
            self.push_clip(area)?;
            let painted = self.render_clip_sections(model, &groups, &layout, area, scroll, today);
            self.pop_clip();
            painted?;
            if overflow > 0.0 {
                let visible = (viewport_height / layout.height).clamp(0.1, 1.0);
                let thumb_height = viewport_height * visible;
                let thumb_top = area.top + (viewport_height - thumb_height) * (scroll / overflow);
                self.fill(
                    rect(
                        area.right - 4.0,
                        thumb_top,
                        area.right - 1.0,
                        thumb_top + thumb_height,
                    ),
                    self.palette.border,
                    1.5,
                )?;
            }
        }
        if selecting {
            self.selection_toolbar(model, area.left, area.right, bottom - 44.0)?;
        }
        if model.collection_picker_open {
            self.render_collection_picker(model, right, top + 44.0)?;
        }
        Ok(())
    }

    fn render_folder_column(&mut self, model: &UiModel, area: LogicalRect) -> Result<(), String> {
        self.text(
            self.strings.folders_caps,
            rect(
                area.left + 4.0,
                area.top - 4.0,
                area.left + 140.0,
                area.top + 18.0,
            ),
            &self.label.clone(),
            self.palette.muted,
        )?;
        let sort = Action::ToggleCollectionSort;
        let sort_area = rect(
            area.right - 70.0,
            area.top - 6.0,
            area.right,
            area.top + 18.0,
        );
        self.text(
            if model.collections_descending {
                self.strings.sort_descending
            } else {
                self.strings.sort_ascending
            },
            sort_area,
            &self.small_right.clone(),
            if self.is_hovered(&sort) {
                self.palette.primary
            } else {
                self.palette.muted
            },
        )?;
        self.hits.push(HitRegion {
            rect: sort_area,
            action: sort,
        });

        let dragging = model.clip_drag_preview.as_ref();
        let rows = rect(area.left, area.top + 26.0, area.right, area.bottom);
        let overflow = folder_column_overflow_in(rows, model.collections.len());
        let scroll = model.folder_scroll.clamp(0.0, overflow);
        self.push_clip(rows)?;
        let painted = self.render_folder_rows(model, rows, scroll, dragging);
        self.pop_clip();
        painted?;
        if overflow > 0.0 {
            let height = rows.bottom - rows.top;
            let visible = (height / (height + overflow)).clamp(0.1, 1.0);
            let thumb = height * visible;
            let top = rows.top + (height - thumb) * (scroll / overflow);
            self.fill(
                rect(rows.right - 3.0, top, rows.right - 1.0, top + thumb),
                self.palette.border,
                1.5,
            )?;
        }
        if model.collections.is_empty() {
            self.text(
                self.strings.no_collections,
                rect(
                    rows.left + 4.0,
                    rows.top + FOLDER_ROW_HEIGHT + 8.0,
                    rows.right,
                    rows.top + FOLDER_ROW_HEIGHT + 32.0,
                ),
                &self.small.clone(),
                self.palette.muted,
            )?;
        }
        Ok(())
    }

    fn render_folder_rows(
        &mut self,
        model: &UiModel,
        rows: LogicalRect,
        scroll: f32,
        dragging: Option<&crate::model::ClipDragPreview>,
    ) -> Result<(), String> {
        let area = rows;
        let mut row_top = area.top - scroll;
        self.folder_row(
            rect(area.left, row_top, area.right, row_top + FOLDER_ROW_HEIGHT),
            rows,
            Glyph::Library,
            self.strings.all_clips,
            model.clips.len(),
            model.active_collection.is_none(),
            false,
            Action::SelectCollection(None),
        )?;
        row_top += FOLDER_ROW_HEIGHT + 2.0;

        for index in model.visible_collection_indices() {
            if row_top > area.bottom {
                break;
            }
            if row_top + FOLDER_ROW_HEIGHT < area.top {
                row_top += FOLDER_ROW_HEIGHT + 2.0;
                continue;
            }
            let collection = &model.collections[index];
            self.folder_row(
                rect(area.left, row_top, area.right, row_top + FOLDER_ROW_HEIGHT),
                rows,
                Glyph::Folder,
                &collection.name,
                collection.clip_count,
                model.active_collection.as_ref() == Some(&collection.path),
                dragging.is_some_and(|drag| drag.target_collection == Some(index)),
                Action::SelectCollection(Some(index)),
            )?;
            row_top += FOLDER_ROW_HEIGHT + 2.0;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn folder_row(
        &mut self,
        area: LogicalRect,
        viewport: LogicalRect,
        glyph: Glyph,
        name: &str,
        count: usize,
        active: bool,
        drop_target: bool,
        action: Action,
    ) -> Result<(), String> {
        let hovered = self.is_hovered(&action);
        if active {
            self.fill(area, self.palette.surface_hover, RADIUS_SMALL)?;
        } else if hovered || drop_target {
            self.fill(
                area,
                mix(
                    self.palette.canvas,
                    self.palette.surface_hover,
                    if drop_target {
                        1.0
                    } else {
                        self.hover_amount(1.0)
                    },
                ),
                RADIUS_SMALL,
            )?;
        }
        if drop_target {
            self.stroke(area, self.palette.secondary, RADIUS_SMALL, 1.0)?;
        }
        let center = (area.top + area.bottom) / 2.0;
        self.glyph(
            glyph,
            rect(
                area.left + 9.0,
                center - 8.0,
                area.left + 25.0,
                center + 8.0,
            ),
            if active {
                self.palette.primary
            } else {
                self.palette.secondary
            },
        )?;
        let count_area = rect(area.right - 48.0, area.top, area.right - 10.0, area.bottom);
        self.text(
            &self.shorten(name, &self.body, count_area.left - (area.left + 34.0) - 8.0),
            rect(
                area.left + 34.0,
                area.top,
                count_area.left - 8.0,
                area.bottom,
            ),
            &self.body.clone(),
            if active {
                self.palette.primary
            } else {
                self.palette.secondary
            },
        )?;
        self.text(
            &count.to_string(),
            count_area,
            &self.small_right.clone(),
            self.palette.muted,
        )?;
        self.push_clipped_hit(area, viewport, action);
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
            self.strings.settings,
            self.strings.settings_subtitle,
            left,
            right,
        )?;
        self.pill(
            rect(right - 154.0, 86.0, right, 130.0),
            self.palette.accent,
            self.strings.save,
            self.palette.accent_text,
            Some(Action::SaveSettings),
        )?;
        let [general, capture, audio, storage] = settings_panel_rects(left, right, height);

        self.settings_panel(general, self.strings.panel_general)?;
        self.settings_compact_row(
            general,
            SETTINGS_GENERAL_ROWS,
            0,
            self.strings.autostart,
            self.strings.autostart_hint,
            if model.autostart_enabled {
                self.strings.on
            } else {
                self.strings.off
            },
            Action::ToggleAutostart,
            SettingControl::Toggle,
        )?;
        let shortcut = if model.hotkey_pending {
            self.strings.hotkey_activating.to_owned()
        } else if model.hotkey_capture {
            hotkey_capture_label(&model.hotkey_modifiers, self.strings)
        } else {
            wreath_windows::hotkey::localized_hotkey_label(&model.config.hotkey)
        };
        self.settings_compact_row(
            general,
            SETTINGS_GENERAL_ROWS,
            1,
            self.strings.replay_hotkey,
            self.strings.replay_hotkey_hint,
            &shortcut,
            Action::CaptureHotkey,
            SettingControl::Button,
        )?;
        let row_height = compact_settings_row_height(general, SETTINGS_GENERAL_ROWS);
        let hotkey_top = general.top + SETTINGS_PANEL_HEADER + row_height;
        let clear = rect(
            general.right - 48.0,
            hotkey_top + 7.0,
            general.right - 20.0,
            hotkey_top + row_height - 7.0,
        );
        self.fill(clear, self.palette.surface_raised, RADIUS_SMALL)?;
        self.glyph(
            Glyph::Close,
            rect(
                clear.left + 3.0,
                clear.top + 3.0,
                clear.right - 3.0,
                clear.bottom - 3.0,
            ),
            self.palette.secondary,
        )?;
        self.hits.push(HitRegion {
            rect: clear,
            action: Action::ClearHotkey,
        });
        self.settings_compact_row(
            general,
            SETTINGS_GENERAL_ROWS,
            2,
            self.strings.theme_row,
            self.strings.theme_hint,
            theme_label(model.config.appearance.theme, self.strings),
            Action::ChooseTheme,
            SettingControl::Dropdown,
        )?;
        self.settings_compact_row(
            general,
            SETTINGS_GENERAL_ROWS,
            3,
            self.strings.hover_row,
            self.strings.hover_hint,
            hover_style_label(model.config.appearance.hover, self.strings),
            Action::ChooseHoverStyle,
            SettingControl::Dropdown,
        )?;
        self.settings_compact_row(
            general,
            SETTINGS_GENERAL_ROWS,
            4,
            self.strings.hover_strength_row,
            self.strings.hover_strength_hint,
            hover_strength_label(model.config.appearance.hover_strength, self.strings),
            Action::ChooseHoverStrength,
            SettingControl::Dropdown,
        )?;
        self.settings_compact_row(
            general,
            SETTINGS_GENERAL_ROWS,
            5,
            self.strings.language_row,
            self.strings.language_hint,
            language_label(model.config.appearance.language, self.strings),
            Action::ChooseLanguage,
            SettingControl::Dropdown,
        )?;

        self.settings_panel(capture, self.strings.panel_capture)?;
        let display = model
            .selected_display()
            .map_or(self.strings.primary_display, |display| {
                display.label.as_str()
            });
        self.settings_compact_row(
            capture,
            SETTINGS_CAPTURE_ROWS,
            0,
            self.strings.display_row,
            self.strings.display_hint,
            display,
            Action::ChooseDisplay,
            SettingControl::Dropdown,
        )?;
        self.settings_compact_row(
            capture,
            SETTINGS_CAPTURE_ROWS,
            1,
            self.strings.clip_duration,
            self.strings.clip_duration_hint,
            &self.strings.seconds(model.config.capture.duration_seconds),
            Action::ChooseDuration,
            SettingControl::Dropdown,
        )?;
        self.settings_compact_row(
            capture,
            SETTINGS_CAPTURE_ROWS,
            2,
            self.strings.frame_rate,
            self.strings.frame_rate_hint,
            &self
                .strings
                .frames_per_second(model.config.capture.frames_per_second),
            Action::ChooseFrameRate,
            SettingControl::Dropdown,
        )?;
        self.settings_compact_row(
            capture,
            SETTINGS_CAPTURE_ROWS,
            3,
            self.strings.video_quality,
            self.strings.video_quality_hint,
            &quality_label(model.config.capture.quality),
            Action::ChooseQuality,
            SettingControl::Dropdown,
        )?;
        self.settings_compact_row(
            capture,
            SETTINGS_CAPTURE_ROWS,
            4,
            self.strings.codec,
            self.strings.codec_hint,
            &format!("{:?}", model.config.capture.codec),
            Action::ChooseCodec,
            SettingControl::Dropdown,
        )?;
        self.settings_compact_row(
            capture,
            SETTINGS_CAPTURE_ROWS,
            5,
            self.strings.capture_cursor,
            self.strings.capture_cursor_hint,
            if model.config.capture.cursor {
                self.strings.on
            } else {
                self.strings.off
            },
            Action::ToggleCursor,
            SettingControl::Toggle,
        )?;

        self.settings_panel(audio, self.strings.panel_audio)?;
        self.settings_compact_row(
            audio,
            SETTINGS_AUDIO_ROWS,
            0,
            self.strings.system_audio,
            self.strings.system_audio_hint,
            if model.config.audio.desktop {
                self.strings.on
            } else {
                self.strings.off
            },
            Action::ToggleDesktopAudio,
            SettingControl::Toggle,
        )?;
        let output_name = model
            .config
            .audio
            .desktop_device
            .as_ref()
            .and_then(|id| {
                model
                    .output_names
                    .iter()
                    .find(|(device_id, _)| device_id == id)
            })
            .map_or(self.strings.windows_default, |(_, name)| name.as_str());
        self.settings_compact_row(
            audio,
            SETTINGS_AUDIO_ROWS,
            1,
            self.strings.output_device,
            self.strings.output_device_hint,
            output_name,
            Action::ChooseDesktopDevice,
            SettingControl::Dropdown,
        )?;
        self.settings_gain_slider(
            audio,
            SETTINGS_AUDIO_ROWS,
            2,
            self.strings.system_level,
            self.strings.system_level_hint,
            model.config.audio.desktop_gain_percent,
            Action::DragDesktopGain,
        )?;
        self.settings_compact_row(
            audio,
            SETTINGS_AUDIO_ROWS,
            3,
            self.strings.microphone,
            self.strings.microphone_hint,
            if model.config.audio.microphone {
                self.strings.on
            } else {
                self.strings.off
            },
            Action::ToggleMicrophone,
            SettingControl::Toggle,
        )?;
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
            .map_or(self.strings.windows_default, |(_, name)| name.as_str());
        self.settings_compact_row(
            audio,
            SETTINGS_AUDIO_ROWS,
            4,
            self.strings.microphone_device,
            self.strings.microphone_device_hint,
            microphone_name,
            Action::ChooseMicrophone,
            SettingControl::Dropdown,
        )?;
        self.settings_gain_slider(
            audio,
            SETTINGS_AUDIO_ROWS,
            5,
            self.strings.microphone_level,
            self.strings.microphone_level_hint,
            model.config.audio.microphone_gain_percent,
            Action::DragMicrophoneGain,
        )?;

        self.settings_panel(storage, self.strings.panel_storage)?;
        self.settings_compact_row(
            storage,
            SETTINGS_STORAGE_ROWS,
            0,
            self.strings.storage_location,
            self.strings.storage_location_hint,
            &model.config.storage.directory.display().to_string(),
            Action::ChooseStorage,
            SettingControl::Button,
        )?;
        self.settings_compact_row(
            storage,
            SETTINGS_STORAGE_ROWS,
            1,
            self.strings.storage_limit,
            self.strings.storage_limit_hint,
            &format_storage_limit(model.config.storage.max_megabytes),
            Action::ChooseStorageLimit,
            SettingControl::Dropdown,
        )?;
        self.text(
            &self.strings.version_line(env!("CARGO_PKG_VERSION")),
            rect(
                storage.left + 20.0,
                storage.top
                    + 44.0
                    + 2.0 * compact_settings_row_height(storage, SETTINGS_STORAGE_ROWS)
                    + 12.0,
                storage.right - 20.0,
                storage.bottom - 14.0,
            ),
            &self.small.clone(),
            self.palette.muted,
        )?;
        Ok(())
    }

    fn render_player(
        &mut self,
        model: &UiModel,
        left: f32,
        right: f32,
        height: f32,
    ) -> Result<(), String> {
        self.page_heading(
            self.strings.preview_title,
            self.strings.preview_subtitle,
            left,
            right,
        )?;
        self.pill(
            rect(right - 344.0, 92.0, right - 218.0, 140.0),
            self.palette.surface,
            &format!("←  {}", self.strings.back),
            self.palette.primary,
            Some(Action::Back),
        )?;
        self.pill(
            rect(right - 200.0, 92.0, right, 140.0),
            self.palette.accent,
            self.strings.edit_clip,
            self.palette.canvas,
            Some(Action::EditActiveClip),
        )?;
        let Some(clip) = model.active_clip() else {
            self.empty_state(self.strings.clip_unavailable, left, right, 240.0)?;
            return Ok(());
        };
        let detail_width = if right - left >= 960.0 { 276.0 } else { 0.0 };
        let main_right = right
            - if detail_width > 0.0 {
                detail_width + 24.0
            } else {
                0.0
            };
        let stage = fit_aspect(
            rect(left, 176.0, main_right, (height - 178.0).max(390.0)),
            model.player_aspect_ratio,
        );
        self.fill(stage, self.palette.stage, RADIUS_LARGE)?;
        self.stroke(stage, self.palette.border, RADIUS_LARGE, 1.0)?;
        self.hits.push(HitRegion {
            rect: stage,
            action: Action::PlayPause,
        });
        self.render_media_controls(model, stage, false)?;

        if detail_width > 0.0 {
            let detail_left = right - detail_width;
            let info = rect(detail_left, stage.top, right, (height - 24.0).min(620.0));
            self.clip_information_panel(model, clip, info, true, model.active_clip)?;
        }
        Ok(())
    }

    fn render_media_controls(
        &mut self,
        model: &UiModel,
        stage: LogicalRect,
        editor: bool,
    ) -> Result<(), String> {
        let controls = rect(stage.left, stage.bottom, stage.right, stage.bottom + 76.0);
        self.fill(controls, self.palette.surface, 0.0)?;
        self.stroke(controls, self.palette.border, 0.0, 1.0)?;
        self.floating_glyph(
            rect(
                controls.left + 12.0,
                controls.top + 16.0,
                controls.left + 54.0,
                controls.bottom - 16.0,
            ),
            if model.player_playing {
                Glyph::Pause
            } else {
                Glyph::Play
            },
            self.palette.primary,
            Some(Action::PlayPause),
        )?;
        self.floating_glyph(
            rect(
                controls.left + 58.0,
                controls.top + 16.0,
                controls.left + 98.0,
                controls.bottom - 16.0,
            ),
            Glyph::ChevronLeft,
            self.palette.secondary,
            (!editor).then_some(Action::PreviousClip),
        )?;
        self.floating_glyph(
            rect(
                controls.left + 102.0,
                controls.top + 16.0,
                controls.left + 142.0,
                controls.bottom - 16.0,
            ),
            Glyph::ChevronRight,
            self.palette.secondary,
            (!editor).then_some(Action::NextClip),
        )?;
        self.text(
            &format!(
                "{} / {}",
                format_player_time(model.player_position_seconds),
                format_player_time(model.player_duration_seconds)
            ),
            rect(
                controls.left + 154.0,
                controls.top,
                controls.left + 252.0,
                controls.bottom,
            ),
            &self.small.clone(),
            self.palette.secondary,
        )?;
        let rail = rect(
            controls.left + 260.0,
            controls.top + 35.0,
            controls.right - 64.0,
            controls.top + 41.0,
        );
        self.draw_progress_rail(model, rail)?;
        self.hits.push(HitRegion {
            rect: rect(
                rail.left,
                controls.top + 12.0,
                rail.right,
                controls.bottom - 12.0,
            ),
            action: if editor {
                Action::DragEditorPlayhead
            } else {
                Action::DragPlayerSeek
            },
        });
        self.floating_glyph(
            rect(
                controls.right - 52.0,
                controls.top + 16.0,
                controls.right - 10.0,
                controls.bottom - 16.0,
            ),
            Glyph::Fullscreen,
            self.palette.primary,
            (!editor).then_some(Action::ToggleFullscreen),
        )
    }

    fn draw_progress_rail(&self, model: &UiModel, rail: LogicalRect) -> Result<(), String> {
        self.fill(rail, self.palette.surface_hover, 3.0)?;
        let progress = if model.player_duration_seconds > 0.0 {
            (model.player_position_seconds / model.player_duration_seconds).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        let x = rail.left + (rail.right - rail.left) * progress;
        self.fill(
            rect(rail.left, rail.top, x, rail.bottom),
            self.palette.accent,
            3.0,
        )?;
        self.fill(
            rect(
                x - 5.0,
                (rail.top + rail.bottom) / 2.0 - 5.0,
                x + 5.0,
                (rail.top + rail.bottom) / 2.0 + 5.0,
            ),
            self.palette.primary,
            5.0,
        )
    }

    fn clip_information_panel(
        &mut self,
        model: &UiModel,
        clip: &wreath_core::clips::Clip,
        area: LogicalRect,
        allow_rename: bool,
        delete_for: Option<usize>,
    ) -> Result<(), String> {
        self.fill(area, self.palette.surface, RADIUS_LARGE)?;
        self.stroke(area, self.palette.border, RADIUS_LARGE, 1.0)?;
        self.text(
            self.strings.clip_information,
            rect(
                area.left + 20.0,
                area.top + 10.0,
                area.right - 20.0,
                area.top + 43.0,
            ),
            &self.body.clone(),
            self.palette.primary,
        )?;
        let resolution = if model.player_video_width > 0 && model.player_video_height > 0 {
            format!("{}×{}", model.player_video_width, model.player_video_height)
        } else {
            self.strings.loading.to_owned()
        };
        let rows = [
            (self.strings.field_title, clip.title.clone()),
            (
                self.strings.field_created,
                crate::clock::stamp_label(
                    crate::clock::local(clip.modified),
                    crate::clock::now(),
                    self.strings,
                ),
            ),
            (
                self.strings.field_duration,
                format_player_time(model.player_duration_seconds),
            ),
            (self.strings.field_size, format_bytes(clip.size_bytes)),
            (self.strings.field_resolution, resolution),
        ];
        let reserved_actions = if delete_for.is_some() { 62.0 } else { 0.0 };
        let row_height = ((area.bottom - area.top - 54.0 - reserved_actions) / rows.len() as f32)
            .clamp(43.0, 55.0);
        for (index, (label, value)) in rows.into_iter().enumerate() {
            let top = area.top + 54.0 + index as f32 * row_height;
            let has_title_action = index == 0 && allow_rename;
            self.text(
                label,
                rect(area.left + 20.0, top, area.right - 20.0, top + 24.0),
                &self.small.clone(),
                self.palette.secondary,
            )?;
            let value_area = rect(
                area.left + 20.0,
                top + 19.0,
                if has_title_action {
                    area.right - 56.0
                } else {
                    area.right - 20.0
                },
                top + row_height,
            );
            self.text(
                &self.shorten(&value, &self.body, value_area.right - value_area.left),
                value_area,
                &self.body.clone(),
                self.palette.primary,
            )?;
            if has_title_action {
                let action = Action::RenameActiveClip;
                let hit = rect(area.right - 52.0, top + 17.0, area.right - 12.0, top + 51.0);
                self.glyph(
                    Glyph::Pencil,
                    rect(
                        hit.left + 11.0,
                        hit.top + 9.0,
                        hit.right - 11.0,
                        hit.bottom - 9.0,
                    ),
                    if self.is_hovered(&action) {
                        self.palette.primary
                    } else {
                        self.palette.secondary
                    },
                )?;
                self.hits.push(HitRegion { rect: hit, action });
            }
        }
        if let Some(index) = delete_for {
            let delete = rect(
                area.left + 18.0,
                area.bottom - 50.0,
                area.right - 18.0,
                area.bottom - 12.0,
            );
            self.pill(
                delete,
                self.palette.stage,
                self.strings.delete_clip,
                self.palette.destructive,
                Some(Action::DeleteClip(index)),
            )?;
        }
        Ok(())
    }

    fn render_editor(
        &mut self,
        model: &UiModel,
        left: f32,
        right: f32,
        height: f32,
    ) -> Result<(), String> {
        self.text(
            self.strings.edit_clip,
            rect(left, 62.0, right - 470.0, 94.0),
            &self.brand.clone(),
            self.palette.primary,
        )?;
        self.text(
            self.strings.editor_subtitle,
            rect(left, 94.0, right - 470.0, 122.0),
            &self.body.clone(),
            self.palette.secondary,
        )?;
        let Some(clip) = model.active_clip() else {
            self.empty_state(self.strings.clip_unavailable, left, right, 240.0)?;
            return Ok(());
        };
        let enabled = model.editor_timing.is_some() && !model.editor_working;
        let undo_enabled = enabled && model.can_undo_editor_trim();
        let redo_enabled = enabled && model.can_redo_editor_trim();
        self.pill(
            rect(right - 430.0, 82.0, right - 386.0, 126.0),
            self.palette.surface,
            "↶",
            if undo_enabled {
                self.palette.primary
            } else {
                self.palette.border
            },
            undo_enabled.then_some(Action::UndoEditorTrim),
        )?;
        self.pill(
            rect(right - 374.0, 82.0, right - 330.0, 126.0),
            self.palette.surface,
            "↷",
            if redo_enabled {
                self.palette.primary
            } else {
                self.palette.border
            },
            redo_enabled.then_some(Action::RedoEditorTrim),
        )?;
        self.pill(
            rect(right - 320.0, 82.0, right - 174.0, 126.0),
            self.palette.surface,
            self.strings.discard,
            self.palette.primary,
            Some(Action::Back),
        )?;
        self.pill(
            rect(right - 158.0, 82.0, right, 126.0),
            if enabled {
                self.palette.accent
            } else {
                self.palette.surface_hover
            },
            if model.editor_working {
                self.strings.saving
            } else {
                self.strings.save
            },
            if enabled {
                self.palette.canvas
            } else {
                self.palette.secondary
            },
            enabled.then_some(if model.trim_replace_original {
                Action::ReplaceCut
            } else {
                Action::SaveCut
            }),
        )?;

        let detail_width = if right - left >= 960.0 { 276.0 } else { 0.0 };
        let main_right = right
            - if detail_width > 0.0 {
                detail_width + 24.0
            } else {
                0.0
            };
        let stage = fit_aspect(
            rect(
                left,
                160.0,
                main_right,
                (height - EDITOR_BOTTOM_RESERVE).max(360.0),
            ),
            model.player_aspect_ratio,
        );
        self.fill(stage, self.palette.stage, RADIUS_LARGE)?;
        self.stroke(stage, self.palette.border, RADIUS_LARGE, 1.0)?;
        self.hits.push(HitRegion {
            rect: stage,
            action: Action::PlayPause,
        });
        self.render_media_controls(model, stage, true)?;

        if detail_width > 0.0 {
            let detail_left = right - detail_width;
            let info = rect(
                detail_left,
                stage.top,
                right,
                (stage.top + 276.0).min(height - 420.0),
            );
            self.clip_information_panel(model, clip, info, true, None)?;
            let duration = rect(detail_left, info.bottom + 16.0, right, info.bottom + 104.0);
            self.fill(duration, self.palette.surface, RADIUS_LARGE)?;
            self.stroke(duration, self.palette.border, RADIUS_LARGE, 1.0)?;
            self.text(
                self.strings.trimmed_duration,
                rect(
                    duration.left + 18.0,
                    duration.top + 8.0,
                    duration.right - 18.0,
                    duration.top + 38.0,
                ),
                &self.body.clone(),
                self.palette.primary,
            )?;
            self.text(
                &format!(
                    "{} — {}",
                    format_editor_time(model.editor_start),
                    format_editor_time(model.editor_end)
                ),
                rect(
                    duration.left + 18.0,
                    duration.top + 42.0,
                    duration.right - 86.0,
                    duration.bottom - 8.0,
                ),
                &self.small.clone(),
                self.palette.secondary,
            )?;
            self.text(
                &format_editor_time(model.editor_selected_duration()),
                rect(
                    duration.right - 82.0,
                    duration.top + 38.0,
                    duration.right - 14.0,
                    duration.bottom - 8.0,
                ),
                &self.body_center.clone(),
                self.palette.primary,
            )?;
            let save_mode = rect(
                detail_left,
                duration.bottom + 14.0,
                right,
                duration.bottom + 80.0,
            );
            self.fill(save_mode, self.palette.surface, RADIUS_LARGE)?;
            self.stroke(save_mode, self.palette.border, RADIUS_LARGE, 1.0)?;
            self.text(
                self.strings.save_as_new,
                rect(
                    save_mode.left + 14.0,
                    save_mode.top + 3.0,
                    save_mode.right - 14.0,
                    save_mode.top + 26.0,
                ),
                &self.small.clone(),
                self.palette.secondary,
            )?;
            let choices = rect(
                save_mode.left + 10.0,
                save_mode.top + 28.0,
                save_mode.right - 10.0,
                save_mode.bottom - 8.0,
            );
            let split = (choices.left + choices.right) / 2.0;
            self.pill(
                rect(choices.left, choices.top, split - 3.0, choices.bottom),
                if model.trim_replace_original {
                    self.palette.stage
                } else {
                    self.palette.accent
                },
                self.strings.new_clip,
                if model.trim_replace_original {
                    self.palette.primary
                } else {
                    self.palette.canvas
                },
                Some(Action::SetTrimReplace(false)),
            )?;
            self.pill(
                rect(split + 3.0, choices.top, choices.right, choices.bottom),
                if model.trim_replace_original {
                    self.palette.accent
                } else {
                    self.palette.stage
                },
                self.strings.replace_original,
                if model.trim_replace_original {
                    self.palette.canvas
                } else {
                    self.palette.primary
                },
                Some(Action::SetTrimReplace(true)),
            )?;
        }

        let timeline_top = stage.bottom + 92.0;
        let timeline = rect(
            left,
            timeline_top,
            right,
            (timeline_top + EDITOR_TIMELINE_HEIGHT).min(height - 16.0),
        );
        self.fill(timeline, self.palette.surface, RADIUS_LARGE)?;
        self.stroke(timeline, self.palette.border, RADIUS_LARGE, 1.0)?;
        self.timeline_labels(
            model,
            rect(
                timeline.left + 24.0,
                timeline.top + 6.0,
                timeline.right - 24.0,
                timeline.top + 32.0,
            ),
        )?;
        let storyboard = rect(
            timeline.left + 24.0,
            timeline.top + 34.0,
            timeline.right - 24.0,
            timeline.top + 94.0,
        );
        self.trim_storyboard(model, clip, storyboard)?;
        Ok(())
    }

    fn timeline_labels(&self, model: &UiModel, area: LogicalRect) -> Result<(), String> {
        let duration = model
            .editor_timing
            .as_ref()
            .map_or(0.0, |timing| timing.duration.as_secs_f64());
        for step in 0..=6 {
            let x = area.left + (area.right - area.left) * step as f32 / 6.0;
            let label_area = match step {
                0 => rect(x + 10.0, area.top, x + 90.0, area.bottom),
                6 => rect(x - 90.0, area.top, x - 10.0, area.bottom),
                _ => rect(x - 30.0, area.top, x + 50.0, area.bottom),
            };
            self.text(
                &format_player_time(duration * step as f64 / 6.0),
                label_area,
                &self.small.clone(),
                self.palette.secondary,
            )?;
        }
        Ok(())
    }

    fn trim_storyboard(
        &mut self,
        model: &UiModel,
        clip: &wreath_core::clips::Clip,
        area: LogicalRect,
    ) -> Result<(), String> {
        self.fill(area, self.palette.stage, RADIUS_SMALL)?;
        let segment_width = (area.right - area.left) / 8.0;
        for segment in 0..8 {
            let preview = rect(
                area.left + segment as f32 * segment_width,
                area.top,
                area.left + (segment + 1) as f32 * segment_width,
                area.bottom,
            );
            let _ = self.draw_thumbnail(&clip.path, preview)?;
        }
        let duration = model
            .editor_timing
            .as_ref()
            .map_or(0.0, |timing| timing.duration.as_secs_f64());
        if duration <= 0.0 {
            return Ok(());
        }
        let start_x = area.left
            + (area.right - area.left) * (model.editor_start.as_secs_f64() / duration) as f32;
        let end_x = area.left
            + (area.right - area.left) * (model.editor_end.as_secs_f64() / duration) as f32;
        let playhead_x = area.left
            + (area.right - area.left)
                * (model.player_position_seconds / duration).clamp(0.0, 1.0) as f32;
        self.stroke(
            rect(start_x, area.top, end_x, area.bottom),
            self.palette.primary,
            2.0,
            2.0,
        )?;
        self.fill(
            rect(
                start_x - 8.0,
                area.top - 2.0,
                start_x + 8.0,
                area.bottom + 2.0,
            ),
            self.palette.primary,
            5.0,
        )?;
        self.fill(
            rect(end_x - 8.0, area.top - 2.0, end_x + 8.0, area.bottom + 2.0),
            self.palette.primary,
            5.0,
        )?;
        self.fill(
            rect(
                playhead_x - 1.0,
                area.top - 56.0,
                playhead_x + 1.0,
                area.bottom + 24.0,
            ),
            self.palette.primary,
            0.0,
        )?;
        self.fill(
            rect(
                playhead_x - 5.0,
                area.top - 60.0,
                playhead_x + 5.0,
                area.top - 50.0,
            ),
            self.palette.primary,
            5.0,
        )?;
        self.hits.push(HitRegion {
            rect: rect(area.left, area.top - 20.0, area.right, area.bottom + 20.0),
            action: Action::DragEditorPlayhead,
        });
        self.hits.push(HitRegion {
            rect: rect(
                start_x - 14.0,
                area.top - 8.0,
                start_x + 14.0,
                area.bottom + 8.0,
            ),
            action: Action::DragEditorStart,
        });
        self.hits.push(HitRegion {
            rect: rect(
                end_x - 14.0,
                area.top - 8.0,
                end_x + 14.0,
                area.bottom + 8.0,
            ),
            action: Action::DragEditorEnd,
        });
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_wreath_logo(&self, area: LogicalRect, fill: u32) -> Result<(), String> {
        use windows::Win32::Graphics::Direct2D::ID2D1StrokeStyle;
        let target = self.target.as_ref().expect("render target exists");
        let outline =
            unsafe { target.CreateSolidColorBrush(&color(self.palette.surface_raised), None) }
                .map_err(|error| error.to_string())?;
        let foreground = unsafe { target.CreateSolidColorBrush(&color(fill), None) }
            .map_err(|error| error.to_string())?;
        let width = area.right - area.left;
        let height = area.bottom - area.top;
        let point = |x: f32, y: f32| Vector2 {
            X: area.left + width * x / 24.0,
            Y: area.top + height * y / 24.0,
        };
        let segments = [
            (7.875, 4.688, 4.688, 4.688),
            (4.688, 4.688, 4.688, 7.875),
            (16.125, 4.688, 19.312, 4.688),
            (19.312, 4.688, 19.312, 7.875),
            (7.875, 19.312, 4.688, 19.312),
            (4.688, 19.312, 4.688, 16.125),
            (16.125, 19.312, 19.312, 19.312),
            (19.312, 19.312, 19.312, 16.125),
            (9.562, 8.438, 9.562, 15.562),
            (14.438, 8.438, 14.438, 15.562),
        ];
        let base = width.min(height);
        for (brush, stroke) in [
            (&outline, base * 17.0 / 128.0),
            (&foreground, base * 10.0 / 128.0),
        ] {
            for (from_x, from_y, to_x, to_y) in segments {
                let from = point(from_x, from_y);
                let to = point(to_x, to_y);
                unsafe {
                    target.DrawLine(from, to, brush, stroke, None::<&ID2D1StrokeStyle>);
                    for center in [from, to] {
                        target.FillEllipse(
                            &D2D1_ELLIPSE {
                                point: center,
                                radiusX: stroke / 2.0,
                                radiusY: stroke / 2.0,
                            },
                            brush,
                        );
                    }
                }
            }
        }
        Ok(())
    }

    fn toggle_visual(&self, area: LogicalRect, enabled: bool) -> Result<(), String> {
        self.fill(
            area,
            if enabled {
                self.palette.primary
            } else {
                self.palette.surface_hover
            },
            (area.bottom - area.top) / 2.0,
        )?;
        self.stroke(
            area,
            if enabled {
                self.palette.primary
            } else {
                self.palette.secondary
            },
            (area.bottom - area.top) / 2.0,
            1.0,
        )?;
        let diameter = area.bottom - area.top - 4.0;
        let left = if enabled {
            area.right - diameter - 2.0
        } else {
            area.left + 2.0
        };
        self.fill(
            rect(left, area.top + 2.0, left + diameter, area.bottom - 2.0),
            if enabled {
                self.palette.canvas
            } else {
                self.palette.secondary
            },
            diameter / 2.0,
        )
    }

    fn icon_button(
        &mut self,
        area: LogicalRect,
        glyph: Glyph,
        action: Action,
    ) -> Result<(), String> {
        self.fill(
            area,
            if self.is_hovered(&action) {
                self.palette.surface_hover
            } else {
                self.palette.surface
            },
            RADIUS,
        )?;
        self.stroke(area, self.palette.border, RADIUS, 1.0)?;
        self.glyph(
            glyph,
            rect(
                area.left + 13.0,
                area.top + 11.0,
                area.right - 13.0,
                area.bottom - 11.0,
            ),
            self.palette.primary,
        )?;
        self.hits.push(HitRegion { rect: area, action });
        Ok(())
    }

    fn search_field(
        &mut self,
        model: &UiModel,
        area: LogicalRect,
        placeholder: &str,
    ) -> Result<(), String> {
        let action = Action::Search;
        self.fill(area, self.palette.surface, RADIUS)?;
        self.stroke(
            area,
            if model.search_focused {
                self.palette.primary
            } else {
                self.palette.border
            },
            RADIUS,
            1.0,
        )?;
        self.glyph(
            Glyph::Search,
            rect(
                area.left + 15.0,
                area.top + 13.0,
                area.left + 37.0,
                area.bottom - 10.0,
            ),
            self.palette.secondary,
        )?;
        self.render_text_input(
            &model.search,
            rect(area.left + 48.0, area.top, area.right - 16.0, area.bottom),
            placeholder,
            model.search_focused,
            TextInputTarget::Search,
        )?;
        self.hits.push(HitRegion { rect: area, action });
        Ok(())
    }

    fn settings_panel(&self, area: LogicalRect, title: &str) -> Result<(), String> {
        self.fill(area, self.palette.surface, RADIUS_LARGE)?;
        self.stroke(area, self.palette.border, RADIUS_LARGE, 1.0)?;
        self.text(
            title,
            rect(
                area.left + 20.0,
                area.top + 8.0,
                area.right - 20.0,
                area.top + 48.0,
            ),
            &self.body.clone(),
            self.palette.primary,
        )
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_arguments)]
    fn settings_compact_row(
        &mut self,
        panel: LogicalRect,
        rows: usize,
        index: usize,
        title: &str,
        description: &str,
        value: &str,
        action: Action,
        control: SettingControl,
    ) -> Result<(), String> {
        let row_height = compact_settings_row_height(panel, rows);
        let top = panel.top + SETTINGS_PANEL_HEADER + index as f32 * row_height;
        if index > 0 {
            self.fill(
                rect(panel.left + 20.0, top, panel.right - 20.0, top + 1.0),
                self.palette.border,
                0.0,
            )?;
        }
        let control_width = ((panel.right - panel.left) * 0.34).clamp(128.0, 205.0);
        let control_area = rect(
            panel.right - control_width - 20.0,
            top + 6.0,
            panel.right - 20.0,
            top + row_height - 6.0,
        );
        // a short row drops the description instead of overlapping it
        let roomy = row_height >= 34.0;
        self.text(
            title,
            rect(
                panel.left + 20.0,
                if roomy { top + 2.0 } else { top },
                control_area.left - 16.0,
                if roomy {
                    top + row_height * 0.50
                } else {
                    top + row_height
                },
            ),
            &self.body.clone(),
            self.palette.primary,
        )?;
        if roomy {
            self.text(
                description,
                rect(
                    panel.left + 20.0,
                    top + row_height * 0.45,
                    control_area.left - 16.0,
                    top + row_height,
                ),
                &self.small.clone(),
                self.palette.secondary,
            )?;
        }
        match control {
            SettingControl::Toggle => self.toggle_visual(
                rect(
                    control_area.right - 42.0,
                    control_area.top + 7.0,
                    control_area.right,
                    control_area.bottom - 7.0,
                ),
                value == "An",
            )?,
            SettingControl::Dropdown => {
                self.fill(control_area, self.palette.stage, RADIUS_SMALL)?;
                self.stroke(control_area, self.palette.border, RADIUS_SMALL, 1.0)?;
                let clipped = self.shorten(value, &self.small, control_width - 50.0);
                self.text(
                    &clipped,
                    rect(
                        control_area.left + 12.0,
                        control_area.top,
                        control_area.right - 34.0,
                        control_area.bottom,
                    ),
                    &self.small.clone(),
                    self.palette.primary,
                )?;
                self.glyph(
                    Glyph::ChevronDown,
                    rect(
                        control_area.right - 27.0,
                        control_area.top + 10.0,
                        control_area.right - 11.0,
                        control_area.bottom - 10.0,
                    ),
                    self.palette.secondary,
                )?;
            }
            SettingControl::Button => {
                self.fill(control_area, self.palette.stage, RADIUS_SMALL)?;
                self.stroke(control_area, self.palette.border, RADIUS_SMALL, 1.0)?;
                let clipped = self.shorten(value, &self.small, control_width - 32.0);
                self.text(
                    &clipped,
                    rect(
                        control_area.left + 10.0,
                        control_area.top,
                        control_area.right - 10.0,
                        control_area.bottom,
                    ),
                    &self.small.clone(),
                    self.palette.primary,
                )?;
            }
        }
        self.hits.push(HitRegion {
            rect: control_area,
            action,
        });
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn settings_gain_slider(
        &mut self,
        panel: LogicalRect,
        rows: usize,
        index: usize,
        title: &str,
        description: &str,
        value: u16,
        action: Action,
    ) -> Result<(), String> {
        let row_height = compact_settings_row_height(panel, rows);
        let top = panel.top + SETTINGS_PANEL_HEADER + index as f32 * row_height;
        if index > 0 {
            self.fill(
                rect(panel.left + 20.0, top, panel.right - 20.0, top + 1.0),
                self.palette.border,
                0.0,
            )?;
        }
        let control_area = settings_control_area(panel, rows, index);
        let roomy = row_height >= 34.0;
        self.text(
            title,
            rect(
                panel.left + 20.0,
                if roomy { top + 2.0 } else { top },
                control_area.left - 16.0,
                if roomy {
                    top + row_height * 0.50
                } else {
                    top + row_height
                },
            ),
            &self.body.clone(),
            self.palette.primary,
        )?;
        if roomy {
            self.text(
                description,
                rect(
                    panel.left + 20.0,
                    top + row_height * 0.45,
                    control_area.left - 16.0,
                    top + row_height,
                ),
                &self.small.clone(),
                self.palette.secondary,
            )?;
        }

        let rail = settings_gain_rail_in_panel(panel, rows, index);
        let fraction = f32::from(value.min(200)) / 200.0;
        let knob_x = rail.left + (rail.right - rail.left) * fraction;
        self.fill(rail, self.palette.border, 2.0)?;
        if knob_x > rail.left {
            self.fill(
                rect(rail.left, rail.top, knob_x, rail.bottom),
                self.palette.primary,
                2.0,
            )?;
        }
        let knob_size = if self.is_hovered(&action) { 12.0 } else { 10.0 };
        self.fill(
            rect(
                knob_x - knob_size / 2.0,
                (rail.top + rail.bottom - knob_size) / 2.0,
                knob_x + knob_size / 2.0,
                (rail.top + rail.bottom + knob_size) / 2.0,
            ),
            self.palette.primary,
            knob_size / 2.0,
        )?;
        self.text(
            &format!("{}%", value.min(200)),
            rect(
                rail.right + 12.0,
                control_area.top,
                control_area.right,
                control_area.bottom,
            ),
            &self.small.clone(),
            self.palette.primary,
        )?;
        self.hits.push(HitRegion {
            rect: control_area,
            action,
        });
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
            rect(left, 58.0, right, 99.0),
            &self.page_title.clone(),
            self.palette.primary,
        )?;
        self.text(
            subtitle,
            rect(left, 99.0, right, 128.0),
            &self.body.clone(),
            self.palette.secondary,
        )
    }

    fn selection_toolbar(
        &mut self,
        model: &UiModel,
        left: f32,
        right: f32,
        top: f32,
    ) -> Result<(), String> {
        let bar = rect(left, top, right, top + 44.0);
        self.fill(bar, self.palette.surface_raised, RADIUS)?;
        self.stroke(bar, self.palette.border, RADIUS, 1.0)?;
        let selected = model.selected_clips.len();
        self.text(
            &self.strings.selected_count(selected),
            rect(bar.left + 16.0, bar.top, bar.left + 200.0, bar.bottom),
            &self.body.clone(),
            self.palette.secondary,
        )?;
        let move_button = rect(
            bar.right - 152.0,
            bar.top + 6.0,
            bar.right - 10.0,
            bar.bottom - 6.0,
        );
        if selected > 0 && !model.collections.is_empty() {
            self.action_button(
                move_button,
                &self.strings.move_button(selected),
                Action::ToggleCollectionPicker,
            )?;
        } else {
            self.fill(move_button, self.palette.surface, RADIUS_SMALL)?;
            self.text(
                &format!("VERSCHIEBEN ({selected})"),
                move_button,
                &self.button.clone(),
                self.palette.muted,
            )?;
        }
        self.pill(
            rect(
                move_button.left - 146.0,
                bar.top + 6.0,
                move_button.left - 10.0,
                bar.bottom - 6.0,
            ),
            self.palette.surface,
            self.strings.select_all,
            self.palette.primary,
            Some(Action::SelectAllVisibleClips),
        )?;
        self.pill(
            rect(
                move_button.left - 246.0,
                bar.top + 6.0,
                move_button.left - 156.0,
                bar.bottom - 6.0,
            ),
            self.palette.surface,
            self.strings.cancel,
            self.palette.secondary,
            Some(Action::ToggleSelectionMode),
        )
    }

    fn render_collection_picker(
        &mut self,
        model: &UiModel,
        right: f32,
        top: f32,
    ) -> Result<(), String> {
        let visible = model.collections.len().min(8);
        let width = 248.0;
        let row_height = 40.0;
        let area = rect(
            right - width,
            top,
            right,
            top + 48.0 + visible as f32 * row_height + 10.0,
        );
        self.fill(area, self.palette.surface_raised, RADIUS_LARGE)?;
        self.stroke(area, self.palette.border, RADIUS_LARGE, 1.0)?;
        self.text(
            self.strings.move_selected_to,
            rect(
                area.left + 14.0,
                area.top + 4.0,
                area.right - 14.0,
                area.top + 44.0,
            ),
            &self.small.clone(),
            self.palette.secondary,
        )?;
        for (index, collection) in model.collections.iter().take(visible).enumerate() {
            let action = Action::MoveSelectedToCollection(index);
            let row = rect(
                area.left + 8.0,
                area.top + 44.0 + index as f32 * row_height,
                area.right - 8.0,
                area.top + 44.0 + (index + 1) as f32 * row_height,
            );
            if self.is_hovered(&action) {
                self.fill(
                    row,
                    self.hover_fill(self.palette.surface, self.palette.surface_hover),
                    RADIUS_SMALL,
                )?;
            }
            self.text(
                &collection.name,
                rect(row.left + 10.0, row.top, row.right - 42.0, row.bottom),
                &self.body.clone(),
                self.palette.primary,
            )?;
            self.text(
                &collection.clip_count.to_string(),
                rect(row.right - 32.0, row.top, row.right - 8.0, row.bottom),
                &self.small.clone(),
                self.palette.secondary,
            )?;
            self.hits.push(HitRegion { rect: row, action });
        }
        Ok(())
    }

    fn render_text_input(
        &mut self,
        input: &TextInput,
        field: LogicalRect,
        placeholder: &str,
        focused: bool,
        target: TextInputTarget,
    ) -> Result<(), String> {
        let body = self.body.clone();
        if input.value.is_empty() {
            self.text(placeholder, field, &body, self.palette.secondary)?;
        } else {
            if focused {
                let (start, end) = input.selection();
                if end > start {
                    let before: String = input.value.chars().take(start).collect();
                    let selected: String =
                        input.value.chars().skip(start).take(end - start).collect();
                    let offset = self.measure(&before, &body);
                    let width = self.measure(&selected, &body);
                    self.fill(
                        rect(
                            field.left + offset,
                            field.top + 8.0,
                            (field.left + offset + width).min(field.right),
                            field.bottom - 8.0,
                        ),
                        self.palette.selection,
                        3.0,
                    )?;
                }
            }
            self.text(&input.value, field, &body, self.palette.primary)?;
        }

        if focused {
            let prefixes = (0..=input.characters())
                .map(|count| {
                    let prefix: String = input.value.chars().take(count).collect();
                    self.measure(&prefix, &body)
                })
                .collect::<Vec<_>>();
            let caret_x = (field.left + prefixes[input.caret]).min(field.right);
            self.fill(
                rect(caret_x, field.top + 8.0, caret_x + 1.5, field.bottom - 8.0),
                self.palette.primary,
                0.0,
            )?;
            for index in 0..=input.characters() {
                let left = if index == 0 {
                    field.left
                } else {
                    field.left + (prefixes[index - 1] + prefixes[index]) / 2.0
                };
                let right = if index == input.characters() {
                    field.right
                } else {
                    field.left + (prefixes[index] + prefixes[index + 1]) / 2.0
                };
                self.hits.push(HitRegion {
                    rect: rect(
                        left.min(field.right),
                        field.top,
                        right.min(field.right),
                        field.bottom,
                    ),
                    action: match target {
                        TextInputTarget::Search => Action::PlaceSearchCaret(index),
                        TextInputTarget::Prompt => Action::PlacePromptCaret(index),
                    },
                });
            }
        }
        Ok(())
    }

    fn render_settings_menu(
        &mut self,
        model: &UiModel,
        width: f32,
        height: f32,
    ) -> Result<(), String> {
        let Some(menu_state) = &model.settings_menu else {
            return Ok(());
        };
        if menu_state.items.is_empty() {
            return Ok(());
        }

        self.hits.push(HitRegion {
            rect: rect(0.0, 0.0, width, height),
            action: Action::DismissSettingsMenu,
        });

        let target_action = match menu_state.kind {
            SettingsMenuKind::Theme => Action::ChooseTheme,
            SettingsMenuKind::Language => Action::ChooseLanguage,
            SettingsMenuKind::HoverStyle => Action::ChooseHoverStyle,
            SettingsMenuKind::HoverStrength => Action::ChooseHoverStrength,
            SettingsMenuKind::TimeFilter => Action::ChooseTimeFilter,
            SettingsMenuKind::CollectionFilter => Action::ChooseCollectionFilter,
            SettingsMenuKind::TypeFilter => Action::ChooseTypeFilter,
            SettingsMenuKind::SizeFilter => Action::ChooseSizeFilter,
            SettingsMenuKind::ClipSort => Action::ChooseClipSort,
            SettingsMenuKind::Display => Action::ChooseDisplay,
            SettingsMenuKind::FrameRate => Action::ChooseFrameRate,
            SettingsMenuKind::Duration => Action::ChooseDuration,
            SettingsMenuKind::Codec => Action::ChooseCodec,
            SettingsMenuKind::Quality => Action::ChooseQuality,
            SettingsMenuKind::AudioMode => Action::ChooseAudioMode,
            SettingsMenuKind::DesktopDevice => Action::ChooseDesktopDevice,
            SettingsMenuKind::DesktopGain => Action::ChooseDesktopGain,
            SettingsMenuKind::Microphone => Action::ChooseMicrophone,
            SettingsMenuKind::MicrophoneGain => Action::ChooseMicrophoneGain,
            SettingsMenuKind::StorageLimit => Action::ChooseStorageLimit,
        };
        let anchor = self
            .hits
            .iter()
            .rev()
            .find(|hit| hit.action == target_action)
            .map_or(rect(width - 380.0, 150.0, width - 40.0, 190.0), |hit| {
                hit.rect
            });
        let control_width = (anchor.right - anchor.left).max(190.0);
        let columns = if menu_state.kind == SettingsMenuKind::DesktopGain {
            3
        } else {
            1
        };
        let has_details = menu_state.items.iter().any(|item| item.detail.is_some());
        let item_height = if has_details { 52.0 } else { 40.0 };
        let rows = menu_state.items.len().div_ceil(columns);
        let menu_height = 12.0 + rows as f32 * item_height;
        let menu_width = control_width.max(if has_details { 310.0 } else { 190.0 });
        let menu_left = (anchor.right - menu_width).max(18.0);
        let below = anchor.bottom + 8.0;
        let above = anchor.top - menu_height - 8.0;
        let menu_top = if below + menu_height <= height - 18.0 {
            below
        } else {
            above.max(18.0)
        };
        let menu = rect(
            menu_left,
            menu_top,
            menu_left + menu_width,
            menu_top + menu_height,
        );

        self.fill_alpha(
            rect(
                menu.left + 4.0,
                menu.top + 6.0,
                menu.right + 4.0,
                menu.bottom + 6.0,
            ),
            self.palette.canvas,
            0.48,
            RADIUS_LARGE,
        )?;
        self.fill(menu, self.palette.surface_raised, RADIUS_LARGE)?;
        self.stroke(menu, self.palette.border, RADIUS_LARGE, 1.0)?;
        self.stroke(
            anchor,
            mix(self.palette.border, self.palette.secondary, 0.7),
            RADIUS,
            1.0,
        )?;

        let cell_width = (menu_width - 12.0) / columns as f32;
        for (index, item) in menu_state.items.iter().enumerate() {
            let column = index % columns;
            let row = index / columns;
            let item_area = rect(
                menu.left + 6.0 + column as f32 * cell_width,
                menu.top + 6.0 + row as f32 * item_height,
                menu.left + 6.0 + (column + 1) as f32 * cell_width,
                menu.top + 6.0 + (row + 1) as f32 * item_height,
            );
            let action = Action::SelectSettingsOption(index);
            let selected = menu_state.selected == Some(index);
            let highlighted = menu_state.highlighted == index || self.is_hovered(&action);
            if selected || highlighted {
                self.fill(
                    item_area,
                    if selected {
                        self.palette.surface_hover
                    } else {
                        self.hover_fill(self.palette.surface, self.palette.surface_hover)
                    },
                    RADIUS_SMALL,
                )?;
            }
            if selected {
                if columns > 1 {
                    self.stroke(item_area, self.palette.accent, RADIUS_SMALL, 1.0)?;
                } else {
                    self.fill(
                        rect(
                            item_area.left + 6.0,
                            item_area.top + 11.0,
                            item_area.left + 9.0,
                            item_area.bottom - 11.0,
                        ),
                        self.palette.accent,
                        1.5,
                    )?;
                }
            }
            if columns > 1 {
                self.text(
                    &item.label,
                    item_area,
                    &self.body_center.clone(),
                    self.palette.primary,
                )?;
            } else if let Some(detail) = &item.detail {
                self.text(
                    &item.label,
                    rect(
                        item_area.left + 20.0,
                        item_area.top + 5.0,
                        item_area.right - 12.0,
                        item_area.top + 28.0,
                    ),
                    &self.body.clone(),
                    self.palette.primary,
                )?;
                self.text(
                    detail,
                    rect(
                        item_area.left + 20.0,
                        item_area.top + 27.0,
                        item_area.right - 12.0,
                        item_area.bottom - 3.0,
                    ),
                    &self.small.clone(),
                    self.palette.secondary,
                )?;
            } else {
                self.text(
                    &item.label,
                    rect(
                        item_area.left + 20.0,
                        item_area.top,
                        item_area.right - 12.0,
                        item_area.bottom,
                    ),
                    &self.body.clone(),
                    self.palette.primary,
                )?;
            }
            self.hits.push(HitRegion {
                rect: item_area,
                action,
            });
        }
        Ok(())
    }

    fn render_context_menu(
        &mut self,
        model: &UiModel,
        width: f32,
        height: f32,
    ) -> Result<(), String> {
        let Some(context) = model.context_menu else {
            return Ok(());
        };
        let Some(clip) = model.clips.get(context.clip) else {
            return Ok(());
        };
        self.hits.push(HitRegion {
            rect: rect(0.0, 0.0, width, height),
            action: Action::DismissContextMenu,
        });

        let visible_collections = model.collections.len().min(6);
        let collection_rows = if visible_collections == 0 {
            0
        } else {
            visible_collections + 1
        };
        let menu_width = 252.0;
        let menu_height = 66.0 + (6 + collection_rows) as f32 * 44.0 + 18.0;
        let left = context.x.min(width - menu_width - 16.0).max(16.0);
        let top = context.y.min(height - menu_height - 16.0).max(16.0);
        let menu = rect(left, top, left + menu_width, top + menu_height);
        self.fill(menu, self.palette.surface_raised, RADIUS_LARGE)?;
        self.stroke(menu, self.palette.border, RADIUS_LARGE, 1.0)?;
        self.text(
            self.strings.clip_actions,
            rect(left + 16.0, top + 12.0, menu.right - 16.0, top + 30.0),
            &self.small.clone(),
            self.palette.secondary,
        )?;
        self.text(
            &clip.title,
            rect(left + 16.0, top + 31.0, menu.right - 16.0, top + 58.0),
            &self.body.clone(),
            self.palette.primary,
        )?;

        let mut row_top = top + 66.0;
        for (label, action) in [
            (
                if model.is_favorite(context.clip) {
                    self.strings.favorite_remove
                } else {
                    self.strings.favorite_add
                },
                Action::ToggleFavorite(context.clip),
            ),
            (
                self.strings.open_in_explorer,
                Action::OpenClipExternally(context.clip),
            ),
            (self.strings.edit_clip, Action::EditClip(context.clip)),
            (self.strings.rename, Action::RenameClip(context.clip)),
            (self.strings.select_multiple, Action::ToggleSelectionMode),
        ] {
            self.context_menu_row(
                rect(left + 8.0, row_top, menu.right - 8.0, row_top + 40.0),
                label,
                action,
                false,
            )?;
            row_top += 44.0;
        }

        if visible_collections > 0 {
            self.text(
                self.strings.move_to_collection,
                rect(
                    left + 16.0,
                    row_top + 4.0,
                    menu.right - 16.0,
                    row_top + 28.0,
                ),
                &self.small.clone(),
                self.palette.secondary,
            )?;
            row_top += 44.0;
            for (collection, item) in model
                .collections
                .iter()
                .take(visible_collections)
                .enumerate()
            {
                self.context_menu_row(
                    rect(left + 8.0, row_top, menu.right - 8.0, row_top + 40.0),
                    &item.name,
                    Action::MoveClipToCollection {
                        clip: context.clip,
                        collection,
                    },
                    false,
                )?;
                row_top += 44.0;
            }
        }

        self.fill(
            rect(left + 16.0, row_top + 1.0, menu.right - 16.0, row_top + 2.0),
            self.palette.border,
            0.0,
        )?;
        row_top += 6.0;
        self.context_menu_row(
            rect(left + 8.0, row_top, menu.right - 8.0, row_top + 40.0),
            self.strings.delete_clip,
            Action::DeleteClip(context.clip),
            true,
        )
    }

    fn context_menu_row(
        &mut self,
        area: LogicalRect,
        label: &str,
        action: Action,
        dangerous: bool,
    ) -> Result<(), String> {
        if self.is_hovered(&action) {
            self.fill(
                area,
                if dangerous {
                    mix(self.palette.surface_hover, self.palette.destructive, 0.28)
                } else {
                    self.hover_fill(self.palette.surface, self.palette.surface_hover)
                },
                RADIUS,
            )?;
        }
        self.text(
            label,
            rect(area.left + 12.0, area.top, area.right - 12.0, area.bottom),
            &self.body.clone(),
            if dangerous {
                self.palette.destructive
            } else {
                self.palette.primary
            },
        )?;
        self.hits.push(HitRegion { rect: area, action });
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
        self.fill_alpha(overlay, self.palette.canvas, 0.82, 0.0)?;
        self.hits.push(HitRegion {
            rect: overlay,
            action: Action::CancelDelete,
        });
        let modal_width = 460.0_f32.min(width - 40.0);
        let modal_height = 224.0;
        let left = (width - modal_width) / 2.0;
        let top = (height - modal_height) / 2.0;
        let modal = rect(left, top, left + modal_width, top + modal_height);
        self.fill(modal, self.palette.surface_raised, RADIUS_LARGE)?;
        self.stroke(modal, self.palette.border, RADIUS_LARGE, 1.0)?;
        let (title, detail, confirmation) = match target {
            DeleteTarget::Clip(index) => {
                let name = model
                    .clips
                    .get(*index)
                    .map_or(self.strings.this_clip, |clip| clip.title.as_str());
                (
                    self.strings.delete_clip_question,
                    self.strings.delete_clip_body(name),
                    self.strings.delete_clip,
                )
            }
            DeleteTarget::Collection(path) => {
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(self.strings.this_collection);
                (
                    self.strings.delete_collection_question,
                    self.strings.delete_collection_body(name),
                    self.strings.delete,
                )
            }
        };
        self.text(
            title,
            rect(left + 28.0, top + 24.0, modal.right - 28.0, top + 58.0),
            &self.section.clone(),
            self.palette.primary,
        )?;
        self.text(
            &detail,
            rect(left + 28.0, top + 66.0, modal.right - 28.0, top + 108.0),
            &self.body.clone(),
            self.palette.secondary,
        )?;
        self.pill(
            rect(
                modal.right - 292.0,
                modal.bottom - 62.0,
                modal.right - 178.0,
                modal.bottom - 22.0,
            ),
            self.palette.surface_hover,
            self.strings.cancel,
            self.palette.primary,
            Some(Action::CancelDelete),
        )?;
        self.pill(
            rect(
                modal.right - 170.0,
                modal.bottom - 62.0,
                modal.right - 22.0,
                modal.bottom - 22.0,
            ),
            self.palette.destructive,
            confirmation,
            self.palette.canvas,
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
        self.fill_alpha(overlay, self.palette.canvas, 0.82, 0.0)?;
        self.hits.push(HitRegion {
            rect: overlay,
            action: Action::CancelPrompt,
        });
        let modal_width = 460.0_f32.min(width - 40.0);
        let modal_height = 232.0;
        let left = (width - modal_width) / 2.0;
        let top = (height - modal_height) / 2.0;
        let modal = rect(left, top, left + modal_width, top + modal_height);
        self.fill(modal, self.palette.surface_raised, RADIUS_LARGE)?;
        self.stroke(modal, self.palette.border, RADIUS_LARGE, 1.0)?;
        self.hits.push(HitRegion {
            rect: modal,
            action: Action::DismissNotice,
        });
        self.text(
            prompt.title(self.strings),
            rect(left + 28.0, top + 24.0, modal.right - 28.0, top + 58.0),
            &self.section.clone(),
            self.palette.primary,
        )?;
        self.text(
            prompt.label(self.strings),
            rect(left + 28.0, top + 64.0, modal.right - 28.0, top + 84.0),
            &self.small.clone(),
            self.palette.secondary,
        )?;
        let field = rect(left + 28.0, top + 90.0, modal.right - 28.0, top + 134.0);
        self.fill(field, self.palette.stage, RADIUS_LARGE)?;
        self.stroke(field, self.palette.accent, RADIUS_LARGE, 1.0)?;
        self.render_text_input(
            &prompt.input,
            rect(
                field.left + 14.0,
                field.top,
                field.right - 14.0,
                field.bottom,
            ),
            "",
            true,
            TextInputTarget::Prompt,
        )?;
        self.text(
            self.strings.prompt_hint,
            rect(left + 28.0, top + 142.0, modal.right - 28.0, top + 162.0),
            &self.small.clone(),
            self.palette.secondary,
        )?;
        self.pill(
            rect(
                modal.right - 292.0,
                modal.bottom - 62.0,
                modal.right - 178.0,
                modal.bottom - 22.0,
            ),
            self.palette.surface_hover,
            self.strings.cancel,
            self.palette.primary,
            Some(Action::CancelPrompt),
        )?;
        self.pill(
            rect(
                modal.right - 170.0,
                modal.bottom - 62.0,
                modal.right - 22.0,
                modal.bottom - 22.0,
            ),
            self.palette.accent,
            prompt.confirm(self.strings),
            self.palette.canvas,
            Some(Action::ConfirmPrompt),
        )
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
            self.palette.secondary,
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
        let hovered = action
            .as_ref()
            .is_some_and(|candidate| self.is_hovered(candidate));
        let is_accent = background == self.palette.accent;
        let rendered_background = if hovered {
            if is_accent {
                mix(
                    self.palette.accent,
                    self.palette.accent_hover,
                    self.hover_amount(1.0),
                )
            } else {
                self.hover_fill(background, self.palette.surface_hover)
            }
        } else {
            background
        };
        self.fill(area, rendered_background, RADIUS)?;
        if !is_accent {
            self.stroke(area, self.palette.border, RADIUS, 1.0)?;
        }
        self.text(label, area, &self.body_center.clone(), foreground)?;
        if let Some(action) = action {
            self.hits.push(HitRegion { rect: area, action });
        }
        Ok(())
    }

    fn floating_icon(
        &mut self,
        area: LogicalRect,
        label: &str,
        foreground: u32,
        action: Option<Action>,
        size: FloatingIconSize,
    ) -> Result<(), String> {
        let hovered = action
            .as_ref()
            .is_some_and(|candidate| self.is_hovered(candidate));
        let progress = if hovered { self.hover_amount(1.0) } else { 0.0 };
        let lift = progress * 2.5;
        let label_area = rect(area.left, area.top - lift, area.right, area.bottom - lift);
        let color = mix(foreground, self.palette.primary, progress * 0.72);
        let format = match size {
            FloatingIconSize::Media => self.media_icon.clone(),
            FloatingIconSize::Navigation => self.navigation_icon.clone(),
            FloatingIconSize::Fullscreen => self.fullscreen_icon.clone(),
        };
        self.text(label, label_area, &format, color)?;
        if let Some(action) = action {
            self.hits.push(HitRegion { rect: area, action });
        }
        Ok(())
    }

    fn floating_glyph(
        &mut self,
        area: LogicalRect,
        glyph: Glyph,
        foreground: u32,
        action: Option<Action>,
    ) -> Result<(), String> {
        let hovered = action
            .as_ref()
            .is_some_and(|candidate| self.is_hovered(candidate));
        let color = if hovered {
            mix(foreground, self.palette.primary, self.hover_amount(0.72))
        } else {
            foreground
        };
        let size = 20.0;
        let center_x = (area.left + area.right) / 2.0;
        let center_y = (area.top + area.bottom) / 2.0;
        self.glyph(
            glyph,
            rect(
                center_x - size / 2.0,
                center_y - size / 2.0,
                center_x + size / 2.0,
                center_y + size / 2.0,
            ),
            color,
        )?;
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

    fn stroke(
        &self,
        area: LogicalRect,
        stroke_color: u32,
        radius: f32,
        width: f32,
    ) -> Result<(), String> {
        use windows::Win32::Graphics::Direct2D::ID2D1StrokeStyle;

        let target = self.target.as_ref().expect("render target exists");
        let brush = unsafe { target.CreateSolidColorBrush(&color(stroke_color), None) }
            .map_err(|error| error.to_string())?;
        unsafe {
            if radius > 0.0 {
                target.DrawRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: area.d2d(),
                        radiusX: radius,
                        radiusY: radius,
                    },
                    &brush,
                    width,
                    None::<&ID2D1StrokeStyle>,
                );
            } else {
                target.DrawRectangle(&area.d2d(), &brush, width, None::<&ID2D1StrokeStyle>);
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
        let target = self.target.as_ref().expect("render target exists");
        let brush = unsafe { target.CreateSolidColorBrush(&color(fill), None) }
            .map_err(|error| error.to_string())?;
        // every icon is laid out on a centred square so a non-square area cannot
        // stretch it out of shape
        let span = (area.right - area.left).min(area.bottom - area.top);
        let center_x = (area.left + area.right) / 2.0;
        let center_y = (area.top + area.bottom) / 2.0;
        let scale = span / 24.0;
        let weight = (span * 0.088).clamp(1.4, 2.1);
        let unit = |x: f32, y: f32| Vector2 {
            X: center_x + (x - 12.0) * scale,
            Y: center_y + (y - 12.0) * scale,
        };
        let path = |points: &[(f32, f32)], closed: bool| -> Result<(), String> {
            let mapped = points.iter().map(|(x, y)| unit(*x, *y)).collect::<Vec<_>>();
            self.stroke_path(&mapped, closed, &brush, weight)
        };
        let solid = |points: &[(f32, f32)]| -> Result<(), String> {
            let mapped = points.iter().map(|(x, y)| unit(*x, *y)).collect::<Vec<_>>();
            self.fill_path(&mapped, &brush)
        };
        let circle = |x: f32, y: f32, radius: f32| -> Result<(), String> {
            unsafe {
                target.DrawEllipse(
                    &D2D1_ELLIPSE {
                        point: unit(x, y),
                        radiusX: radius * scale,
                        radiusY: radius * scale,
                    },
                    &brush,
                    weight,
                    Some(&self.round_stroke),
                );
            }
            Ok(())
        };
        let dot = |x: f32, y: f32, radius: f32| -> Result<(), String> {
            unsafe {
                target.FillEllipse(
                    &D2D1_ELLIPSE {
                        point: unit(x, y),
                        radiusX: radius * scale,
                        radiusY: radius * scale,
                    },
                    &brush,
                );
            }
            Ok(())
        };
        let rounded =
            |left: f32, top: f32, right: f32, bottom: f32, radius: f32| -> Result<(), String> {
                let start = unit(left, top);
                let end = unit(right, bottom);
                unsafe {
                    target.DrawRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F {
                                left: start.X,
                                top: start.Y,
                                right: end.X,
                                bottom: end.Y,
                            },
                            radiusX: radius * scale,
                            radiusY: radius * scale,
                        },
                        &brush,
                        weight,
                        Some(&self.round_stroke),
                    );
                }
                Ok(())
            };
        let bar = |left: f32, top: f32, right: f32, bottom: f32| -> Result<(), String> {
            let start = unit(left, top);
            let end = unit(right, bottom);
            let radius = (end.X - start.X) / 2.0;
            unsafe {
                target.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F {
                            left: start.X,
                            top: start.Y,
                            right: end.X,
                            bottom: end.Y,
                        },
                        radiusX: radius,
                        radiusY: radius,
                    },
                    &brush,
                );
            }
            Ok(())
        };
        let arc = |from: (f32, f32), to: (f32, f32), radius: f32| -> Result<(), String> {
            self.stroke_arc(
                unit(from.0, from.1),
                unit(to.0, to.1),
                radius * scale,
                &brush,
                weight,
            )
        };

        match glyph {
            Glyph::Library => {
                rounded(3.2, 5.2, 20.8, 18.8, 3.2)?;
                solid(&[(10.2, 8.8), (15.6, 12.0), (10.2, 15.2)])?;
            }
            Glyph::Collections => {
                rounded(3.4, 8.6, 20.6, 20.0, 2.8)?;
                path(&[(6.4, 5.8), (17.6, 5.8)], false)?;
                path(&[(8.8, 3.2), (15.2, 3.2)], false)?;
            }
            Glyph::Settings => {
                for (y, knob) in [(6.6, 9.0), (12.0, 15.0), (17.4, 7.4)] {
                    path(&[(4.4, y), (19.6, y)], false)?;
                    dot(knob, y, 2.0)?;
                }
            }
            Glyph::Folder => {
                path(
                    &[
                        (3.6, 18.9),
                        (3.6, 6.9),
                        (9.3, 6.9),
                        (11.3, 9.4),
                        (20.4, 9.4),
                        (20.4, 18.9),
                    ],
                    true,
                )?;
            }
            Glyph::Search => {
                circle(10.4, 10.4, 6.4)?;
                path(&[(15.2, 15.2), (20.4, 20.4)], false)?;
            }
            Glyph::Grid => {
                rounded(4.0, 4.0, 10.9, 10.9, 1.8)?;
                rounded(13.1, 4.0, 20.0, 10.9, 1.8)?;
                rounded(4.0, 13.1, 10.9, 20.0, 1.8)?;
                rounded(13.1, 13.1, 20.0, 20.0, 1.8)?;
            }
            Glyph::List => {
                for y in [6.6, 12.0, 17.4] {
                    path(&[(4.6, y), (19.4, y)], false)?;
                }
            }
            Glyph::More => {
                for x in [6.2, 12.0, 17.8] {
                    dot(x, 12.0, 1.35)?;
                }
            }
            Glyph::Clock => {
                circle(12.0, 12.0, 8.2)?;
                path(&[(12.0, 7.4), (12.0, 12.0), (15.6, 14.1)], false)?;
            }
            Glyph::Monitor => {
                rounded(3.2, 4.6, 20.8, 16.4, 2.6)?;
                path(&[(12.0, 16.4), (12.0, 19.6)], false)?;
                path(&[(8.6, 19.8), (15.4, 19.8)], false)?;
            }
            Glyph::Audio => {
                path(
                    &[
                        (3.6, 9.6),
                        (7.4, 9.6),
                        (11.8, 5.4),
                        (11.8, 18.6),
                        (7.4, 14.4),
                        (3.6, 14.4),
                    ],
                    true,
                )?;
                arc((15.0, 9.6), (15.0, 14.4), 2.6)?;
                arc((17.6, 7.2), (17.6, 16.8), 5.2)?;
            }
            Glyph::Quality => {
                for (x, top) in [(5.6, 16.4), (10.0, 12.8), (14.4, 9.2), (18.8, 5.6)] {
                    path(&[(x, 18.6), (x, top)], false)?;
                }
            }
            Glyph::Filter => {
                path(&[(4.4, 7.0), (19.6, 7.0)], false)?;
                path(&[(7.2, 12.0), (16.8, 12.0)], false)?;
                path(&[(10.0, 17.0), (14.0, 17.0)], false)?;
            }
            Glyph::Microphone => {
                bar(10.1, 3.4, 13.9, 12.6)?;
                arc((7.2, 10.6), (16.8, 10.6), 4.8)?;
                path(&[(12.0, 16.6), (12.0, 20.4)], false)?;
            }
            Glyph::Star | Glyph::StarFilled => {
                let corners = (0..10)
                    .map(|step| {
                        let radius = if step % 2 == 0 { 8.8 } else { 4.0 };
                        let angle =
                            -std::f32::consts::FRAC_PI_2 + step as f32 * std::f32::consts::PI / 5.0;
                        (12.0 + radius * angle.cos(), 12.4 + radius * angle.sin())
                    })
                    .collect::<Vec<_>>();
                if matches!(glyph, Glyph::StarFilled) {
                    solid(&corners)?;
                } else {
                    path(&corners, true)?;
                }
            }
            Glyph::External => {
                path(
                    &[
                        (10.4, 5.4),
                        (4.6, 5.4),
                        (4.6, 19.4),
                        (18.6, 19.4),
                        (18.6, 13.6),
                    ],
                    false,
                )?;
                path(&[(13.4, 4.6), (19.4, 4.6), (19.4, 10.6)], false)?;
                path(&[(19.4, 4.6), (12.2, 11.8)], false)?;
            }
            Glyph::Play => {
                path(&[(8.6, 5.4), (18.4, 12.0), (8.6, 18.6)], true)?;
            }
            Glyph::Pause => {
                bar(8.2, 5.4, 10.8, 18.6)?;
                bar(13.2, 5.4, 15.8, 18.6)?;
            }
            Glyph::ChevronLeft => {
                path(&[(14.6, 5.6), (8.6, 12.0), (14.6, 18.4)], false)?;
            }
            Glyph::ChevronRight => {
                path(&[(9.4, 5.6), (15.4, 12.0), (9.4, 18.4)], false)?;
            }
            Glyph::ChevronDown => {
                path(&[(5.6, 9.4), (12.0, 15.4), (18.4, 9.4)], false)?;
            }
            Glyph::Close => {
                path(&[(7.8, 7.8), (16.2, 16.2)], false)?;
                path(&[(16.2, 7.8), (7.8, 16.2)], false)?;
            }
            Glyph::Pencil => {
                path(
                    &[
                        (5.2, 18.8),
                        (6.8, 14.2),
                        (16.4, 4.6),
                        (19.4, 7.6),
                        (9.8, 17.2),
                    ],
                    true,
                )?;
                path(&[(6.8, 14.2), (9.8, 17.2)], false)?;
            }
            Glyph::Fullscreen => {
                path(&[(4.4, 9.8), (4.4, 4.4), (9.8, 4.4)], false)?;
                path(&[(14.2, 4.4), (19.6, 4.4), (19.6, 9.8)], false)?;
                path(&[(19.6, 14.2), (19.6, 19.6), (14.2, 19.6)], false)?;
                path(&[(9.8, 19.6), (4.4, 19.6), (4.4, 14.2)], false)?;
            }
        }
        Ok(())
    }

    fn stroke_path(
        &self,
        points: &[Vector2],
        closed: bool,
        brush: &ID2D1SolidColorBrush,
        weight: f32,
    ) -> Result<(), String> {
        if points.len() < 2 {
            return Ok(());
        }
        let geometry = self.build_path(points, closed, D2D1_FIGURE_BEGIN_HOLLOW)?;
        let target = self.target.as_ref().expect("render target exists");
        unsafe { target.DrawGeometry(&geometry, brush, weight, Some(&self.round_stroke)) };
        Ok(())
    }

    fn fill_path(&self, points: &[Vector2], brush: &ID2D1SolidColorBrush) -> Result<(), String> {
        if points.len() < 3 {
            return Ok(());
        }
        let geometry = self.build_path(points, true, D2D1_FIGURE_BEGIN_FILLED)?;
        let target = self.target.as_ref().expect("render target exists");
        unsafe { target.FillGeometry(&geometry, brush, None) };
        Ok(())
    }

    fn build_path(
        &self,
        points: &[Vector2],
        closed: bool,
        begin: D2D1_FIGURE_BEGIN,
    ) -> Result<ID2D1PathGeometry, String> {
        let geometry =
            unsafe { self.d2d_factory.CreatePathGeometry() }.map_err(|error| error.to_string())?;
        let sink = unsafe { geometry.Open() }.map_err(|error| error.to_string())?;
        unsafe {
            sink.BeginFigure(points[0], begin);
            sink.AddLines(&points[1..]);
            sink.EndFigure(if closed {
                D2D1_FIGURE_END_CLOSED
            } else {
                D2D1_FIGURE_END_OPEN
            });
        }
        unsafe { sink.Close() }.map_err(|error| error.to_string())?;
        Ok(geometry)
    }

    fn stroke_arc(
        &self,
        from: Vector2,
        to: Vector2,
        radius: f32,
        brush: &ID2D1SolidColorBrush,
        weight: f32,
    ) -> Result<(), String> {
        let geometry =
            unsafe { self.d2d_factory.CreatePathGeometry() }.map_err(|error| error.to_string())?;
        let sink = unsafe { geometry.Open() }.map_err(|error| error.to_string())?;
        unsafe {
            sink.BeginFigure(from, D2D1_FIGURE_BEGIN_HOLLOW);
            sink.AddArc(&D2D1_ARC_SEGMENT {
                point: to,
                size: D2D_SIZE_F {
                    width: radius,
                    height: radius,
                },
                rotationAngle: 0.0,
                sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                arcSize: D2D1_ARC_SIZE_SMALL,
            });
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
        }
        unsafe { sink.Close() }.map_err(|error| error.to_string())?;
        let target = self.target.as_ref().expect("render target exists");
        unsafe { target.DrawGeometry(&geometry, brush, weight, Some(&self.round_stroke)) };
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
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
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
        let source = unsafe { bitmap.GetSize() };
        if source.width <= 0.0 || source.height <= 0.0 {
            return Ok(false);
        }
        let properties = D2D1_BITMAP_BRUSH_PROPERTIES {
            extendModeX: D2D1_EXTEND_MODE_CLAMP,
            extendModeY: D2D1_EXTEND_MODE_CLAMP,
            interpolationMode: D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
        };
        let brush = unsafe { target.CreateBitmapBrush(bitmap, Some(&properties), None) }
            .map_err(|error| error.to_string())?;
        let transform = Matrix3x2 {
            M11: (destination.right - destination.left) / source.width,
            M12: 0.0,
            M21: 0.0,
            M22: (destination.bottom - destination.top) / source.height,
            M31: destination.left,
            M32: destination.top,
        };
        unsafe {
            brush.SetTransform(&transform);
            target.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: destination.d2d(),
                    radiusX: RADIUS_SMALL,
                    radiusY: RADIUS_SMALL,
                },
                &brush,
            );
        }
        Ok(true)
    }

    fn clip_duration(&mut self, path: &Path) -> Option<u64> {
        if let Some(duration) = self.clip_durations.get(path) {
            return *duration;
        }
        let duration = shell_clip_duration(path);
        self.clip_durations.insert(path.to_path_buf(), duration);
        duration
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
    family: PCWSTR,
    size: f32,
    semibold: bool,
    centered: bool,
) -> Result<IDWriteTextFormat, String> {
    let format = unsafe {
        factory.CreateTextFormat(
            family,
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

struct LibraryLayout {
    columns: usize,
    card_width: f32,
    card_height: f32,
    row_pitch: f32,
    sections: Vec<f32>,
    height: f32,
}

fn hover_blend_amount(progress: f32, strength: f32, weight: f32) -> f32 {
    (progress * strength * weight).clamp(0.0, 1.0)
}

fn clip_columns(width: f32) -> usize {
    if width >= 1_460.0 {
        5
    } else if width >= 1_080.0 {
        4
    } else if width >= 700.0 {
        3
    } else if width >= 480.0 {
        2
    } else {
        1
    }
}

fn library_layout(counts: &[usize], width: f32, grid: bool) -> LibraryLayout {
    let columns = if grid { clip_columns(width) } else { 1 };
    let card_width = if grid {
        ((width - CLIP_COLUMN_GAP * (columns - 1) as f32) / columns as f32).max(120.0)
    } else {
        width.max(240.0)
    };
    let card_height = if grid {
        (card_width * 9.0 / 16.0).round() + CLIP_META_HEIGHT
    } else {
        CLIP_LIST_ROW_HEIGHT
    };
    let row_pitch = if grid {
        card_height + CLIP_ROW_GAP
    } else {
        CLIP_LIST_ROW_HEIGHT
    };
    let mut sections = Vec::with_capacity(counts.len());
    let mut offset = 0.0;
    for count in counts {
        sections.push(offset);
        let rows = count.div_ceil(columns).max(1);
        offset +=
            CLIP_SECTION_HEADER + (rows - 1) as f32 * row_pitch + card_height + CLIP_GROUP_GAP;
    }
    LibraryLayout {
        columns,
        card_width,
        card_height,
        row_pitch,
        sections,
        height: (offset - CLIP_GROUP_GAP).max(0.0),
    }
}

pub fn sidebar_width(collapsed: bool) -> f32 {
    if collapsed {
        SIDEBAR_COLLAPSED_WIDTH
    } else {
        SIDEBAR_WIDTH
    }
}

fn page_has_chrome(page: Page) -> bool {
    matches!(page, Page::Library | Page::Collections | Page::Settings)
}

fn content_top() -> f32 {
    TOOLBAR_TOP + TOOLBAR_HEIGHT + 32.0
}

fn content_bottom(height: f32, chrome: bool) -> f32 {
    if chrome {
        height - STATUS_BAR_HEIGHT - 10.0
    } else {
        height
    }
}

/// Height the folder rows need beyond the column, for the wheel handler.
fn folder_column_overflow_in(rows: LogicalRect, collections: usize) -> f32 {
    let content = (collections + 1) as f32 * (FOLDER_ROW_HEIGHT + 2.0);
    (content - (rows.bottom - rows.top)).max(0.0)
}

pub fn folder_column_overflow(model: &UiModel, width: f32, height: f32) -> f32 {
    if model.page != Page::Collections {
        return 0.0;
    }
    let left = sidebar_width(model.sidebar_collapsed) + CONTENT_PADDING;
    let rows = rect(
        left,
        content_top() + 100.0 + 26.0,
        (left + FOLDER_COLUMN_WIDTH).min(width),
        content_bottom(height, true),
    );
    folder_column_overflow_in(rows, model.collections.len())
}

/// True while the pointer sits over the collections folder column.
pub fn folder_column_contains(model: &UiModel, x: f32) -> bool {
    model.page == Page::Collections
        && x < sidebar_width(model.sidebar_collapsed) + CONTENT_PADDING + FOLDER_COLUMN_WIDTH
}

pub fn clips_overflow(model: &UiModel, width: f32, height: f32) -> f32 {
    let collections = match model.page {
        Page::Library => false,
        Page::Collections => true,
        _ => return 0.0,
    };
    let today = crate::clock::now();
    let indices = model.visible_clip_indices_at(usize::MAX, today);
    if indices.is_empty() {
        return 0.0;
    }
    let counts = model
        .clip_day_groups(&indices, today)
        .iter()
        .map(|group| group.indices.len())
        .collect::<Vec<_>>();
    let mut left = sidebar_width(model.sidebar_collapsed) + CONTENT_PADDING;
    let mut top = content_top() + 100.0;
    if collections {
        left += FOLDER_COLUMN_WIDTH + 12.0 + FOLDER_COLUMN_GAP;
        top += 34.0;
    }
    let area_width = width - CONTENT_PADDING - left - CLIP_SCROLL_RESERVE;
    let layout = library_layout(&counts, area_width, model.library_grid);
    let selecting = model.selection_mode && !model.selected_clips.is_empty();
    let mut bottom = content_bottom(height, true);
    if selecting {
        bottom -= 60.0;
    }
    (layout.height - (bottom - top).max(0.0)).max(0.0)
}

fn section_widths(available: f32, preferred: &[f32], minimum: f32) -> Vec<f32> {
    const MAX_GROWTH: f32 = 1.45;

    let mut widths = preferred.to_vec();
    loop {
        let total = widths.iter().sum::<f32>();
        if total <= available {
            let growth = (available / total.max(1.0)).min(MAX_GROWTH);
            return widths.iter().map(|width| width * growth).collect();
        }
        let scale = available / total;
        if widths.iter().all(|width| width * scale >= minimum) {
            return widths.iter().map(|width| width * scale).collect();
        }
        if widths.len() == 1 {
            return vec![available.max(0.0)];
        }
        widths.pop();
    }
}

fn text_format_trailing(
    factory: &IDWriteFactory,
    family: PCWSTR,
    size: f32,
) -> Result<IDWriteTextFormat, String> {
    let format = text_format(factory, family, size, false, false)?;
    unsafe { format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_TRAILING) }
        .map_err(|error| error.to_string())?;
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

fn shell_clip_duration(path: &Path) -> Option<u64> {
    const PKEY_MEDIA_DURATION: PROPERTYKEY = PROPERTYKEY {
        fmtid: GUID::from_u128(0x64440490_4c8b_11d1_8b70_080036b11a03),
        pid: 3,
    };
    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let store: IPropertyStore = unsafe {
        SHGetPropertyStoreFromParsingName(PCWSTR(wide.as_ptr()), None::<&IBindCtx>, GPS_DEFAULT)
    }
    .ok()?;
    let value = unsafe { store.GetValue(&PKEY_MEDIA_DURATION) }.ok()?;
    let hundred_nanoseconds = u64::try_from(&value).ok()?;
    (hundred_nanoseconds > 0).then(|| ((hundred_nanoseconds + 5_000_000) / 10_000_000).max(1))
}

fn format_clip_badge_duration(total_seconds: u64) -> String {
    let hours = total_seconds / 3_600;
    let minutes = total_seconds % 3_600 / 60;
    let seconds = total_seconds % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

/// A panel divides its own height by the rows it carries, so a long group stays
/// inside its box instead of running into the one below.
fn compact_settings_row_height(panel: LogicalRect, rows: usize) -> f32 {
    ((panel.bottom - panel.top - SETTINGS_PANEL_HEADER) / rows.max(1) as f32).clamp(30.0, 52.0)
}

fn settings_panel_rects(left: f32, right: f32, height: f32) -> [LogicalRect; 4] {
    let gap = 14.0;
    let middle = (left + right) / 2.0;
    let top = 144.0;
    let panels_bottom = if height >= 920.0 {
        height - 133.0
    } else {
        height - 22.0
    };
    let panel_space = (panels_bottom - top - gap).max(420.0);
    let top_height = (panel_space / 2.0).max(180.0);
    let bottom_top = top + top_height + gap;
    [
        rect(left, top, middle - gap / 2.0, top + top_height),
        rect(middle + gap / 2.0, top, right, top + top_height),
        rect(left, bottom_top, middle - gap / 2.0, panels_bottom),
        rect(middle + gap / 2.0, bottom_top, right, panels_bottom),
    ]
}

fn settings_control_area(panel: LogicalRect, rows: usize, index: usize) -> LogicalRect {
    let row_height = compact_settings_row_height(panel, rows);
    let top = panel.top + SETTINGS_PANEL_HEADER + index as f32 * row_height;
    let control_width = ((panel.right - panel.left) * 0.34).clamp(128.0, 205.0);
    rect(
        panel.right - control_width - 20.0,
        top + 6.0,
        panel.right - 20.0,
        top + row_height - 6.0,
    )
}

fn settings_gain_rail_in_panel(panel: LogicalRect, rows: usize, index: usize) -> LogicalRect {
    let control = settings_control_area(panel, rows, index);
    let center = (control.top + control.bottom) / 2.0;
    rect(
        control.left + 5.0,
        center - 2.0,
        control.right - 48.0,
        center + 2.0,
    )
}

fn mix(from: u32, to: u32, amount: f32) -> u32 {
    let amount = amount.clamp(0.0, 1.0);
    let channel = |shift: u32| {
        let from = ((from >> shift) & 0xff) as f32;
        let to = ((to >> shift) & 0xff) as f32;
        from.mul_add(1.0 - amount, to * amount).round() as u32
    };
    (channel(16) << 16) | (channel(8) << 8) | channel(0)
}

fn color(rgb: u32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: ((rgb >> 16) & 0xff) as f32 / 255.0,
        g: ((rgb >> 8) & 0xff) as f32 / 255.0,
        b: (rgb & 0xff) as f32 / 255.0,
        a: 1.0,
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
    if megabytes >= 1_048_576 && megabytes % 1_048_576 == 0 {
        format!("{} TB", megabytes / 1_048_576)
    } else if megabytes >= 1_024 && megabytes % 1_024 == 0 {
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

fn hotkey_capture_label(modifiers: &[String], text: &Strings) -> String {
    let modifiers = modifiers
        .iter()
        .map(|modifier| match modifier.as_str() {
            "SUPER" => "Win",
            "CTRL" => "Ctrl",
            "ALT" => "Alt",
            "SHIFT" => "Shift",
            value => value,
        })
        .collect::<Vec<_>>();
    if modifiers.is_empty() {
        text.hotkey_prompt.to_owned()
    } else {
        format!("{} + …", modifiers.join(" + "))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CLIP_GROUP_GAP, CLIP_SCROLL_RESERVE, CLIP_SECTION_HEADER, CONTENT_PADDING,
        FOLDER_ROW_HEIGHT, Page, Palette, SIDEBAR_COLLAPSED_WIDTH, SIDEBAR_WIDTH, TOOLBAR_HEIGHT,
        TOOLBAR_TOP, Theme, clip_columns, content_bottom, content_top, folder_column_overflow_in,
        format_bytes, format_storage_limit, hover_blend_amount, library_layout, page_has_chrome,
        palette_for, rect, section_widths, settings_gain_percent, sidebar_width,
    };

    fn luminance(color: u32) -> f32 {
        let channel = |shift: u32| ((color >> shift) & 0xff) as f32 / 255.0;
        0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
    }

    fn contrast(palette: Palette) -> f32 {
        (luminance(palette.primary) - luminance(palette.canvas)).abs()
    }

    #[test]
    fn every_theme_keeps_text_readable_and_the_thumbnail_bed_quiet() {
        for theme in Theme::OPTIONS {
            let palette = palette_for(theme);
            assert!(
                contrast(palette) > 0.6,
                "{theme:?} does not separate text from the canvas"
            );
            assert!(
                (luminance(palette.stage) - luminance(palette.canvas)).abs() < 0.1,
                "{theme:?} makes the thumbnail bed shout"
            );
            assert_ne!(palette.accent, palette.accent_text);
        }

        assert!(luminance(palette_for(Theme::Light).canvas) > 0.7);
        assert!(luminance(palette_for(Theme::Dark).canvas) < 0.1);
    }

    #[test]
    fn only_the_cafe_theme_spends_an_accent_on_live_indicators() {
        assert_eq!(
            palette_for(Theme::Dark).live,
            palette_for(Theme::Dark).primary
        );
        assert_eq!(
            palette_for(Theme::Light).live,
            palette_for(Theme::Light).primary
        );
        assert_ne!(
            palette_for(Theme::Cafe).live,
            palette_for(Theme::Cafe).primary
        );
    }

    #[test]
    fn hover_strength_scales_the_blend_and_can_switch_it_off() {
        assert_eq!(hover_blend_amount(1.0, 0.0, 1.0), 0.0);
        assert_eq!(hover_blend_amount(1.0, 0.55, 1.0), 0.55);
        assert_eq!(hover_blend_amount(0.5, 1.0, 1.0), 0.5);
        assert_eq!(hover_blend_amount(1.0, 1.6, 1.0), 1.0);
        assert_eq!(hover_blend_amount(1.0, 1.0, 0.72), 0.72);
    }

    #[test]
    fn storage_sizes_use_mb_gb_and_tb_labels() {
        assert_eq!(format_bytes(512 * 1_024), "0.5 MB");
        assert_eq!(format_bytes(20 * 1_048_576), "20.0 MB");
        assert_eq!(format_bytes(5 * 1_073_741_824), "5.0 GB");
        assert_eq!(format_storage_limit(512), "512 MB");
        assert_eq!(format_storage_limit(10_240), "10 GB");
        assert_eq!(format_storage_limit(1_048_576), "1 TB");
    }

    #[test]
    fn the_toolbar_and_status_bar_frame_the_application_pages() {
        assert!(page_has_chrome(Page::Library));
        assert!(page_has_chrome(Page::Collections));
        assert!(page_has_chrome(Page::Settings));
        assert!(!page_has_chrome(Page::Player));
        assert!(!page_has_chrome(Page::Editor));

        assert_eq!(content_top(), TOOLBAR_TOP + TOOLBAR_HEIGHT + 32.0);
        assert!(content_top() - (TOOLBAR_TOP + TOOLBAR_HEIGHT) >= 30.0);
        assert_eq!(content_bottom(900.0, false), 900.0);
        assert!(900.0 - content_bottom(900.0, true) >= 84.0);
    }

    #[test]
    fn the_clips_grid_fills_the_window_without_oversized_cards() {
        let clips_width = |window: f32, collapsed: bool| {
            window - sidebar_width(collapsed) - CONTENT_PADDING * 2.0 - CLIP_SCROLL_RESERVE
        };

        assert_eq!(clip_columns(clips_width(1_440.0, false)), 4);
        assert_eq!(clip_columns(clips_width(1_920.0, false)), 5);
        assert_eq!(clip_columns(clips_width(1_600.0, true)), 5);
        assert_eq!(clip_columns(clips_width(1_100.0, false)), 3);
        assert_eq!(clip_columns(500.0), 2);
        assert_eq!(clip_columns(320.0), 1);

        assert_eq!(sidebar_width(false), SIDEBAR_WIDTH);
        assert_eq!(sidebar_width(true), SIDEBAR_COLLAPSED_WIDTH);
    }

    #[test]
    fn day_sections_stack_without_overlapping_their_rows() {
        let layout = library_layout(&[12, 8], 1_200.0, true);

        assert_eq!(layout.columns, 4);
        assert_eq!(layout.row_pitch, layout.card_height + 18.0);
        let first_section =
            CLIP_SECTION_HEADER + 2.0 * layout.row_pitch + layout.card_height + CLIP_GROUP_GAP;
        assert_eq!(layout.sections, vec![0.0, first_section]);
        assert_eq!(
            layout.height,
            first_section + CLIP_SECTION_HEADER + layout.row_pitch + layout.card_height
        );

        let single = library_layout(&[1], 1_200.0, true);
        assert_eq!(single.sections, vec![0.0]);
        assert_eq!(single.height, CLIP_SECTION_HEADER + single.card_height);
    }

    #[test]
    fn the_clips_list_lays_out_one_row_per_clip() {
        let layout = library_layout(&[3], 900.0, false);

        assert_eq!(layout.columns, 1);
        assert_eq!(layout.card_width, 900.0);
        assert_eq!(
            layout.height,
            CLIP_SECTION_HEADER + 3.0 * layout.card_height
        );
    }

    #[test]
    fn the_folder_column_scrolls_once_its_rows_pass_the_bottom() {
        let rows = rect(0.0, 0.0, 200.0, 200.0);

        assert_eq!(folder_column_overflow_in(rows, 2), 0.0);
        let overflow = folder_column_overflow_in(rows, 20);
        assert!(overflow > 0.0);
        assert_eq!(overflow, 21.0 * (FOLDER_ROW_HEIGHT + 2.0) - 200.0);
    }

    #[test]
    fn toolbar_sections_fill_the_bar_and_shrink_before_one_is_dropped() {
        let preferred = [150.0, 168.0, 168.0, 180.0];

        let spread = section_widths(900.0, &preferred, 118.0);
        assert_eq!(spread.len(), 4);
        assert!((spread.iter().sum::<f32>() - 900.0).abs() < 0.01);
        assert!(spread[0] > preferred[0]);

        let capped = section_widths(4_000.0, &preferred, 118.0);
        assert!((capped.iter().sum::<f32>() - 666.0 * 1.45).abs() < 0.01);

        let scaled = section_widths(600.0, &preferred, 118.0);
        assert_eq!(scaled.len(), 4);
        assert!((scaled.iter().sum::<f32>() - 600.0).abs() < 0.01);

        let dropped = section_widths(400.0, &preferred, 118.0);
        assert_eq!(dropped.len(), 3);
        assert!(dropped.iter().all(|width| *width >= 118.0));

        assert_eq!(section_widths(120.0, &preferred, 118.0), vec![120.0]);
    }

    #[test]
    fn audio_gain_slider_maps_its_full_width_to_zero_through_two_hundred_percent() {
        let rail = rect(100.0, 20.0, 300.0, 24.0);

        assert_eq!(settings_gain_percent(rail, 50.0), 0);
        assert_eq!(settings_gain_percent(rail, 100.0), 0);
        assert_eq!(settings_gain_percent(rail, 200.0), 100);
        assert_eq!(settings_gain_percent(rail, 300.0), 200);
        assert_eq!(settings_gain_percent(rail, 350.0), 200);
    }
}
