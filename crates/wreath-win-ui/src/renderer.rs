use std::collections::{HashMap, HashSet, VecDeque};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use windows::Win32::Foundation::{HWND, PROPERTYKEY};
use windows::Win32::Graphics::Direct2D::Common::{D2D_RECT_F, D2D_SIZE_U, D2D1_COLOR_F};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_BRUSH_PROPERTIES, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
    D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_ELLIPSE, D2D1_EXTEND_MODE_CLAMP,
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
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory, IWICPalette,
    WICBitmapDitherTypeNone, WICBitmapIgnoreAlpha, WICBitmapPaletteTypeCustom,
    WICDecodeMetadataCacheOnLoad,
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

use crate::model::{
    Action, DeleteTarget, Page, SettingsMenuKind, SettingsSection, TextInput, UiModel,
    quality_label,
};

// THESIS: one calm, local clipping workspace; refuse the old icon rail and
// tabbed settings page in favor of the supplied full-height navigation shell.
// OWN-WORLD: #0b0b0c canvas, #111113 panels, warm-white type, one-pixel
// #2a2a2d contours, ten-pixel cards, compact Segoe UI controls.
// STORY: save a replay, find it, organize it, and tune capture in one scan.
// FIRST VIEWPORT: 244px sidebar, 40px content inset, page tools at the top,
// dense four-column media grid; settings use a 2x2 panel matrix.
// FORM: pinned reference reproduction, operate mode, seed key WREATH-REF-2026.
// FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, and DESIGN.md
const CANVAS: u32 = 0x0b0b0c;
const STAGE: u32 = 0x101011;
const SURFACE: u32 = 0x121214;
const SURFACE_RAISED: u32 = 0x18181a;
const SURFACE_HOVER: u32 = 0x222225;
const BORDER: u32 = 0x2d2d30;
const PRIMARY: u32 = 0xf2f2f3;
const SECONDARY: u32 = 0xb4b4b8;
const ACCENT: u32 = 0xedeef2;
const ACCENT_HOVER: u32 = 0xffffff;
const ACCENT_MUTED: u32 = 0x28282b;
const READY: u32 = 0x35d07f;
const WARNING: u32 = 0xf0b849;
const SELECTION: u32 = 0x424854;
const DANGER: u32 = 0xf15b68;
const SETTINGS_ROW_TOP: f32 = 210.0;
const SETTINGS_ROW_HEIGHT: f32 = 56.0;
const SETTINGS_ROW_GAP: f32 = 0.0;
const SIDEBAR_EXPANDED: f32 = 244.0;
const SIDEBAR_COMPACT: f32 = 82.0;
const EDITOR_BOTTOM_RESERVE: f32 = 226.0;
const EDITOR_TIMELINE_HEIGHT: f32 = 118.0;
const HOME_GIRL_ASPECT_RATIO: f32 = 1206.0 / 1693.0;
const HOME_GIRL_BOTTOM_OVERFLOW: f32 = 70.0;
const HOME_GIRL_PNG: &[u8] = include_bytes!("../../../assets/wreath-home-girl.png");
const SETTINGS_STICKER_ASPECT_RATIO: f32 = 577.0 / 433.0;
const SETTINGS_STICKER_PNG: &[u8] = include_bytes!("../../../assets/wreath-settings-67.png");

fn settings_row_top(index: usize) -> f32 {
    SETTINGS_ROW_TOP + index as f32 * (SETTINGS_ROW_HEIGHT + SETTINGS_ROW_GAP)
}

#[derive(Debug, Clone, Copy)]
enum Glyph {
    Home,
    Library,
    Collections,
    Settings,
    Sliders,
    Folder,
    Record,
    Search,
    Grid,
    List,
    More,
    Plus,
    Clock,
    Monitor,
    Audio,
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

#[derive(Debug, Clone, Copy)]
enum PaginationKind {
    Library,
    CollectionCards,
    CollectionClips,
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

pub fn player_bounds(
    width: u32,
    height: u32,
    aspect_ratio: f32,
    sidebar_expanded: bool,
) -> LogicalRect {
    let width = width as f32;
    let rail = sidebar_width(width, sidebar_expanded);
    let padding = if width < 1_180.0 { 28.0 } else { 40.0 };
    let left = rail + padding;
    let right = width - padding;
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

pub fn editor_player_bounds(
    width: u32,
    height: u32,
    aspect_ratio: f32,
    sidebar_expanded: bool,
) -> LogicalRect {
    let width = width as f32;
    let rail = sidebar_width(width, sidebar_expanded);
    let padding = if width < 1_180.0 { 28.0 } else { 40.0 };
    let left = rail + padding;
    let right = width - padding;
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

pub fn editor_timeline_rail(
    width: u32,
    height: u32,
    aspect_ratio: f32,
    sidebar_expanded: bool,
) -> LogicalRect {
    let width_f = width as f32;
    let padding = if width_f < 1_180.0 { 28.0 } else { 40.0 };
    let left = sidebar_width(width_f, sidebar_expanded) + padding;
    let right = width_f - padding;
    let stage = editor_player_bounds(width, height, aspect_ratio, sidebar_expanded);
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

pub fn player_timeline_rail(
    width: u32,
    height: u32,
    aspect_ratio: f32,
    sidebar_expanded: bool,
) -> LogicalRect {
    let stage = player_bounds(width, height, aspect_ratio, sidebar_expanded);
    rect(
        stage.left + 260.0,
        stage.bottom + 35.0,
        stage.right - 64.0,
        stage.bottom + 41.0,
    )
}

pub fn player_volume_rail(
    width: u32,
    height: u32,
    aspect_ratio: f32,
    sidebar_expanded: bool,
) -> LogicalRect {
    let stage = player_bounds(width, height, aspect_ratio, sidebar_expanded);
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
    width: u32,
    height: u32,
    sidebar_expanded: bool,
    row: usize,
) -> LogicalRect {
    let width = width as f32;
    let rail = sidebar_width(width, sidebar_expanded);
    let padding = if width < 1_180.0 { 28.0 } else { 40.0 };
    let left = rail + padding;
    let right = width - padding;
    let [_, _, audio, _] = settings_panel_rects(left, right, height as f32);
    settings_gain_rail_in_panel(audio, row)
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

fn home_girl_layout(
    width: f32,
    height: f32,
    content_left: f32,
    content_right: f32,
) -> (LogicalRect, f32) {
    let minimum_content_width = if width < 1_080.0 { 420.0 } else { 540.0 };
    let available_height = (height - 250.0).max(180.0);
    let available_width = (width - content_left - minimum_content_width - 18.0).max(160.0);
    let girl_height = (height * 0.57)
        .clamp(240.0, 500.0)
        .min(available_height)
        .min(available_width / HOME_GIRL_ASPECT_RATIO);
    let girl_width = girl_height * HOME_GIRL_ASPECT_RATIO;
    let destination = rect(
        width - girl_width,
        height + HOME_GIRL_BOTTOM_OVERFLOW - girl_height,
        width + 1.0,
        height + HOME_GIRL_BOTTOM_OVERFLOW + 1.0,
    );
    let text_right = (destination.left - 18.0)
        .max(content_left + minimum_content_width)
        .min(content_right);
    (destination, text_right)
}

/// Decoration: it stays below the last settings row and is dropped rather than
/// shrunk when a short window leaves no room.
fn settings_sticker_layout(left: f32, right: f32, height: f32) -> Option<LogicalRect> {
    const MINIMUM_HEIGHT: f32 = 97.0;
    const MAXIMUM_HEIGHT: f32 = 190.0;
    const BOTTOM_MARGIN: f32 = 16.0;

    let top_limit = settings_row_top(2) + SETTINGS_ROW_HEIGHT + 20.0;
    let available_height = height - BOTTOM_MARGIN - top_limit;
    let available_width = (right - left) * 0.42;
    let sticker_height = available_height
        .min(MAXIMUM_HEIGHT)
        .min(available_width / SETTINGS_STICKER_ASPECT_RATIO);
    if sticker_height < MINIMUM_HEIGHT {
        return None;
    }
    let sticker_width = sticker_height * SETTINGS_STICKER_ASPECT_RATIO;
    let bottom = height - BOTTOM_MARGIN;
    Some(rect(
        right - sticker_width,
        bottom - sticker_height,
        right,
        bottom,
    ))
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
    brand: IDWriteTextFormat,
    page_title: IDWriteTextFormat,
    stat: IDWriteTextFormat,
    body: IDWriteTextFormat,
    small: IDWriteTextFormat,
    body_center: IDWriteTextFormat,
    media_icon: IDWriteTextFormat,
    navigation_icon: IDWriteTextFormat,
    fullscreen_icon: IDWriteTextFormat,
    hits: Vec<HitRegion>,
    wic_factory: IWICImagingFactory,
    home_girl: Option<ID2D1Bitmap>,
    settings_sticker: Option<ID2D1Bitmap>,
    thumbnails: HashMap<PathBuf, ID2D1Bitmap>,
    clip_durations: HashMap<PathBuf, Option<u64>>,
    /// Least recently drawn first, so the cache can be bounded.
    thumbnail_order: VecDeque<PathBuf>,
    unavailable_thumbnails: HashSet<PathBuf>,
    consecutive_failures: u32,
    hovered: Option<Action>,
    hover_progress: f32,
    reduced_motion: bool,
}

impl Renderer {
    /// Bounded: an unbounded cache turned a large library into hundreds of
    /// megabytes of bitmaps that were never drawn again.
    const MAX_THUMBNAILS: usize = 96;

    pub fn new() -> Result<Self, String> {
        let d2d_factory =
            unsafe { D2D1CreateFactory::<ID2D1Factory>(D2D1_FACTORY_TYPE_SINGLE_THREADED, None) }
                .map_err(|error| error.to_string())?;
        let write_factory =
            unsafe { DWriteCreateFactory::<IDWriteFactory>(DWRITE_FACTORY_TYPE_SHARED) }
                .map_err(|error| error.to_string())?;
        let title = text_format(
            &write_factory,
            w!("Segoe UI Variable Display"),
            31.0,
            true,
            false,
        )?;
        let heading = text_format(
            &write_factory,
            w!("Segoe UI Variable Display"),
            27.0,
            true,
            false,
        )?;
        let section = text_format(
            &write_factory,
            w!("Segoe UI Variable Display"),
            17.0,
            true,
            false,
        )?;
        let brand = text_format(
            &write_factory,
            w!("Segoe UI Variable Display"),
            24.0,
            true,
            false,
        )?;
        let page_title = text_format(
            &write_factory,
            w!("Segoe UI Variable Display"),
            29.0,
            true,
            false,
        )?;
        let stat = text_format(
            &write_factory,
            w!("Segoe UI Variable Display"),
            20.0,
            true,
            false,
        )?;
        let body = text_format(
            &write_factory,
            w!("Segoe UI Variable Text"),
            14.0,
            false,
            false,
        )?;
        let small = text_format(
            &write_factory,
            w!("Segoe UI Variable Text"),
            12.0,
            false,
            false,
        )?;
        let body_center = text_format(
            &write_factory,
            w!("Segoe UI Variable Text"),
            14.0,
            false,
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
            write_factory,
            target: None,
            title,
            heading,
            section,
            brand,
            page_title,
            stat,
            body,
            small,
            body_center,
            media_icon,
            navigation_icon,
            fullscreen_icon,
            hits: Vec::new(),
            wic_factory,
            home_girl: None,
            settings_sticker: None,
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

    /// Drops render-target-bound images while the window is hidden or the
    /// target is being rebuilt. They are decoded again on the next paint.
    pub fn release_cached_images(&mut self) {
        self.home_girl = None;
        self.settings_sticker = None;
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

    /// Starts the 120–140 ms Figma hover settle whenever the pointer crosses
    /// into a different interactive region.
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
        self.ensure_target(window, width, height)?;
        self.hits.clear();
        let target = self.target.as_ref().expect("render target exists").clone();
        unsafe {
            target.BeginDraw();
            target.Clear(Some(&color(CANVAS)));
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
        self.ensure_target(window, width, height)?;
        self.hits.clear();
        let target = self.target.as_ref().expect("render target exists").clone();
        unsafe {
            target.BeginDraw();
            target.Clear(Some(&color(CANVAS)));
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
            let rail = sidebar_width(width as f32, model.sidebar_expanded);
            let notice_tone = if model.hotkey_capture {
                WARNING
            } else {
                ACCENT
            };
            let notice_area = rect(
                rail + 18.0,
                height as f32 - 62.0,
                width as f32 - 22.0,
                height as f32 - 18.0,
            );
            self.fill(notice_area, SURFACE_HOVER, 10.0)?;
            self.stroke(notice_area, mix(BORDER, notice_tone, 0.42), 10.0, 1.0)?;
            self.fill(
                rect(
                    notice_area.left + 7.0,
                    notice_area.top + 12.0,
                    notice_area.left + 10.0,
                    notice_area.bottom - 12.0,
                ),
                notice_tone,
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
        if let Some(drag) = &model.clip_drag_preview {
            let label = if drag.count == 1 {
                "Move 1 clip".to_owned()
            } else {
                format!("Move {} clips", drag.count)
            };
            let chip_width = 138.0;
            let chip_height = 36.0;
            let left = (drag.x + 14.0).clamp(12.0, width as f32 - chip_width - 12.0);
            let top = (drag.y + 14.0).clamp(12.0, height as f32 - chip_height - 12.0);
            let chip = rect(left, top, left + chip_width, top + chip_height);
            self.fill(chip, SURFACE_RAISED, 8.0)?;
            self.stroke(chip, ACCENT, 8.0, 1.0)?;
            self.text(&label, chip, &self.body_center.clone(), PRIMARY)?;
        }
        Ok(())
    }

    fn render_fullscreen_header(&mut self, width: f32) -> Result<(), String> {
        self.fill(rect(0.0, 0.0, width, 78.0), CANVAS, 0.0)?;
        self.pill(
            rect(18.0, 18.0, 208.0, 60.0),
            SURFACE,
            "←  Zurück zur Preview",
            PRIMARY,
            Some(Action::ToggleFullscreen),
        )?;
        self.pill(
            rect(width - 228.0, 18.0, width - 18.0, 60.0),
            SURFACE,
            "Originalgröße     ESC",
            PRIMARY,
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
        self.fill(rect(0.0, 0.0, width, height), CANVAS, 0.0)?;
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
            PRIMARY,
            Some(Action::PlayPause),
            FloatingIconSize::Media,
        )?;
        self.floating_icon(
            rect(72.0, row_top, 112.0, row_bottom),
            "‹",
            SECONDARY,
            model.adjacent_clip(-1).map(|_| Action::PreviousClip),
            FloatingIconSize::Navigation,
        )?;
        self.floating_icon(
            rect(116.0, row_top, 156.0, row_bottom),
            "›",
            SECONDARY,
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
            SECONDARY,
        )?;
        self.floating_icon(
            rect(width - 108.0, row_top, width - 66.0, row_bottom),
            if model.player_volume_percent == 0 {
                "🔇"
            } else {
                "🔊"
            },
            PRIMARY,
            Some(Action::ToggleMute),
            FloatingIconSize::Media,
        )?;
        self.floating_icon(
            rect(width - 60.0, row_top, width - 18.0, row_bottom),
            "⛶",
            PRIMARY,
            Some(Action::ToggleFullscreen),
            FloatingIconSize::Fullscreen,
        )?;
        let info = rect(18.0, 92.0, width - 18.0, height - 10.0);
        self.fill(info, SURFACE, 9.0)?;
        self.stroke(info, BORDER, 9.0, 1.0)?;
        if let Some(clip) = model.active_clip() {
            let preview = rect(
                info.left + 18.0,
                info.top + 14.0,
                info.left + 108.0,
                info.bottom - 14.0,
            );
            self.fill(preview, STAGE, 6.0)?;
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
                PRIMARY,
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
                SECONDARY,
            )?;
            self.pill(
                rect(
                    width / 2.0 - 228.0,
                    info.top + 18.0,
                    width / 2.0 - 72.0,
                    info.bottom - 18.0,
                ),
                STAGE,
                "Clip bearbeiten",
                PRIMARY,
                Some(Action::EditActiveClip),
            )?;
            self.pill(
                rect(
                    width / 2.0 - 56.0,
                    info.top + 18.0,
                    width / 2.0 + 104.0,
                    info.bottom - 18.0,
                ),
                STAGE,
                "Ordner öffnen",
                PRIMARY,
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
                    STAGE,
                    "Clip löschen",
                    DANGER,
                    Some(Action::DeleteClip(index)),
                )?;
            }
        }
        Ok(())
    }

    #[allow(dead_code)]
    fn render_fullscreen_controls_legacy(
        &mut self,
        model: &UiModel,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        let width = width as f32;
        let height = height as f32;
        let timeline = fullscreen_timeline_rail(width as u32, height as u32);
        self.fill(timeline, SURFACE_HOVER, 2.0)?;
        let progress = if model.player_duration_seconds > 0.0 {
            (model.player_position_seconds / model.player_duration_seconds).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        let playhead = timeline.left + (timeline.right - timeline.left) * progress;
        if progress > 0.0 {
            self.fill(
                rect(timeline.left, timeline.top, playhead, timeline.bottom),
                ACCENT,
                2.0,
            )?;
        }
        let timeline_y = (timeline.top + timeline.bottom) / 2.0;
        self.fill(
            rect(
                playhead - 5.0,
                timeline_y - 5.0,
                playhead + 5.0,
                timeline_y + 5.0,
            ),
            PRIMARY,
            5.0,
        )?;
        self.hits.push(HitRegion {
            rect: rect(timeline.left, 0.0, timeline.right, 28.0),
            action: Action::DragPlayerSeek,
        });

        let row_top = height - 56.0;
        let row_bottom = height - 8.0;
        self.floating_icon(
            rect(18.0, row_top, 58.0, row_bottom),
            if model.player_playing { "Ⅱ" } else { "▶" },
            PRIMARY,
            Some(Action::PlayPause),
            FloatingIconSize::Media,
        )?;
        self.floating_icon(
            rect(62.0, row_top, 102.0, row_bottom),
            "‹",
            if model.adjacent_clip(-1).is_some() {
                PRIMARY
            } else {
                SECONDARY
            },
            model.adjacent_clip(-1).map(|_| Action::PreviousClip),
            FloatingIconSize::Navigation,
        )?;
        self.floating_icon(
            rect(106.0, row_top, 146.0, row_bottom),
            "›",
            if model.adjacent_clip(1).is_some() {
                PRIMARY
            } else {
                SECONDARY
            },
            model.adjacent_clip(1).map(|_| Action::NextClip),
            FloatingIconSize::Navigation,
        )?;
        self.floating_icon(
            rect(154.0, row_top, 194.0, row_bottom),
            if model.player_volume_percent == 0 {
                "🔇"
            } else {
                "🔊"
            },
            PRIMARY,
            Some(Action::ToggleMute),
            FloatingIconSize::Media,
        )?;

        let volume = fullscreen_volume_rail(width as u32, height as u32);
        self.fill(volume, SURFACE_HOVER, 3.0)?;
        let volume_fraction = f32::from(model.player_volume_percent) / 100.0;
        let volume_x = volume.left + (volume.right - volume.left) * volume_fraction;
        if volume_fraction > 0.0 {
            self.fill(
                rect(volume.left, volume.top, volume_x, volume.bottom),
                ACCENT,
                3.0,
            )?;
        }
        let volume_y = (volume.top + volume.bottom) / 2.0;
        self.fill(
            rect(
                volume_x - 5.0,
                volume_y - 5.0,
                volume_x + 5.0,
                volume_y + 5.0,
            ),
            PRIMARY,
            5.0,
        )?;
        self.hits.push(HitRegion {
            rect: rect(volume.left - 6.0, row_top, volume.right + 6.0, row_bottom),
            action: Action::DragPlayerVolume,
        });

        self.text(
            &format!(
                "{} / {}",
                format_player_time(model.player_position_seconds),
                format_player_time(model.player_duration_seconds)
            ),
            rect(
                volume.right + 20.0,
                row_top,
                volume.right + 150.0,
                row_bottom,
            ),
            &self.small.clone(),
            SECONDARY,
        )?;
        self.text(
            "Exit fullscreen",
            rect(width - 178.0, row_top, width - 54.0, row_bottom),
            &self.small.clone(),
            SECONDARY,
        )?;
        self.floating_icon(
            rect(width - 54.0, row_top, width - 14.0, row_bottom),
            "⛶",
            PRIMARY,
            Some(Action::ToggleFullscreen),
            FloatingIconSize::Fullscreen,
        )
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
        let rail = sidebar_width(width, model.sidebar_expanded);
        let expanded = rail >= 200.0;
        self.fill(rect(0.0, 0.0, rail, height), CANVAS, 0.0)?;
        self.fill(rect(rail - 1.0, 0.0, rail, height), BORDER, 0.0)?;
        let brand_row = rect(26.0, 48.0, rail - 20.0, 90.0);
        self.draw_wreath_logo(
            rect(
                brand_row.left,
                brand_row.top,
                brand_row.left + 42.0,
                brand_row.bottom,
            ),
            PRIMARY,
        )?;
        if expanded {
            self.text(
                "wreath",
                rect(
                    brand_row.left + 54.0,
                    brand_row.top,
                    brand_row.right,
                    brand_row.bottom,
                ),
                &self.brand.clone(),
                PRIMARY,
            )?;
        }

        let nav = [
            (Page::Home, Glyph::Home, "Dashboard"),
            (Page::Library, Glyph::Library, "Clips"),
            (Page::Collections, Glyph::Collections, "Collections"),
        ];
        for (offset, (page, icon, label)) in nav.iter().enumerate() {
            let top = 128.0 + offset as f32 * 56.0;
            let active = model.page == *page
                || (matches!(model.page, Page::Player | Page::Editor)
                    && model.previous_page == *page);
            self.sidebar_item(
                rail,
                top,
                *icon,
                label,
                active,
                Action::Navigate(*page),
                expanded,
            )?;
        }
        self.fill(rect(20.0, 302.0, rail - 20.0, 303.0), BORDER, 0.0)?;
        self.sidebar_item(
            rail,
            320.0,
            Glyph::Folder,
            "Ordner öffnen",
            false,
            Action::OpenClipsFolder,
            expanded,
        )?;
        if expanded && height >= 700.0 {
            let storage = rect(20.0, height - 230.0, rail - 20.0, height - 120.0);
            self.stroke(storage, BORDER, 10.0, 1.0)?;
            self.text(
                "Speicherplatz",
                rect(
                    storage.left + 18.0,
                    storage.top + 13.0,
                    storage.right - 18.0,
                    storage.top + 36.0,
                ),
                &self.body.clone(),
                PRIMARY,
            )?;
            let used = model.total_size_bytes();
            let limit = u64::from(model.config.storage.max_megabytes).saturating_mul(1_048_576);
            let fraction = if limit == 0 {
                0.0
            } else {
                (used as f32 / limit as f32).clamp(0.0, 1.0)
            };
            let track = rect(
                storage.left + 18.0,
                storage.top + 52.0,
                storage.right - 18.0,
                storage.top + 62.0,
            );
            self.fill(track, SURFACE_HOVER, 5.0)?;
            self.fill(
                rect(
                    track.left,
                    track.top,
                    track.left + (track.right - track.left) * fraction,
                    track.bottom,
                ),
                SECONDARY,
                5.0,
            )?;
            self.text(
                &format!(
                    "{} / {}",
                    format_bytes(used),
                    format_storage_limit(model.config.storage.max_megabytes)
                ),
                rect(
                    storage.left + 18.0,
                    storage.top + 72.0,
                    storage.right - 56.0,
                    storage.bottom - 10.0,
                ),
                &self.small.clone(),
                SECONDARY,
            )?;
            self.text(
                &format!("{}%", (fraction * 100.0).round() as u32),
                rect(
                    storage.right - 52.0,
                    storage.top + 72.0,
                    storage.right - 18.0,
                    storage.bottom - 10.0,
                ),
                &self.small.clone(),
                SECONDARY,
            )?;
        }
        self.fill(
            rect(20.0, height - 88.0, rail - 20.0, height - 87.0),
            BORDER,
            0.0,
        )?;
        self.sidebar_item(
            rail,
            height - 72.0,
            Glyph::Settings,
            "Einstellungen",
            model.page == Page::Settings,
            Action::Navigate(Page::Settings),
            expanded,
        )?;

        let padding = if width < 1_180.0 { 28.0 } else { 40.0 };
        let left = rail + padding;
        let right = width - padding;
        match model.page {
            Page::Home => self.render_home(model, left, right, width, height)?,
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
        _width: f32,
        height: f32,
    ) -> Result<(), String> {
        self.text(
            "Willkommen bei wreath",
            rect(left, 56.0, right - 190.0, 96.0),
            &self.page_title.clone(),
            PRIMARY,
        )?;
        self.text(
            "Erstelle Clips. Teile Momente. Behalte, was zählt.",
            rect(left, 96.0, right - 220.0, 126.0),
            &self.body.clone(),
            SECONDARY,
        )?;
        self.pill(
            rect(right - 144.0, 82.0, right, 128.0),
            SURFACE,
            "+  Neuer Clip",
            PRIMARY,
            Some(Action::SaveReplay),
        )?;

        let gap = 18.0;
        let card_width = ((right - left) - gap * 3.0) / 4.0;
        let card_top = 158.0;
        let card_bottom = 350.0_f32.min(height * 0.43);
        let display = model.selected_display().map_or_else(
            || "Automatisch".to_owned(),
            |display| format!("{} × {}", display.width, display.height),
        );
        let audio = match (model.config.audio.desktop, model.config.audio.microphone) {
            (true, true) => "System + Mikrofon",
            (true, false) => "Systemaudio",
            (false, true) => "Mikrofon",
            (false, false) => "Audio aus",
        };
        let cards = [
            (
                Glyph::Clock,
                "Replay-Dauer",
                format!("{} Sekunden", model.config.capture.duration_seconds),
                "Pufferlänge festlegen",
                Action::ChooseDuration,
            ),
            (
                Glyph::Monitor,
                "Bildschirm",
                display,
                "Aufnahmequelle wählen",
                Action::ChooseDisplay,
            ),
            (
                Glyph::Sliders,
                "Qualität",
                format!(
                    "{} · {} FPS",
                    quality_label(model.config.capture.quality),
                    model.config.capture.frames_per_second
                ),
                "Aufnahmequalität wählen",
                Action::ChooseQuality,
            ),
            (
                Glyph::Audio,
                "Audio",
                audio.to_owned(),
                "Tonspuren auswählen",
                Action::ChooseAudioMode,
            ),
        ];
        for (index, (glyph, title, value, description, action)) in cards.into_iter().enumerate() {
            let x = left + index as f32 * (card_width + gap);
            self.quick_setup_card(
                rect(x, card_top, x + card_width, card_bottom),
                glyph,
                title,
                &value,
                description,
                action,
            )?;
        }

        let recent_top = card_bottom + 34.0;
        self.text(
            "Kürzlich aufgenommene Clips",
            rect(left, recent_top, right - 180.0, recent_top + 30.0),
            &self.section.clone(),
            PRIMARY,
        )?;
        self.text(
            "Alle anzeigen  →",
            rect(right - 150.0, recent_top, right, recent_top + 30.0),
            &self.body.clone(),
            SECONDARY,
        )?;
        let indices = model.visible_clip_indices(8);
        if indices.is_empty() {
            self.empty_state("Noch keine Clips", left, right, recent_top + 84.0)?;
        } else {
            self.clip_grid(
                model,
                &indices,
                left,
                right,
                recent_top + 42.0,
                height - 126.0,
            )?;
        }
        if height >= 790.0 {
            let status = rect(left + 82.0, height - 90.0, right - 80.0, height - 28.0);
            self.fill(status, SURFACE, 12.0)?;
            self.stroke(status, BORDER, 12.0, 1.0)?;
            self.text(
                if model.config.hotkey.is_bound() {
                    "Bereit zum Aufnehmen"
                } else {
                    "Hotkey erforderlich"
                },
                rect(
                    status.left + 20.0,
                    status.top + 8.0,
                    status.right - 210.0,
                    status.top + 33.0,
                ),
                &self.body.clone(),
                PRIMARY,
            )?;
            self.text(
                "Drücke deinen Hotkey oder speichere den Replay direkt.",
                rect(
                    status.left + 20.0,
                    status.top + 31.0,
                    status.right - 210.0,
                    status.bottom - 5.0,
                ),
                &self.small.clone(),
                SECONDARY,
            )?;
            self.icon_button(
                rect(
                    status.right - 56.0,
                    status.top + 10.0,
                    status.right - 12.0,
                    status.bottom - 10.0,
                ),
                Glyph::Folder,
                Action::QuickOpenClipsFolder,
            )?;
            self.icon_button(
                rect(
                    status.right - 110.0,
                    status.top + 10.0,
                    status.right - 66.0,
                    status.bottom - 10.0,
                ),
                Glyph::Sliders,
                Action::QuickOpenSettings,
            )?;
            self.icon_button(
                rect(
                    status.right - 164.0,
                    status.top + 10.0,
                    status.right - 120.0,
                    status.bottom - 10.0,
                ),
                Glyph::Record,
                Action::QuickSaveReplay,
            )?;
        }
        Ok(())
    }

    fn render_library(
        &mut self,
        model: &UiModel,
        left: f32,
        right: f32,
        height: f32,
    ) -> Result<(), String> {
        self.page_heading(
            "Clips",
            "Hier findest du all deine aufgenommenen Clips.",
            left,
            right,
        )?;
        let tools_top = 96.0;
        let search = rect(
            (right - 736.0).max(left + 310.0),
            tools_top,
            right - 466.0,
            tools_top + 45.0,
        );
        self.search_field(model, search, "Clips suchen...")?;
        self.pill(
            rect(right - 445.0, tools_top, right - 252.0, tools_top + 45.0),
            SURFACE,
            if model.clips_oldest_first {
                "Sortieren: Älteste"
            } else {
                "Sortieren: Neueste"
            },
            PRIMARY,
            Some(Action::ToggleClipSort),
        )?;
        self.pill(
            rect(right - 232.0, tools_top, right - 122.0, tools_top + 45.0),
            SURFACE,
            if model.selection_mode {
                "Abbrechen"
            } else {
                "Auswählen"
            },
            PRIMARY,
            Some(Action::ToggleSelectionMode),
        )?;
        self.icon_button(
            rect(right - 112.0, tools_top, right - 64.0, tools_top + 45.0),
            Glyph::Grid,
            Action::SetLibraryGrid(true),
        )?;
        self.icon_button(
            rect(right - 52.0, tools_top, right, tools_top + 45.0),
            Glyph::List,
            Action::SetLibraryGrid(false),
        )?;

        let stats = rect(left, 162.0, right, 251.0);
        self.fill(stats, SURFACE, 10.0)?;
        self.stroke(stats, BORDER, 10.0, 1.0)?;
        let values = [
            (Glyph::Library, "Gesamtclips", model.clips.len().to_string()),
            (
                Glyph::Clock,
                "Replay-Fenster",
                format!("{} Sekunden", model.config.capture.duration_seconds),
            ),
            (
                Glyph::Folder,
                "Gesamtgröße",
                format_bytes(model.total_size_bytes()),
            ),
            (
                Glyph::Record,
                "Letzte Aufnahme",
                model
                    .clips
                    .first()
                    .map_or("Noch keine".to_owned(), |clip| age(clip.modified)),
            ),
        ];
        let stat_width = (right - left) / 4.0;
        for (index, (glyph, label, value)) in values.into_iter().enumerate() {
            let x = left + stat_width * index as f32;
            if index > 0 {
                self.fill(
                    rect(x, stats.top + 21.0, x + 1.0, stats.bottom - 21.0),
                    BORDER,
                    0.0,
                )?;
            }
            self.glyph(
                glyph,
                rect(x + 50.0, stats.top + 30.0, x + 82.0, stats.top + 62.0),
                SECONDARY,
            )?;
            self.text(
                label,
                rect(
                    x + 100.0,
                    stats.top + 15.0,
                    x + stat_width - 18.0,
                    stats.top + 43.0,
                ),
                &self.small.clone(),
                SECONDARY,
            )?;
            self.text(
                &value,
                rect(
                    x + 100.0,
                    stats.top + 41.0,
                    x + stat_width - 18.0,
                    stats.bottom - 12.0,
                ),
                &self.stat.clone(),
                PRIMARY,
            )?;
        }
        self.text(
            "Alle Clips",
            rect(left, 282.0, right, 315.0),
            &self.section.clone(),
            PRIMARY,
        )?;
        let indices = model.visible_clip_indices(usize::MAX);
        if indices.is_empty() {
            self.empty_state(
                if model.search.value.is_empty() {
                    "Noch keine Clips"
                } else {
                    "Keine passenden Clips"
                },
                left,
                right,
                365.0,
            )?;
        } else {
            let content_top = 322.0;
            let content_bottom = height - 92.0;
            let per_page = if model.library_grid {
                grid_capacity(right - left, content_bottom - content_top)
            } else {
                (((content_bottom - content_top - 48.0) / 71.0).floor() as usize).max(1)
            };
            let total_pages = indices.len().div_ceil(per_page);
            let page = model.library_page.min(total_pages.saturating_sub(1));
            let start = page * per_page;
            let page_indices = &indices[start..(start + per_page).min(indices.len())];
            if model.library_grid {
                self.clip_grid(
                    model,
                    page_indices,
                    left,
                    right,
                    content_top,
                    content_bottom,
                )?;
            } else {
                self.clip_list(
                    model,
                    page_indices,
                    left,
                    right,
                    content_top,
                    content_bottom,
                )?;
            }
            self.pagination(
                (left + right) / 2.0,
                height - 70.0,
                page,
                total_pages,
                PaginationKind::Library,
            )?;
        }
        if model.collection_picker_open {
            self.render_collection_picker(model, right, 154.0)?;
        }
        Ok(())
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
            "Organisiere deine Clips in Sammlungen.",
            left,
            right,
        )?;
        self.pill(
            rect(right - 170.0, 86.0, right, 130.0),
            ACCENT,
            "+  Neue Sammlung",
            CANVAS,
            Some(Action::CreateCollection),
        )?;
        let detail_width = if right - left >= 1080.0 { 244.0 } else { 0.0 };
        let content_right = right
            - if detail_width > 0.0 {
                detail_width + 40.0
            } else {
                0.0
            };
        self.search_field(
            model,
            rect(
                left,
                157.0,
                (left + 576.0).min(content_right - 260.0),
                204.0,
            ),
            "Sammlungen suchen...",
        )?;
        self.pill(
            rect(content_right - 296.0, 157.0, content_right - 130.0, 204.0),
            SURFACE,
            if model.collections_descending {
                "Sortieren: Z-A"
            } else {
                "Sortieren: A-Z"
            },
            PRIMARY,
            Some(Action::ToggleCollectionSort),
        )?;
        self.icon_button(
            rect(content_right - 112.0, 157.0, content_right - 62.0, 204.0),
            Glyph::Grid,
            Action::SetCollectionsGrid(true),
        )?;
        self.icon_button(
            rect(content_right - 50.0, 157.0, content_right, 204.0),
            Glyph::List,
            Action::SetCollectionsGrid(false),
        )?;

        let card_top = 235.0;
        let gap = 14.0;
        let collection_indices = model.visible_collection_indices();
        let cards_per_page = 2;
        let card_pages = collection_indices.len().div_ceil(cards_per_page).max(1);
        let card_page = model
            .collection_cards_page
            .min(card_pages.saturating_sub(1));
        let card_start = card_page * cards_per_page;
        let visible_cards = &collection_indices
            [card_start..(card_start + cards_per_page).min(collection_indices.len())];
        if model.collections_grid {
            let column_count = (visible_cards.len() + 1).clamp(1, 4);
            let card_width =
                ((content_right - left) - gap * (column_count - 1) as f32) / column_count as f32;
            let card_height = 116.0;
            self.collection_card(
                rect(left, card_top, left + card_width, card_top + card_height),
                "Alle Clips",
                "Lokale Bibliothek",
                model.clips.len(),
                Glyph::Library,
                model.active_collection.is_none(),
                Action::SelectCollection(None),
            )?;
            for (slot, collection_index) in visible_cards.iter().enumerate() {
                let collection = &model.collections[*collection_index];
                let x = left + (slot + 1) as f32 * (card_width + gap);
                self.collection_card(
                    rect(x, card_top, x + card_width, card_top + card_height),
                    &collection.name,
                    "Gespeicherte Sammlung",
                    collection.clip_count,
                    Glyph::Collections,
                    model.active_collection.as_ref() == Some(&collection.path),
                    Action::SelectCollection(Some(*collection_index)),
                )?;
            }
            if collection_indices.len() < 2 && model.search.value.is_empty() && height >= 800.0 {
                self.collection_card(
                    rect(left, card_top + 138.0, left + card_width, card_top + 226.0),
                    "Neue Sammlung",
                    "Erstelle eine neue Sammlung",
                    0,
                    Glyph::Plus,
                    false,
                    Action::CreateCollection,
                )?;
            }
        } else {
            self.collection_row(
                "Alle Clips",
                model.clips.len(),
                model.active_collection.is_none(),
                rect(left, card_top, content_right, card_top + 44.0),
                None,
                false,
            )?;
            for (slot, collection_index) in visible_cards.iter().enumerate() {
                let collection = &model.collections[*collection_index];
                let y = card_top + 50.0 + slot as f32 * 50.0;
                self.collection_row(
                    &collection.name,
                    collection.clip_count,
                    model.active_collection.as_ref() == Some(&collection.path),
                    rect(left, y, content_right, y + 44.0),
                    Some(*collection_index),
                    false,
                )?;
            }
        }
        self.pagination(
            (left + content_right) / 2.0,
            card_top + 128.0,
            card_page,
            card_pages,
            PaginationKind::CollectionCards,
        )?;

        let title = model
            .active_collection
            .as_ref()
            .and_then(|active| {
                model
                    .collections
                    .iter()
                    .find(|collection| &collection.path == active)
            })
            .map_or("Clips in „Alle Clips“", |collection| {
                collection.name.as_str()
            });
        let table_top = (card_top + 205.0).min(height - 150.0);
        self.text(
            title,
            rect(left, table_top - 34.0, content_right, table_top),
            &self.section.clone(),
            PRIMARY,
        )?;
        self.collection_table(model, left, content_right, table_top, height - 54.0)?;

        if detail_width > 0.0 {
            let detail = rect(content_right + 40.0, 157.0, right, 437.0);
            self.fill(detail, SURFACE, 10.0)?;
            self.stroke(detail, BORDER, 10.0, 1.0)?;
            self.text(
                title.trim_start_matches("Clips in „").trim_end_matches('“'),
                rect(
                    detail.left + 22.0,
                    detail.top + 18.0,
                    detail.right - 22.0,
                    detail.top + 48.0,
                ),
                &self.section.clone(),
                PRIMARY,
            )?;
            self.text(
                "Lokale Clips und Aufnahmen",
                rect(
                    detail.left + 22.0,
                    detail.top + 50.0,
                    detail.right - 22.0,
                    detail.top + 76.0,
                ),
                &self.small.clone(),
                SECONDARY,
            )?;
            self.fill(
                rect(
                    detail.left + 22.0,
                    detail.top + 90.0,
                    detail.right - 22.0,
                    detail.top + 91.0,
                ),
                BORDER,
                0.0,
            )?;
            self.text(
                &format!("{} Clips", model.visible_clip_indices(usize::MAX).len()),
                rect(
                    detail.left + 22.0,
                    detail.top + 101.0,
                    detail.right - 22.0,
                    detail.top + 131.0,
                ),
                &self.body.clone(),
                SECONDARY,
            )?;
            self.text(
                "Aktionen",
                rect(
                    detail.left + 22.0,
                    detail.top + 151.0,
                    detail.right - 22.0,
                    detail.top + 177.0,
                ),
                &self.small.clone(),
                SECONDARY,
            )?;
            let rename = rect(
                detail.left + 22.0,
                detail.top + 181.0,
                detail.right - 22.0,
                detail.top + 209.0,
            );
            self.text("Umbenennen", rename, &self.body.clone(), PRIMARY)?;
            if model.active_collection.is_some() {
                self.hits.push(HitRegion {
                    rect: rename,
                    action: Action::RenameActiveCollection,
                });
                let delete = rect(
                    detail.left + 22.0,
                    detail.top + 217.0,
                    detail.right - 22.0,
                    detail.top + 245.0,
                );
                self.text("Löschen", delete, &self.body.clone(), DANGER)?;
                self.hits.push(HitRegion {
                    rect: delete,
                    action: Action::DeleteActiveCollection,
                });
            }
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
            "Einstellungen",
            "Passe wreath nach deinen Wünschen an.",
            left,
            right,
        )?;
        self.pill(
            rect(right - 154.0, 86.0, right, 130.0),
            ACCENT,
            "Speichern",
            CANVAS,
            Some(Action::SaveSettings),
        )?;
        let [general, capture, audio, storage] = settings_panel_rects(left, right, height);
        self.settings_panel(general, "Allgemein")?;
        self.settings_compact_row(
            general,
            0,
            "Programmstart",
            "wreath automatisch mit Windows starten",
            if model.autostart_enabled { "An" } else { "Aus" },
            Action::ToggleAutostart,
            SettingControl::Toggle,
        )?;
        let shortcut = if model.hotkey_pending {
            "Aktivieren...".to_owned()
        } else if model.hotkey_capture {
            hotkey_capture_label(&model.hotkey_modifiers)
        } else {
            wreath_windows::hotkey::localized_hotkey_label(&model.config.hotkey)
        };
        self.settings_compact_row(
            general,
            1,
            "Replay-Hotkey",
            "Speichert den aktuellen Replay",
            &shortcut,
            Action::CaptureHotkey,
            SettingControl::Button,
        )?;
        let row_height = compact_settings_row_height(general);
        let hotkey_top = general.top + 44.0 + row_height;
        let clear = rect(
            general.right - 48.0,
            hotkey_top + 7.0,
            general.right - 20.0,
            hotkey_top + row_height - 7.0,
        );
        self.fill(clear, SURFACE_RAISED, 6.0)?;
        self.glyph(
            Glyph::Close,
            rect(
                clear.left + 3.0,
                clear.top + 3.0,
                clear.right - 3.0,
                clear.bottom - 3.0,
            ),
            SECONDARY,
        )?;
        self.hits.push(HitRegion {
            rect: clear,
            action: Action::ClearHotkey,
        });
        self.settings_compact_row(
            general,
            2,
            "Mauszeiger aufnehmen",
            "Zeiger in Aufnahmen anzeigen",
            if model.config.capture.cursor {
                "An"
            } else {
                "Aus"
            },
            Action::ToggleCursor,
            SettingControl::Toggle,
        )?;

        self.settings_panel(capture, "Aufnahmen")?;
        let display = model
            .selected_display()
            .map_or("Primärer Bildschirm", |display| display.label.as_str());
        self.settings_compact_row(
            capture,
            0,
            "Bildschirm",
            "Aufnahmequelle",
            display,
            Action::ChooseDisplay,
            SettingControl::Dropdown,
        )?;
        self.settings_compact_row(
            capture,
            1,
            "Clip-Dauer",
            "Länge des Replay-Fensters",
            &format!("{} Sekunden", model.config.capture.duration_seconds),
            Action::ChooseDuration,
            SettingControl::Dropdown,
        )?;
        self.settings_compact_row(
            capture,
            2,
            "Bildrate",
            "Maximal 60 Bilder pro Sekunde",
            &format!("{} fps", model.config.capture.frames_per_second),
            Action::ChooseFrameRate,
            SettingControl::Dropdown,
        )?;
        self.settings_compact_row(
            capture,
            3,
            "Codec",
            "Hardware-Encoder",
            &format!("{:?}", model.config.capture.codec),
            Action::ChooseCodec,
            SettingControl::Dropdown,
        )?;

        self.settings_panel(audio, "Audio")?;
        self.settings_compact_row(
            audio,
            0,
            "Systemaudio",
            "Spiel- und Desktop-Ton aufnehmen",
            if model.config.audio.desktop {
                "An"
            } else {
                "Aus"
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
            .map_or("Windows-Standard", |(_, name)| name.as_str());
        self.settings_compact_row(
            audio,
            1,
            "Ausgabegerät",
            "Quelle für Systemaudio",
            output_name,
            Action::ChooseDesktopDevice,
            SettingControl::Dropdown,
        )?;
        self.settings_gain_slider(
            audio,
            2,
            "Systemaudio-Pegel",
            "Balance der Desktop-Aufnahme",
            model.config.audio.desktop_gain_percent,
            Action::DragDesktopGain,
        )?;
        self.settings_compact_row(
            audio,
            3,
            "Mikrofon",
            "Eingabegerät mit aufnehmen",
            if model.config.audio.microphone {
                "An"
            } else {
                "Aus"
            },
            Action::ToggleMicrophone,
            SettingControl::Toggle,
        )?;
        self.settings_gain_slider(
            audio,
            4,
            "Mikrofon-Pegel",
            "Lautstärke der Stimme",
            model.config.audio.microphone_gain_percent,
            Action::DragMicrophoneGain,
        )?;

        self.settings_panel(storage, "Speicher und Qualität")?;
        self.settings_compact_row(
            storage,
            0,
            "Speicherort",
            "Lokaler Ordner für Clips",
            &model.config.storage.directory.display().to_string(),
            Action::ChooseStorage,
            SettingControl::Button,
        )?;
        self.settings_compact_row(
            storage,
            1,
            "Speicherlimit",
            "Maximaler Platz für Clips",
            &format_storage_limit(model.config.storage.max_megabytes),
            Action::ChooseStorageLimit,
            SettingControl::Dropdown,
        )?;
        self.settings_compact_row(
            storage,
            2,
            "Videoqualität",
            "Detailgrad und Speicherbedarf",
            &quality_label(model.config.capture.quality),
            Action::ChooseQuality,
            SettingControl::Dropdown,
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
            .map_or("Windows-Standard", |(_, name)| name.as_str());
        self.settings_compact_row(
            storage,
            3,
            "Mikrofon-Gerät",
            "Aktives Windows-Eingabegerät",
            microphone_name,
            Action::ChooseMicrophone,
            SettingControl::Dropdown,
        )?;
        if height >= 920.0 {
            let about = rect(left, audio.bottom + 14.0, right, height - 14.0);
            self.fill(about, SURFACE, 10.0)?;
            self.stroke(about, BORDER, 10.0, 1.0)?;
            self.text(
                "Über wreath",
                rect(
                    about.left + 20.0,
                    about.top + 8.0,
                    about.right - 230.0,
                    about.top + 39.0,
                ),
                &self.body.clone(),
                PRIMARY,
            )?;
            self.text(
                &format!("wreath {}", env!("CARGO_PKG_VERSION")),
                rect(
                    about.left + 20.0,
                    about.top + 37.0,
                    about.right - 230.0,
                    about.top + 62.0,
                ),
                &self.small.clone(),
                SECONDARY,
            )?;
            self.text(
                "Ein lokales Clipping-Tool für deine besten Momente.",
                rect(
                    about.left + 20.0,
                    about.top + 59.0,
                    about.right - 230.0,
                    about.bottom - 6.0,
                ),
                &self.small.clone(),
                SECONDARY,
            )?;
            self.pill(
                rect(
                    about.right - 196.0,
                    about.top + 22.0,
                    about.right - 20.0,
                    about.bottom - 22.0,
                ),
                STAGE,
                "Einstellungen speichern",
                PRIMARY,
                Some(Action::SaveSettings),
            )?;
        }
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
            "Clip-Preview",
            "Schau dir deinen Clip an und prüfe die besten Momente.",
            left,
            right,
        )?;
        self.pill(
            rect(right - 344.0, 92.0, right - 218.0, 140.0),
            SURFACE,
            "←  Zurück",
            PRIMARY,
            Some(Action::Back),
        )?;
        self.pill(
            rect(right - 200.0, 92.0, right, 140.0),
            ACCENT,
            "Clip bearbeiten",
            CANVAS,
            Some(Action::EditActiveClip),
        )?;
        let Some(clip) = model.active_clip() else {
            self.empty_state("Clip nicht verfügbar", left, right, 240.0)?;
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
        self.fill(stage, STAGE, 10.0)?;
        self.stroke(stage, BORDER, 10.0, 1.0)?;
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
        self.fill(controls, SURFACE, 0.0)?;
        self.stroke(controls, BORDER, 0.0, 1.0)?;
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
            PRIMARY,
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
            SECONDARY,
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
            SECONDARY,
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
            SECONDARY,
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
            PRIMARY,
            (!editor).then_some(Action::ToggleFullscreen),
        )
    }

    fn draw_progress_rail(&self, model: &UiModel, rail: LogicalRect) -> Result<(), String> {
        self.fill(rail, SURFACE_HOVER, 3.0)?;
        let progress = if model.player_duration_seconds > 0.0 {
            (model.player_position_seconds / model.player_duration_seconds).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        let x = rail.left + (rail.right - rail.left) * progress;
        self.fill(rect(rail.left, rail.top, x, rail.bottom), ACCENT, 3.0)?;
        self.fill(
            rect(
                x - 5.0,
                (rail.top + rail.bottom) / 2.0 - 5.0,
                x + 5.0,
                (rail.top + rail.bottom) / 2.0 + 5.0,
            ),
            PRIMARY,
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
        self.fill(area, SURFACE, 10.0)?;
        self.stroke(area, BORDER, 10.0, 1.0)?;
        self.text(
            "Clip-Informationen",
            rect(
                area.left + 20.0,
                area.top + 10.0,
                area.right - 20.0,
                area.top + 43.0,
            ),
            &self.body.clone(),
            PRIMARY,
        )?;
        let resolution = if model.player_video_width > 0 && model.player_video_height > 0 {
            format!("{}×{}", model.player_video_width, model.player_video_height)
        } else {
            "Wird geladen".to_owned()
        };
        let rows = [
            ("Titel", clip.title.clone()),
            ("Erstellt", age(clip.modified)),
            (
                "Dauer (Original)",
                format_player_time(model.player_duration_seconds),
            ),
            ("Größe (Original)", format_bytes(clip.size_bytes)),
            ("Auflösung", resolution),
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
                SECONDARY,
            )?;
            self.text(
                &ellipsize(&value, 31),
                rect(
                    area.left + 20.0,
                    top + 19.0,
                    if has_title_action {
                        area.right - 56.0
                    } else {
                        area.right - 20.0
                    },
                    top + row_height,
                ),
                &self.body.clone(),
                PRIMARY,
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
                        PRIMARY
                    } else {
                        SECONDARY
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
                STAGE,
                "Clip löschen",
                DANGER,
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
            "Clip bearbeiten",
            rect(left, 62.0, right - 470.0, 94.0),
            &self.brand.clone(),
            PRIMARY,
        )?;
        self.text(
            "Schneide deinen Clip und speichere nur die besten Momente.",
            rect(left, 94.0, right - 470.0, 122.0),
            &self.body.clone(),
            SECONDARY,
        )?;
        let Some(clip) = model.active_clip() else {
            self.empty_state("Clip nicht verfügbar", left, right, 240.0)?;
            return Ok(());
        };
        let enabled = model.editor_timing.is_some() && !model.editor_working;
        let undo_enabled = enabled && model.can_undo_editor_trim();
        let redo_enabled = enabled && model.can_redo_editor_trim();
        self.pill(
            rect(right - 430.0, 82.0, right - 386.0, 126.0),
            SURFACE,
            "↶",
            if undo_enabled { PRIMARY } else { BORDER },
            undo_enabled.then_some(Action::UndoEditorTrim),
        )?;
        self.pill(
            rect(right - 374.0, 82.0, right - 330.0, 126.0),
            SURFACE,
            "↷",
            if redo_enabled { PRIMARY } else { BORDER },
            redo_enabled.then_some(Action::RedoEditorTrim),
        )?;
        self.pill(
            rect(right - 320.0, 82.0, right - 174.0, 126.0),
            SURFACE,
            "Verwerfen",
            PRIMARY,
            Some(Action::Back),
        )?;
        self.pill(
            rect(right - 158.0, 82.0, right, 126.0),
            if enabled { ACCENT } else { SURFACE_HOVER },
            if model.editor_working {
                "Speichert…"
            } else {
                "Speichern"
            },
            if enabled { CANVAS } else { SECONDARY },
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
        self.fill(stage, STAGE, 10.0)?;
        self.stroke(stage, BORDER, 10.0, 1.0)?;
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
            self.fill(duration, SURFACE, 10.0)?;
            self.stroke(duration, BORDER, 10.0, 1.0)?;
            self.text(
                "Geschnittene Dauer",
                rect(
                    duration.left + 18.0,
                    duration.top + 8.0,
                    duration.right - 18.0,
                    duration.top + 38.0,
                ),
                &self.body.clone(),
                PRIMARY,
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
                SECONDARY,
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
                PRIMARY,
            )?;
            let save_mode = rect(
                detail_left,
                duration.bottom + 14.0,
                right,
                duration.bottom + 80.0,
            );
            self.fill(save_mode, SURFACE, 10.0)?;
            self.stroke(save_mode, BORDER, 10.0, 1.0)?;
            self.text(
                "Speichern als",
                rect(
                    save_mode.left + 14.0,
                    save_mode.top + 3.0,
                    save_mode.right - 14.0,
                    save_mode.top + 26.0,
                ),
                &self.small.clone(),
                SECONDARY,
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
                    STAGE
                } else {
                    ACCENT
                },
                "Neuer Clip",
                if model.trim_replace_original {
                    PRIMARY
                } else {
                    CANVAS
                },
                Some(Action::SetTrimReplace(false)),
            )?;
            self.pill(
                rect(split + 3.0, choices.top, choices.right, choices.bottom),
                if model.trim_replace_original {
                    ACCENT
                } else {
                    STAGE
                },
                "Original ersetzen",
                if model.trim_replace_original {
                    CANVAS
                } else {
                    PRIMARY
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
        self.fill(timeline, SURFACE, 10.0)?;
        self.stroke(timeline, BORDER, 10.0, 1.0)?;
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
                SECONDARY,
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
        self.fill(area, STAGE, 7.0)?;
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
            PRIMARY,
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
            PRIMARY,
            5.0,
        )?;
        self.fill(
            rect(end_x - 8.0, area.top - 2.0, end_x + 8.0, area.bottom + 2.0),
            PRIMARY,
            5.0,
        )?;
        self.fill(
            rect(
                playhead_x - 1.0,
                area.top - 56.0,
                playhead_x + 1.0,
                area.bottom + 24.0,
            ),
            PRIMARY,
            0.0,
        )?;
        self.fill(
            rect(
                playhead_x - 5.0,
                area.top - 60.0,
                playhead_x + 5.0,
                area.top - 50.0,
            ),
            PRIMARY,
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

    #[allow(dead_code)]
    fn render_home_legacy(
        &mut self,
        model: &UiModel,
        left: f32,
        right: f32,
        width: f32,
        height: f32,
    ) -> Result<(), String> {
        let hotkey_ready = model.config.hotkey.is_bound();
        let status_color = if hotkey_ready { READY } else { DANGER };
        self.text(
            "Replay buffer",
            rect(left, 116.0, right, 138.0),
            &self.small.clone(),
            SECONDARY,
        )?;
        self.text(
            if hotkey_ready {
                "Replay ready"
            } else {
                "Shortcut required"
            },
            rect(left, 143.0, right, 198.0),
            &self.title.clone(),
            PRIMARY,
        )?;
        let (home_girl, content_right) = home_girl_layout(width, height, left, right);
        let panel_bottom = 542.0_f32.min(height - 42.0);
        let panel = rect(left, 216.0, content_right, panel_bottom);

        self.fill(
            rect(
                panel.left,
                panel.top + 24.0,
                panel.left + 3.0,
                panel.top + 126.0,
            ),
            status_color,
            1.5,
        )?;

        let pulse = rect(
            panel.left + 34.0,
            panel.top + 40.0,
            panel.left + 44.0,
            panel.top + 50.0,
        );
        self.fill(pulse, status_color, 5.0)?;
        self.text(
            if hotkey_ready {
                "CAPTURE READY"
            } else {
                "HOTKEY NOT SET"
            },
            rect(
                panel.left + 58.0,
                panel.top + 34.0,
                panel.right - 34.0,
                panel.top + 56.0,
            ),
            &self.small.clone(),
            status_color,
        )?;
        self.text(
            &if hotkey_ready {
                format!(
                    "Your last {} seconds are ready.",
                    model.config.capture.duration_seconds
                )
            } else {
                "Set a shortcut before you start clipping.".to_owned()
            },
            rect(
                panel.left + 34.0,
                panel.top + 60.0,
                panel.right - 250.0,
                panel.top + 104.0,
            ),
            &self.heading.clone(),
            PRIMARY,
        )?;
        self.text(
            if hotkey_ready {
                "Save a replay without leaving what you are doing."
            } else {
                "Wreath cannot save a replay until a hotkey is configured in Settings → Controls."
            },
            rect(
                panel.left + 34.0,
                panel.top + 108.0,
                panel.right - 250.0,
                panel.top + 138.0,
            ),
            &self.body.clone(),
            SECONDARY,
        )?;
        let signal_left = panel.left + 34.0;
        let signal_right = panel.right - 34.0;
        let signal_y = panel.top + 164.0;
        self.fill(
            rect(signal_left, signal_y, signal_right, signal_y + 1.0),
            BORDER,
            0.0,
        )?;
        let signal_width = signal_right - signal_left;
        for index in 0..28 {
            let progress = index as f32 / 27.0;
            let x = signal_left + signal_width * progress;
            let tick_height = match index % 5 {
                0 => 8.0,
                1 | 4 => 4.0,
                _ => 6.0,
            };
            self.fill(
                rect(
                    x,
                    signal_y - tick_height * 0.5,
                    x + 1.0,
                    signal_y + tick_height * 0.5,
                ),
                if index >= 24 { status_color } else { BORDER },
                0.0,
            )?;
        }

        let divider_top = panel.top + 194.0;
        self.fill(
            rect(signal_left, divider_top, signal_right, divider_top + 1.0),
            BORDER,
            0.0,
        )?;
        let display = model.selected_display().map_or_else(
            || format!("{} fps", model.config.capture.frames_per_second),
            |display| {
                format!(
                    "{}×{} · {} fps",
                    display.width, display.height, model.config.capture.frames_per_second
                )
            },
        );
        let audio = if model.config.audio.desktop && model.config.audio.microphone {
            "Game + microphone"
        } else if model.config.audio.desktop {
            "Game audio"
        } else if model.config.audio.microphone {
            "Microphone"
        } else {
            "Audio off"
        };
        let facts = [
            ("Display", display),
            ("Audio", audio.to_owned()),
            (
                "Library",
                format!(
                    "{} clips · {}",
                    model.clips.len(),
                    format_bytes(model.total_size_bytes())
                ),
            ),
        ];
        let facts_top = divider_top + 22.0;
        let fact_width = (panel.right - panel.left - 68.0) / facts.len() as f32;
        for (index, (label, value)) in facts.iter().enumerate() {
            let fact_left = panel.left + 34.0 + fact_width * index as f32;
            if index > 0 {
                self.fill(
                    rect(
                        fact_left,
                        facts_top - 2.0,
                        fact_left + 1.0,
                        facts_top + 52.0,
                    ),
                    BORDER,
                    0.0,
                )?;
            }
            self.text(
                label,
                rect(
                    fact_left + if index > 0 { 24.0 } else { 0.0 },
                    facts_top,
                    fact_left + fact_width - 18.0,
                    facts_top + 20.0,
                ),
                &self.small.clone(),
                SECONDARY,
            )?;
            self.text(
                value,
                rect(
                    fact_left + if index > 0 { 24.0 } else { 0.0 },
                    facts_top + 24.0,
                    fact_left + fact_width - 18.0,
                    facts_top + 54.0,
                ),
                &self.body.clone(),
                PRIMARY,
            )?;
        }
        self.draw_home_girl(home_girl)?;
        Ok(())
    }

    #[allow(dead_code)]
    fn render_library_legacy(
        &mut self,
        model: &UiModel,
        left: f32,
        right: f32,
        height: f32,
    ) -> Result<(), String> {
        self.page_heading("Library", "Local replays", left, right)?;
        self.selection_toolbar(model, right, true)?;
        self.text(
            &format!(
                "{} clips  •  {}",
                model.clips.len(),
                format_bytes(model.total_size_bytes())
            ),
            rect(left, 205.0, right, 229.0),
            &self.small.clone(),
            SECONDARY,
        )?;
        let indices = model.visible_clip_indices(200);
        if indices.is_empty() {
            self.empty_state(
                if model.search.value.is_empty() {
                    "No clips yet"
                } else {
                    "No matching clips"
                },
                left,
                right,
                286.0,
            )?;
            if model.collection_picker_open {
                self.render_collection_picker(model, right, 190.0)?;
            }
            return Ok(());
        }
        self.clip_grid(model, &indices, left, right, 240.0, height - 24.0)?;
        if model.collection_picker_open {
            self.render_collection_picker(model, right, 190.0)?;
        }
        Ok(())
    }

    #[allow(dead_code)]
    fn render_collections_legacy(
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
        self.selection_toolbar(model, right, false)?;
        let sidebar_width = ((right - left) * 0.24).clamp(170.0, 240.0);
        self.pill(
            rect(left, 205.0, left + sidebar_width, 249.0),
            SURFACE,
            "+ New collection",
            PRIMARY,
            Some(Action::CreateCollection),
        )?;
        if model.active_collection.is_some() {
            self.pill(
                rect(left, 255.0, left + sidebar_width, 295.0),
                SURFACE,
                "Delete collection",
                DANGER,
                Some(Action::DeleteActiveCollection),
            )?;
        }
        self.collection_row(
            "All clips",
            model.clips.len(),
            model.active_collection.is_none(),
            rect(left, 310.0, left + sidebar_width, 354.0),
            None,
            false,
        )?;
        for (index, collection) in model.collections.iter().take(8).enumerate() {
            let top = 360.0 + index as f32 * 48.0;
            self.collection_row(
                &collection.name,
                collection.clip_count,
                model.active_collection.as_ref() == Some(&collection.path),
                rect(left, top, left + sidebar_width, top + 40.0),
                Some(index),
                model
                    .clip_drag_preview
                    .as_ref()
                    .is_some_and(|drag| drag.target_collection == Some(index)),
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
            rect(content_left, 207.0, right, 241.0),
            &self.section.clone(),
            PRIMARY,
        )?;
        let indices = model.visible_clip_indices(200);
        if indices.is_empty() {
            self.empty_state("This collection is empty", content_left, right, 340.0)?;
        } else {
            self.clip_grid(model, &indices, content_left, right, 258.0, height - 24.0)?;
        }
        if model.collection_picker_open {
            self.render_collection_picker(model, right, 190.0)?;
        }
        Ok(())
    }

    #[allow(dead_code)]
    fn render_settings_legacy(
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
            let tab = rect(x, 205.0, x + tab_width, 247.0);
            let action = Action::SettingsSection(section);
            if model.settings_section == section || self.is_hovered(&action) {
                self.fill(
                    tab,
                    if model.settings_section == section {
                        ACCENT_MUTED
                    } else {
                        mix(SURFACE, ACCENT, self.hover_progress * 0.12)
                    },
                    9.0,
                )?;
                self.stroke(
                    tab,
                    if model.settings_section == section {
                        ACCENT
                    } else {
                        BORDER
                    },
                    9.0,
                    1.0,
                )?;
            } else {
                self.stroke(tab, BORDER, 9.0, 1.0)?;
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
            self.hits.push(HitRegion { rect: tab, action });
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
                    settings_row_top(0),
                    Action::ChooseDisplay,
                    SettingControl::Dropdown,
                )?;
                self.setting_row(
                    "Frame rate",
                    &format!("{} fps", model.config.capture.frames_per_second),
                    "Available rates follow the selected monitor.",
                    left,
                    right,
                    settings_row_top(1),
                    Action::ChooseFrameRate,
                    SettingControl::Dropdown,
                )?;
                self.setting_row(
                    "Capture cursor",
                    on_off(model.config.capture.cursor),
                    "Include the pointer in saved clips.",
                    left,
                    right,
                    settings_row_top(2),
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
                    settings_row_top(0),
                    Action::ChooseDuration,
                    SettingControl::Dropdown,
                )?;
                self.setting_row(
                    "Codec",
                    &format!("{:?}", model.config.capture.codec),
                    "Hardware encoder selection; Auto is recommended.",
                    left,
                    right,
                    settings_row_top(1),
                    Action::ChooseCodec,
                    SettingControl::Dropdown,
                )?;
                self.setting_row(
                    "Quality",
                    &quality_label(model.config.capture.quality),
                    "Balances image detail and replay memory.",
                    left,
                    right,
                    settings_row_top(2),
                    Action::ChooseQuality,
                    SettingControl::Dropdown,
                )?;
            }
            SettingsSection::Audio => {
                let column_gap = 12.0;
                let column_middle = (left + right) / 2.0;
                let game_right = column_middle - column_gap / 2.0;
                let microphone_left = column_middle + column_gap / 2.0;
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
                    .map_or("Windows default", |(_, name)| name.as_str());
                self.setting_row(
                    "Game audio",
                    on_off(model.config.audio.desktop),
                    "Record game and system sound.",
                    left,
                    game_right,
                    settings_row_top(0),
                    Action::ToggleDesktopAudio,
                    SettingControl::Toggle,
                )?;
                self.setting_row(
                    "Game audio level",
                    &format!("{}%", model.config.audio.desktop_gain_percent),
                    "Recording level; Windows volume stays unchanged.",
                    left,
                    game_right,
                    settings_row_top(1),
                    Action::ChooseDesktopGain,
                    SettingControl::Dropdown,
                )?;
                self.setting_row(
                    "Output device",
                    output_name,
                    "Capture this output instead of following the Windows default.",
                    left,
                    game_right,
                    settings_row_top(2),
                    Action::ChooseDesktopDevice,
                    SettingControl::Dropdown,
                )?;
                self.setting_row(
                    "Microphone",
                    on_off(model.config.audio.microphone),
                    "Capture your selected input with its own level.",
                    microphone_left,
                    right,
                    settings_row_top(0),
                    Action::ToggleMicrophone,
                    SettingControl::Toggle,
                )?;
                self.setting_row(
                    "Input device",
                    microphone_name,
                    "Choose an active Windows input.",
                    microphone_left,
                    right,
                    settings_row_top(1),
                    Action::ChooseMicrophone,
                    SettingControl::Dropdown,
                )?;
                self.setting_row(
                    "Microphone level",
                    &format!("{}%", model.config.audio.microphone_gain_percent),
                    "Recording level for your voice.",
                    microphone_left,
                    right,
                    settings_row_top(2),
                    Action::ChooseMicrophoneGain,
                    SettingControl::Dropdown,
                )?;
            }
            SettingsSection::Controls => {
                let shortcut = if model.hotkey_pending {
                    "Activating…".into()
                } else if model.hotkey_capture {
                    hotkey_capture_label(&model.hotkey_modifiers)
                } else {
                    wreath_windows::hotkey::localized_hotkey_label(&model.config.hotkey)
                };
                self.setting_row(
                    "Save replay",
                    &shortcut,
                    if model.hotkey_pending {
                        "Checking availability without restarting capture."
                    } else if let Some(error) = &model.hotkey_error {
                        error
                    } else if model.hotkey_capture {
                        "Hold Ctrl or Shift, then press one other key. F1–F24 and Print Screen work alone; Escape cancels."
                    } else if model.hotkey_deferred {
                        "Saved. It becomes active when background capture starts."
                    } else {
                        "Click to change. Use Ctrl or Shift with one other key."
                    },
                    left,
                    right - 54.0,
                    settings_row_top(0),
                    Action::CaptureHotkey,
                    SettingControl::Button,
                )?;
                let clear = rect(right - 46.0, 287.0, right, 329.0);
                self.fill(clear, SURFACE_HOVER, 9.0)?;
                self.glyph(
                    Glyph::Close,
                    rect(right - 33.0, 300.0, right - 13.0, 320.0),
                    SECONDARY,
                )?;
                self.hits.push(HitRegion {
                    rect: clear,
                    action: Action::ClearHotkey,
                });
                self.setting_row(
                    "Start with Windows",
                    on_off(model.autostart_enabled),
                    "Launch Wreath in the tray and start the replay buffer after sign-in.",
                    left,
                    right,
                    settings_row_top(1),
                    Action::ToggleAutostart,
                    SettingControl::Toggle,
                )?;
            }
            SettingsSection::Storage => {
                self.setting_row(
                    "Save location",
                    &model.config.storage.directory.display().to_string(),
                    "Choose a local folder through the Windows picker.",
                    left,
                    right,
                    settings_row_top(0),
                    Action::ChooseStorage,
                    SettingControl::Button,
                )?;
                self.setting_row(
                    "Storage limit",
                    &format_storage_limit(model.config.storage.max_megabytes),
                    "Old clips are never uploaded.",
                    left,
                    right,
                    settings_row_top(1),
                    Action::ChooseStorageLimit,
                    SettingControl::Dropdown,
                )?;
            }
        }
        if let Some(sticker) = settings_sticker_layout(left, right, height) {
            self.draw_settings_sticker(sticker)?;
        }
        self.pill(
            rect(right - 160.0, 205.0, right, 249.0),
            ACCENT,
            "Save settings",
            CANVAS,
            Some(Action::SaveSettings),
        )
    }

    #[allow(dead_code)]
    fn render_player_legacy(
        &mut self,
        model: &UiModel,
        left: f32,
        right: f32,
        height: f32,
    ) -> Result<(), String> {
        self.pill(
            rect(left, 112.0, left + 88.0, 152.0),
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
            rect(left + 108.0, 106.0, right - 300.0, 142.0),
            &self.section.clone(),
            PRIMARY,
        )?;
        self.text(
            &format!(
                "{}  •  {}",
                age(clip.modified),
                format_bytes(clip.size_bytes)
            ),
            rect(left + 108.0, 142.0, right, 166.0),
            &self.small.clone(),
            SECONDARY,
        )?;
        self.pill(
            rect(right - 274.0, 112.0, right - 142.0, 152.0),
            ACCENT,
            "Edit clip",
            CANVAS,
            Some(Action::EditActiveClip),
        )?;
        self.pill(
            rect(right - 134.0, 112.0, right, 152.0),
            SURFACE,
            "Open folder",
            PRIMARY,
            Some(Action::OpenClipsFolder),
        )?;
        let stage = fit_aspect(
            rect(
                left + 96.0,
                184.0,
                right - 96.0,
                (height - 112.0).max(330.0),
            ),
            model.player_aspect_ratio,
        );
        self.fill(stage, STAGE, 12.0)?;
        self.stroke(stage, BORDER, 12.0, 1.0)?;
        self.hits.push(HitRegion {
            rect: stage,
            action: Action::PlayPause,
        });
        let switch_top = (stage.top + stage.bottom) / 2.0 - 22.0;
        self.floating_icon(
            rect(
                stage.left - 56.0,
                switch_top,
                stage.left - 12.0,
                switch_top + 44.0,
            ),
            "‹",
            if model.adjacent_clip(-1).is_some() {
                PRIMARY
            } else {
                SECONDARY
            },
            model.adjacent_clip(-1).map(|_| Action::PreviousClip),
            FloatingIconSize::Navigation,
        )?;
        self.floating_icon(
            rect(
                stage.right + 12.0,
                switch_top,
                stage.right + 56.0,
                switch_top + 44.0,
            ),
            "›",
            if model.adjacent_clip(1).is_some() {
                PRIMARY
            } else {
                SECONDARY
            },
            model.adjacent_clip(1).map(|_| Action::NextClip),
            FloatingIconSize::Navigation,
        )?;
        let volume_x = stage.right + 78.0;
        let volume_top = switch_top - 76.0;
        let volume_bottom = switch_top + 120.0;
        self.text(
            &format!("{}%", model.player_volume_percent),
            rect(
                volume_x - 20.0,
                volume_top,
                volume_x + 20.0,
                volume_top + 28.0,
            ),
            &self.body_center.clone(),
            SECONDARY,
        )?;
        let volume_rail = rect(
            volume_x - 2.5,
            volume_top + 38.0,
            volume_x + 2.5,
            volume_bottom,
        );
        self.fill(volume_rail, SURFACE_HOVER, 2.5)?;
        let volume_fraction = f32::from(model.player_volume_percent) / 100.0;
        let volume_knob =
            volume_rail.bottom - (volume_rail.bottom - volume_rail.top) * volume_fraction;
        self.fill(
            rect(
                volume_rail.left,
                volume_knob,
                volume_rail.right,
                volume_rail.bottom,
            ),
            ACCENT,
            2.5,
        )?;
        self.fill(
            rect(
                volume_x - 6.0,
                volume_knob - 6.0,
                volume_x + 6.0,
                volume_knob + 6.0,
            ),
            PRIMARY,
            6.0,
        )?;
        self.hits.push(HitRegion {
            rect: rect(
                volume_x - 18.0,
                volume_rail.top - 8.0,
                volume_x + 18.0,
                volume_rail.bottom + 8.0,
            ),
            action: Action::DragPlayerVolume,
        });
        self.floating_icon(
            rect(
                volume_x - 19.0,
                volume_rail.bottom + 12.0,
                volume_x + 19.0,
                volume_rail.bottom + 50.0,
            ),
            if model.player_volume_percent == 0 {
                "🔇"
            } else {
                "🔊"
            },
            if model.player_volume_percent == 0 {
                PRIMARY
            } else {
                SECONDARY
            },
            Some(Action::ToggleMute),
            FloatingIconSize::Media,
        )?;
        let controls_top = height - 88.0;
        self.floating_icon(
            rect(left, controls_top, left + 42.0, controls_top + 38.0),
            if model.player_playing { "Ⅱ" } else { "▶" },
            PRIMARY,
            Some(Action::PlayPause),
            FloatingIconSize::Media,
        )?;
        let rail = rect(
            left + 58.0,
            controls_top + 16.0,
            right - 160.0,
            controls_top + 22.0,
        );
        self.fill(rail, SURFACE_HOVER, 3.0)?;
        let progress = if model.player_duration_seconds > 0.0 {
            (model.player_position_seconds / model.player_duration_seconds).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        if model.player_duration_seconds > 0.0 {
            let playhead = rail.left + (rail.right - rail.left) * progress;
            if progress > 0.0 {
                self.fill(
                    rect(rail.left, rail.top, playhead, rail.bottom),
                    ACCENT,
                    3.0,
                )?;
            }
            let center_y = (rail.top + rail.bottom) / 2.0;
            self.fill(
                rect(
                    playhead - 5.0,
                    center_y - 5.0,
                    playhead + 5.0,
                    center_y + 5.0,
                ),
                PRIMARY,
                5.0,
            )?;
        }
        self.hits.push(HitRegion {
            rect: rect(rail.left, controls_top, rail.right, controls_top + 38.0),
            action: Action::DragPlayerSeek,
        });
        self.floating_icon(
            rect(
                right - 146.0,
                controls_top,
                right - 108.0,
                controls_top + 38.0,
            ),
            "⛶",
            PRIMARY,
            Some(Action::ToggleFullscreen),
            FloatingIconSize::Fullscreen,
        )?;
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

    #[allow(dead_code)]
    fn render_editor_legacy(
        &mut self,
        model: &UiModel,
        left: f32,
        right: f32,
        height: f32,
    ) -> Result<(), String> {
        self.pill(
            rect(left, 108.0, left + 88.0, 148.0),
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
            rect(left + 108.0, 104.0, right - 378.0, 138.0),
            &self.section.clone(),
            PRIMARY,
        )?;
        self.text(
            if model.editor_loading {
                "Reading duration and keyframes…"
            } else {
                "Choose the moment to keep"
            },
            rect(left + 108.0, 138.0, right - 378.0, 162.0),
            &self.small.clone(),
            SECONDARY,
        )?;

        let actions_enabled = model.editor_timing.is_some() && !model.editor_working;
        let action_background = if actions_enabled {
            ACCENT
        } else {
            SURFACE_HOVER
        };
        let action_foreground = if actions_enabled { CANVAS } else { SECONDARY };
        self.pill(
            rect(right - 360.0, 108.0, right - 186.0, 148.0),
            action_background,
            "Save as new clip",
            action_foreground,
            actions_enabled.then_some(Action::SaveCut),
        )?;
        self.pill(
            rect(right - 174.0, 108.0, right, 148.0),
            action_background,
            "Save as original",
            action_foreground,
            actions_enabled.then_some(Action::ReplaceCut),
        )?;

        let stage = fit_aspect(
            rect(left, 176.0, right, (height - 274.0).max(330.0)),
            model.player_aspect_ratio,
        );
        self.fill(stage, STAGE, 11.0)?;
        self.stroke(stage, BORDER, 11.0, 1.0)?;
        self.hits.push(HitRegion {
            rect: stage,
            action: Action::PlayPause,
        });

        let timeline_top = (stage.bottom + 18.0).min(height - 220.0);
        let timeline = rect(left, timeline_top, right, timeline_top + 128.0);
        self.fill(timeline, SURFACE, 11.0)?;
        self.text(
            "Keep this moment",
            rect(
                left + 16.0,
                timeline_top + 10.0,
                right - 180.0,
                timeline_top + 28.0,
            ),
            &self.small.clone(),
            SECONDARY,
        )?;
        self.trim_rail(model, timeline_top + 54.0, left + 20.0, right - 20.0)?;
        self.text(
            &format!(
                "{} — {}  ·  {} kept",
                format_editor_time(model.editor_start),
                format_editor_time(model.editor_end),
                format_editor_time(model.editor_selected_duration())
            ),
            rect(
                left + 20.0,
                timeline_top + 90.0,
                right - 20.0,
                timeline_top + 118.0,
            ),
            &self.body_center.clone(),
            PRIMARY,
        )?;

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
            rect(left, timeline.bottom + 14.0, right, timeline.bottom + 50.0),
            &self.small.clone(),
            SECONDARY,
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
        self.hits.push(HitRegion {
            rect: rect(rail.left, top - 10.0, rail.right, top + 34.0),
            action: Action::DragEditorPlayhead,
        });
        if duration > 0.0 {
            if let Some(timing) = &model.editor_timing {
                let stride = timing.keyframes.len().div_ceil(80).max(1);
                for keyframe in timing.keyframes.iter().step_by(stride) {
                    let fraction = (keyframe.as_secs_f64() / duration).clamp(0.0, 1.0) as f32;
                    let x = rail.left + (rail.right - rail.left) * fraction;
                    self.fill(rect(x, top + 22.0, x + 1.0, top + 27.0), ACCENT, 0.0)?;
                }
            }
            let start_fraction =
                (model.editor_start.as_secs_f64() / duration).clamp(0.0, 1.0) as f32;
            let end_fraction = (model.editor_end.as_secs_f64() / duration).clamp(0.0, 1.0) as f32;
            let start_x = rail.left + (rail.right - rail.left) * start_fraction;
            let end_x = rail.left + (rail.right - rail.left) * end_fraction;
            self.fill(rect(start_x, rail.top, end_x, rail.bottom), ACCENT, 0.0)?;
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

    #[allow(clippy::too_many_arguments)]
    fn sidebar_item(
        &mut self,
        rail: f32,
        top: f32,
        glyph: Glyph,
        label: &str,
        active: bool,
        action: Action,
        expanded: bool,
    ) -> Result<(), String> {
        let area = if expanded {
            rect(16.0, top, rail - 16.0, top + 50.0)
        } else {
            rect((rail - 50.0) / 2.0, top, (rail + 50.0) / 2.0, top + 50.0)
        };
        if active || self.is_hovered(&action) {
            self.fill(
                area,
                if active {
                    ACCENT_MUTED
                } else {
                    mix(SURFACE_HOVER, ACCENT, self.hover_progress * 0.08)
                },
                9.0,
            )?;
        }
        self.glyph(
            glyph,
            rect(
                area.left + 17.0,
                area.top + 14.0,
                area.left + 41.0,
                area.bottom - 12.0,
            ),
            if active { PRIMARY } else { SECONDARY },
        )?;
        if expanded {
            self.text(
                label,
                rect(area.left + 61.0, area.top, area.right - 12.0, area.bottom),
                &self.body.clone(),
                if active { PRIMARY } else { SECONDARY },
            )?;
        }
        self.hits.push(HitRegion { rect: area, action });
        Ok(())
    }

    fn draw_wreath_logo(&self, area: LogicalRect, fill: u32) -> Result<(), String> {
        use windows::Win32::Graphics::Direct2D::ID2D1StrokeStyle;
        let target = self.target.as_ref().expect("render target exists");
        let outline = unsafe { target.CreateSolidColorBrush(&color(SURFACE_RAISED), None) }
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
            if enabled { PRIMARY } else { SURFACE_HOVER },
            (area.bottom - area.top) / 2.0,
        )?;
        self.stroke(
            area,
            if enabled { PRIMARY } else { SECONDARY },
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
            if enabled { CANVAS } else { SECONDARY },
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
                SURFACE_HOVER
            } else {
                SURFACE
            },
            9.0,
        )?;
        self.stroke(area, BORDER, 9.0, 1.0)?;
        self.glyph(
            glyph,
            rect(
                area.left + 13.0,
                area.top + 11.0,
                area.right - 13.0,
                area.bottom - 11.0,
            ),
            PRIMARY,
        )?;
        self.hits.push(HitRegion { rect: area, action });
        Ok(())
    }

    fn quick_setup_card(
        &mut self,
        area: LogicalRect,
        glyph: Glyph,
        title: &str,
        value: &str,
        description: &str,
        action: Action,
    ) -> Result<(), String> {
        let hovered = self.is_hovered(&action);
        self.fill(area, if hovered { SURFACE_RAISED } else { SURFACE }, 10.0)?;
        self.stroke(area, if hovered { SECONDARY } else { BORDER }, 10.0, 1.0)?;
        self.glyph(
            glyph,
            rect(
                area.left + 22.0,
                area.top + 22.0,
                area.left + 58.0,
                area.top + 58.0,
            ),
            SECONDARY,
        )?;
        self.glyph(
            Glyph::ChevronDown,
            rect(
                area.right - 38.0,
                area.top + 29.0,
                area.right - 22.0,
                area.top + 45.0,
            ),
            SECONDARY,
        )?;
        self.text(
            title,
            rect(
                area.left + 22.0,
                area.top + 72.0,
                area.right - 18.0,
                area.top + 98.0,
            ),
            &self.small.clone(),
            SECONDARY,
        )?;
        self.text(
            value,
            rect(
                area.left + 22.0,
                area.top + 99.0,
                area.right - 18.0,
                area.top + 130.0,
            ),
            &self.section.clone(),
            PRIMARY,
        )?;
        self.fill(
            rect(
                area.left + 22.0,
                area.top + 140.0,
                area.right - 22.0,
                area.top + 141.0,
            ),
            BORDER,
            0.0,
        )?;
        self.text(
            description,
            rect(
                area.left + 22.0,
                area.top + 150.0,
                area.right - 18.0,
                area.bottom - 10.0,
            ),
            &self.small.clone(),
            SECONDARY,
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
        self.fill(area, SURFACE, 9.0)?;
        self.stroke(
            area,
            if model.search_focused {
                PRIMARY
            } else {
                BORDER
            },
            9.0,
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
            SECONDARY,
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

    #[allow(clippy::too_many_arguments)]
    fn collection_card(
        &mut self,
        area: LogicalRect,
        title: &str,
        description: &str,
        count: usize,
        glyph: Glyph,
        active: bool,
        action: Action,
    ) -> Result<(), String> {
        self.fill(area, if active { SURFACE_RAISED } else { SURFACE }, 9.0)?;
        self.stroke(
            area,
            if active { PRIMARY } else { BORDER },
            9.0,
            if active { 1.5 } else { 1.0 },
        )?;
        let icon = rect(
            area.left + 16.0,
            area.top + 16.0,
            area.left + 64.0,
            area.top + 64.0,
        );
        self.fill(icon, STAGE, 8.0)?;
        self.stroke(icon, BORDER, 8.0, 1.0)?;
        self.glyph(
            glyph,
            rect(
                icon.left + 13.0,
                icon.top + 13.0,
                icon.right - 13.0,
                icon.bottom - 13.0,
            ),
            PRIMARY,
        )?;
        self.text(
            title,
            rect(
                area.left + 78.0,
                area.top + 14.0,
                area.right - 18.0,
                area.top + 42.0,
            ),
            &self.section.clone(),
            PRIMARY,
        )?;
        self.text(
            description,
            rect(
                area.left + 78.0,
                area.top + 43.0,
                area.right - 18.0,
                area.top + 68.0,
            ),
            &self.small.clone(),
            SECONDARY,
        )?;
        if count > 0 || title == "Alle Clips" {
            self.text(
                &format!("{count} Clips"),
                rect(
                    area.left + 16.0,
                    area.bottom - 34.0,
                    area.right - 45.0,
                    area.bottom - 8.0,
                ),
                &self.small.clone(),
                SECONDARY,
            )?;
            self.glyph(
                Glyph::More,
                rect(
                    area.right - 36.0,
                    area.bottom - 31.0,
                    area.right - 16.0,
                    area.bottom - 11.0,
                ),
                SECONDARY,
            )?;
        }
        self.hits.push(HitRegion { rect: area, action });
        Ok(())
    }

    fn collection_table(
        &mut self,
        model: &UiModel,
        left: f32,
        right: f32,
        top: f32,
        bottom: f32,
    ) -> Result<(), String> {
        let area = rect(left, top, right, bottom);
        self.fill(area, SURFACE, 9.0)?;
        self.stroke(area, BORDER, 9.0, 1.0)?;
        self.text(
            "VORSCHAU",
            rect(left + 24.0, top + 4.0, left + 190.0, top + 47.0),
            &self.small.clone(),
            SECONDARY,
        )?;
        self.text(
            "TITEL",
            rect(left + 212.0, top + 4.0, right - 330.0, top + 47.0),
            &self.small.clone(),
            SECONDARY,
        )?;
        self.text(
            "ERSTELLT",
            rect(right - 258.0, top + 4.0, right - 70.0, top + 47.0),
            &self.small.clone(),
            SECONDARY,
        )?;
        self.fill(rect(left, top + 47.0, right, top + 48.0), BORDER, 0.0)?;
        let indices = model.visible_clip_indices(usize::MAX);
        let available_rows = (((bottom - top - 94.0) / 71.0).floor() as usize).max(1);
        let total_pages = indices.len().div_ceil(available_rows).max(1);
        let page = model
            .collection_clips_page
            .min(total_pages.saturating_sub(1));
        let start = page * available_rows;
        let page_indices = &indices[start..(start + available_rows).min(indices.len())];
        for (row, index) in page_indices.iter().copied().enumerate() {
            let row_top = top + 48.0 + row as f32 * 71.0;
            if row_top + 71.0 > bottom {
                break;
            }
            if row > 0 {
                self.fill(rect(left, row_top, right, row_top + 1.0), BORDER, 0.0)?;
            }
            let clip = &model.clips[index];
            let preview = rect(left + 24.0, row_top + 9.0, left + 154.0, row_top + 61.0);
            self.fill(preview, STAGE, 6.0)?;
            let _ = self.draw_thumbnail(&clip.path, preview)?;
            self.text(
                &clip.title,
                rect(left + 212.0, row_top, right - 300.0, row_top + 71.0),
                &self.body.clone(),
                PRIMARY,
            )?;
            self.text(
                &age(clip.modified),
                rect(right - 258.0, row_top, right - 70.0, row_top + 71.0),
                &self.small.clone(),
                SECONDARY,
            )?;
            self.glyph(
                Glyph::More,
                rect(right - 46.0, row_top + 25.0, right - 24.0, row_top + 47.0),
                SECONDARY,
            )?;
            self.hits.push(HitRegion {
                rect: rect(left, row_top, right, row_top + 71.0),
                action: Action::OpenClip(index),
            });
            self.hits.push(HitRegion {
                rect: rect(right - 58.0, row_top + 12.0, right, row_top + 59.0),
                action: Action::OpenClipMenu(index),
            });
        }
        self.pagination(
            (left + right) / 2.0,
            bottom - 42.0,
            page,
            total_pages,
            PaginationKind::CollectionClips,
        )?;
        Ok(())
    }

    fn settings_panel(&self, area: LogicalRect, title: &str) -> Result<(), String> {
        self.fill(area, SURFACE, 10.0)?;
        self.stroke(area, BORDER, 10.0, 1.0)?;
        self.text(
            title,
            rect(
                area.left + 20.0,
                area.top + 8.0,
                area.right - 20.0,
                area.top + 48.0,
            ),
            &self.body.clone(),
            PRIMARY,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn settings_compact_row(
        &mut self,
        panel: LogicalRect,
        index: usize,
        title: &str,
        description: &str,
        value: &str,
        action: Action,
        control: SettingControl,
    ) -> Result<(), String> {
        let row_height = compact_settings_row_height(panel);
        let top = panel.top + 44.0 + index as f32 * row_height;
        if index > 0 {
            self.fill(
                rect(panel.left + 20.0, top, panel.right - 20.0, top + 1.0),
                BORDER,
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
        self.text(
            title,
            rect(
                panel.left + 20.0,
                top + 2.0,
                control_area.left - 16.0,
                top + row_height * 0.50,
            ),
            &self.body.clone(),
            PRIMARY,
        )?;
        self.text(
            description,
            rect(
                panel.left + 20.0,
                top + row_height * 0.45,
                control_area.left - 16.0,
                top + row_height,
            ),
            &self.small.clone(),
            SECONDARY,
        )?;
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
                self.fill(control_area, STAGE, 7.0)?;
                self.stroke(control_area, BORDER, 7.0, 1.0)?;
                let clipped = ellipsize(value, ((control_width - 50.0) / 6.4) as usize);
                self.text(
                    &clipped,
                    rect(
                        control_area.left + 12.0,
                        control_area.top,
                        control_area.right - 34.0,
                        control_area.bottom,
                    ),
                    &self.small.clone(),
                    PRIMARY,
                )?;
                self.glyph(
                    Glyph::ChevronDown,
                    rect(
                        control_area.right - 27.0,
                        control_area.top + 10.0,
                        control_area.right - 11.0,
                        control_area.bottom - 10.0,
                    ),
                    SECONDARY,
                )?;
            }
            SettingControl::Button => {
                self.fill(control_area, STAGE, 7.0)?;
                self.stroke(control_area, BORDER, 7.0, 1.0)?;
                let clipped = ellipsize(value, ((control_width - 32.0) / 6.4) as usize);
                self.text(
                    &clipped,
                    rect(
                        control_area.left + 10.0,
                        control_area.top,
                        control_area.right - 10.0,
                        control_area.bottom,
                    ),
                    &self.small.clone(),
                    PRIMARY,
                )?;
            }
        }
        self.hits.push(HitRegion {
            rect: control_area,
            action,
        });
        Ok(())
    }

    fn settings_gain_slider(
        &mut self,
        panel: LogicalRect,
        index: usize,
        title: &str,
        description: &str,
        value: u16,
        action: Action,
    ) -> Result<(), String> {
        let row_height = compact_settings_row_height(panel);
        let top = panel.top + 44.0 + index as f32 * row_height;
        if index > 0 {
            self.fill(
                rect(panel.left + 20.0, top, panel.right - 20.0, top + 1.0),
                BORDER,
                0.0,
            )?;
        }
        let control_area = settings_control_area(panel, index);
        self.text(
            title,
            rect(
                panel.left + 20.0,
                top + 2.0,
                control_area.left - 16.0,
                top + row_height * 0.50,
            ),
            &self.body.clone(),
            PRIMARY,
        )?;
        self.text(
            description,
            rect(
                panel.left + 20.0,
                top + row_height * 0.45,
                control_area.left - 16.0,
                top + row_height,
            ),
            &self.small.clone(),
            SECONDARY,
        )?;

        let rail = settings_gain_rail_in_panel(panel, index);
        let fraction = f32::from(value.min(200)) / 200.0;
        let knob_x = rail.left + (rail.right - rail.left) * fraction;
        self.fill(rail, BORDER, 2.0)?;
        if knob_x > rail.left {
            self.fill(rect(rail.left, rail.top, knob_x, rail.bottom), PRIMARY, 2.0)?;
        }
        let knob_size = if self.is_hovered(&action) { 12.0 } else { 10.0 };
        self.fill(
            rect(
                knob_x - knob_size / 2.0,
                (rail.top + rail.bottom - knob_size) / 2.0,
                knob_x + knob_size / 2.0,
                (rail.top + rail.bottom + knob_size) / 2.0,
            ),
            PRIMARY,
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
            PRIMARY,
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
            PRIMARY,
        )?;
        self.text(
            subtitle,
            rect(left, 99.0, right, 128.0),
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
        let columns = if width >= 900.0 {
            4
        } else if width >= 650.0 {
            3
        } else if width >= 450.0 {
            2
        } else {
            1
        };
        let gap = 18.0;
        let card_width = (width - gap * (columns - 1) as f32) / columns as f32;
        let preview_height = card_width * 9.0 / 16.0;
        let card_height = preview_height + 66.0;
        for (position, index) in indices.iter().enumerate() {
            let row = position / columns;
            let column = position % columns;
            let y = top + row as f32 * (card_height + gap);
            if y + card_height > bottom {
                break;
            }
            let x = left + column as f32 * (card_width + gap);
            let card = rect(x, y, x + card_width, y + card_height);
            let action = if model.selection_mode {
                Action::ToggleClipSelection(*index)
            } else {
                Action::OpenClip(*index)
            };
            let selected = model.clip_is_selected(*index);
            let dragged = model
                .clip_drag_preview
                .as_ref()
                .is_some_and(|drag| drag.clip == *index || selected);
            self.fill(
                card,
                if selected {
                    mix(SURFACE, ACCENT, 0.10)
                } else if self.is_hovered(&action) {
                    mix(SURFACE, ACCENT, self.hover_progress * 0.065)
                } else {
                    SURFACE
                },
                10.0,
            )?;
            self.stroke(
                card,
                if dragged {
                    READY
                } else if selected {
                    ACCENT
                } else if self.is_hovered(&action) {
                    mix(BORDER, ACCENT, self.hover_progress * 0.55)
                } else {
                    BORDER
                },
                10.0,
                if selected || dragged { 2.0 } else { 1.0 },
            )?;
            let preview = rect(x + 1.0, y + 1.0, x + card_width - 1.0, y + preview_height);
            self.fill(preview, STAGE, 9.0)?;
            let clip = &model.clips[*index];
            if !self.draw_thumbnail(&clip.path, preview)? {
                self.text(
                    "▶",
                    rect(
                        x + 14.0,
                        preview.top + 12.0,
                        x + card_width,
                        preview.top + 38.0,
                    ),
                    &self.section.clone(),
                    SECONDARY,
                )?;
            }
            if let Some(duration) = self.clip_duration(&clip.path) {
                let label = format_clip_badge_duration(duration);
                let badge_width = 18.0 + label.chars().count() as f32 * 7.0;
                let badge = rect(
                    preview.left + 9.0,
                    preview.bottom - 29.0,
                    preview.left + 9.0 + badge_width,
                    preview.bottom - 7.0,
                );
                self.fill(badge, 0xDB08090A, 5.0)?;
                self.text(&label, badge, &self.body_center.clone(), PRIMARY)?;
            }
            if model.selection_mode {
                let check = rect(
                    preview.right - 32.0,
                    preview.top + 8.0,
                    preview.right - 8.0,
                    preview.top + 32.0,
                );
                self.fill(check, if selected { ACCENT } else { SURFACE_RAISED }, 12.0)?;
                self.stroke(check, if selected { ACCENT } else { BORDER }, 12.0, 1.0)?;
                if selected {
                    self.text("✓", check, &self.body_center.clone(), CANVAS)?;
                }
            }
            self.text(
                &clip.title,
                rect(
                    x + 12.0,
                    y + preview_height + 7.0,
                    x + card_width - 8.0,
                    y + preview_height + 31.0,
                ),
                &self.body.clone(),
                PRIMARY,
            )?;
            self.text(
                &format!(
                    "{}  •  {}",
                    age(clip.modified),
                    format_bytes(clip.size_bytes)
                ),
                rect(
                    x + 12.0,
                    y + preview_height + 31.0,
                    x + card_width - 38.0,
                    y + preview_height + 59.0,
                ),
                &self.small.clone(),
                SECONDARY,
            )?;
            self.glyph(
                Glyph::More,
                rect(
                    x + card_width - 30.0,
                    y + preview_height + 31.0,
                    x + card_width - 10.0,
                    y + preview_height + 53.0,
                ),
                SECONDARY,
            )?;
            self.hits.push(HitRegion { rect: card, action });
            if !model.selection_mode {
                self.hits.push(HitRegion {
                    rect: rect(
                        x + card_width - 42.0,
                        y + preview_height + 25.0,
                        x + card_width,
                        y + card_height,
                    ),
                    action: Action::OpenClipMenu(*index),
                });
            }
        }
        Ok(())
    }

    fn clip_list(
        &mut self,
        model: &UiModel,
        indices: &[usize],
        left: f32,
        right: f32,
        top: f32,
        bottom: f32,
    ) -> Result<(), String> {
        self.fill(rect(left, top, right, bottom), SURFACE, 9.0)?;
        self.stroke(rect(left, top, right, bottom), BORDER, 9.0, 1.0)?;
        self.text(
            "VORSCHAU",
            rect(left + 24.0, top + 4.0, left + 190.0, top + 47.0),
            &self.small.clone(),
            SECONDARY,
        )?;
        self.text(
            "TITEL",
            rect(left + 212.0, top + 4.0, right - 330.0, top + 47.0),
            &self.small.clone(),
            SECONDARY,
        )?;
        self.text(
            "ERSTELLT",
            rect(right - 258.0, top + 4.0, right - 70.0, top + 47.0),
            &self.small.clone(),
            SECONDARY,
        )?;
        self.fill(rect(left, top + 47.0, right, top + 48.0), BORDER, 0.0)?;
        for (row, index) in indices.iter().copied().enumerate() {
            let row_top = top + 48.0 + row as f32 * 71.0;
            let clip = &model.clips[index];
            if row > 0 {
                self.fill(rect(left, row_top, right, row_top + 1.0), BORDER, 0.0)?;
            }
            let preview = rect(left + 24.0, row_top + 9.0, left + 154.0, row_top + 61.0);
            self.fill(preview, STAGE, 6.0)?;
            let _ = self.draw_thumbnail(&clip.path, preview)?;
            self.text(
                &clip.title,
                rect(left + 212.0, row_top, right - 300.0, row_top + 71.0),
                &self.body.clone(),
                PRIMARY,
            )?;
            self.text(
                &age(clip.modified),
                rect(right - 258.0, row_top, right - 70.0, row_top + 71.0),
                &self.small.clone(),
                SECONDARY,
            )?;
            self.glyph(
                Glyph::More,
                rect(right - 46.0, row_top + 25.0, right - 24.0, row_top + 47.0),
                SECONDARY,
            )?;
            let action = if model.selection_mode {
                Action::ToggleClipSelection(index)
            } else {
                Action::OpenClip(index)
            };
            self.hits.push(HitRegion {
                rect: rect(left, row_top, right, row_top + 71.0),
                action,
            });
            if !model.selection_mode {
                self.hits.push(HitRegion {
                    rect: rect(right - 58.0, row_top + 12.0, right, row_top + 59.0),
                    action: Action::OpenClipMenu(index),
                });
            }
        }
        Ok(())
    }

    fn pagination(
        &mut self,
        center: f32,
        top: f32,
        page: usize,
        total_pages: usize,
        kind: PaginationKind,
    ) -> Result<(), String> {
        if total_pages <= 1 {
            return Ok(());
        }
        let previous = page.saturating_sub(1);
        let next = (page + 1).min(total_pages - 1);
        let action = |target| match kind {
            PaginationKind::Library => Action::SetLibraryPage(target),
            PaginationKind::CollectionCards => Action::SetCollectionCardsPage(target),
            PaginationKind::CollectionClips => Action::SetCollectionClipsPage(target),
        };
        let area = rect(center - 106.0, top, center + 106.0, top + 42.0);
        self.fill(area, SURFACE, 8.0)?;
        self.stroke(area, BORDER, 8.0, 1.0)?;
        self.pill(
            rect(
                area.left + 7.0,
                area.top + 6.0,
                area.left + 43.0,
                area.bottom - 6.0,
            ),
            if page > 0 { SURFACE_RAISED } else { SURFACE },
            "‹",
            if page > 0 { PRIMARY } else { SECONDARY },
            (page > 0).then_some(action(previous)),
        )?;
        self.text(
            &format!("{} / {}", page + 1, total_pages),
            rect(area.left + 50.0, area.top, area.right - 50.0, area.bottom),
            &self.body_center.clone(),
            PRIMARY,
        )?;
        self.pill(
            rect(
                area.right - 43.0,
                area.top + 6.0,
                area.right - 7.0,
                area.bottom - 6.0,
            ),
            if page + 1 < total_pages {
                SURFACE_RAISED
            } else {
                SURFACE
            },
            "›",
            if page + 1 < total_pages {
                PRIMARY
            } else {
                SECONDARY
            },
            (page + 1 < total_pages).then_some(action(next)),
        )
    }

    fn collection_row(
        &mut self,
        name: &str,
        count: usize,
        active: bool,
        area: LogicalRect,
        index: Option<usize>,
        drop_target: bool,
    ) -> Result<(), String> {
        let action = Action::SelectCollection(index);
        if active || drop_target || self.is_hovered(&action) {
            self.fill(
                area,
                if drop_target {
                    mix(SURFACE, READY, 0.18)
                } else if active {
                    ACCENT_MUTED
                } else {
                    mix(SURFACE, ACCENT, self.hover_progress * 0.10)
                },
                9.0,
            )?;
            self.stroke(
                area,
                if drop_target {
                    READY
                } else if active {
                    ACCENT
                } else {
                    BORDER
                },
                9.0,
                if drop_target { 2.0 } else { 1.0 },
            )?;
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
        self.hits.push(HitRegion { rect: area, action });
        Ok(())
    }

    fn selection_toolbar(
        &mut self,
        model: &UiModel,
        right: f32,
        include_refresh: bool,
    ) -> Result<(), String> {
        let top = 141.0;
        let bottom = 181.0;
        if !model.selection_mode {
            if include_refresh {
                self.pill(
                    rect(right - 112.0, top, right, bottom),
                    SURFACE,
                    "Refresh",
                    PRIMARY,
                    Some(Action::Refresh),
                )?;
            }
            let button_right = if include_refresh {
                right - 122.0
            } else {
                right
            };
            return self.pill(
                rect(button_right - 124.0, top, button_right, bottom),
                SURFACE,
                "Select clips",
                PRIMARY,
                Some(Action::ToggleSelectionMode),
            );
        }

        let selected = model.selected_clips.len();
        self.pill(
            rect(right - 390.0, top, right - 274.0, bottom),
            SURFACE,
            "Cancel",
            SECONDARY,
            Some(Action::ToggleSelectionMode),
        )?;
        self.pill(
            rect(right - 264.0, top, right - 138.0, bottom),
            SURFACE,
            "Select all",
            PRIMARY,
            Some(Action::SelectAllVisibleClips),
        )?;
        self.pill(
            rect(right - 128.0, top, right, bottom),
            if selected > 0 { ACCENT } else { SURFACE },
            &format!("Move ({selected})"),
            if selected > 0 { CANVAS } else { SECONDARY },
            (selected > 0 && !model.collections.is_empty())
                .then_some(Action::ToggleCollectionPicker),
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
        self.fill(area, SURFACE_RAISED, 10.0)?;
        self.stroke(area, BORDER, 10.0, 1.0)?;
        self.text(
            "Move selected clips to",
            rect(
                area.left + 14.0,
                area.top + 4.0,
                area.right - 14.0,
                area.top + 44.0,
            ),
            &self.small.clone(),
            SECONDARY,
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
                self.fill(row, mix(SURFACE, ACCENT, self.hover_progress * 0.12), 7.0)?;
            }
            self.text(
                &collection.name,
                rect(row.left + 10.0, row.top, row.right - 42.0, row.bottom),
                &self.body.clone(),
                PRIMARY,
            )?;
            self.text(
                &collection.clip_count.to_string(),
                rect(row.right - 32.0, row.top, row.right - 8.0, row.bottom),
                &self.small.clone(),
                SECONDARY,
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
            self.text(placeholder, field, &body, SECONDARY)?;
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
                        SELECTION,
                        3.0,
                    )?;
                }
            }
            self.text(&input.value, field, &body, PRIMARY)?;
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
                PRIMARY,
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
            CANVAS,
            0.48,
            11.0,
        )?;
        self.fill(menu, SURFACE_RAISED, 10.0)?;
        self.stroke(menu, BORDER, 10.0, 1.0)?;
        self.stroke(anchor, ACCENT, 9.0, 1.0)?;

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
                        ACCENT_MUTED
                    } else {
                        mix(SURFACE_HOVER, ACCENT, self.hover_progress * 0.10)
                    },
                    7.0,
                )?;
            }
            if selected {
                if columns > 1 {
                    self.stroke(item_area, ACCENT, 7.0, 1.0)?;
                } else {
                    self.fill(
                        rect(
                            item_area.left + 6.0,
                            item_area.top + 11.0,
                            item_area.left + 9.0,
                            item_area.bottom - 11.0,
                        ),
                        ACCENT,
                        1.5,
                    )?;
                }
            }
            if columns > 1 {
                self.text(&item.label, item_area, &self.body_center.clone(), PRIMARY)?;
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
                    PRIMARY,
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
                    SECONDARY,
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
                    PRIMARY,
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
        let menu_height = 66.0 + (3 + collection_rows) as f32 * 44.0 + 18.0;
        let left = context.x.min(width - menu_width - 16.0).max(16.0);
        let top = context.y.min(height - menu_height - 16.0).max(16.0);
        let menu = rect(left, top, left + menu_width, top + menu_height);
        self.fill(menu, SURFACE_RAISED, 10.0)?;
        self.stroke(menu, BORDER, 10.0, 1.0)?;
        self.text(
            "Clip-Aktionen",
            rect(left + 16.0, top + 12.0, menu.right - 16.0, top + 30.0),
            &self.small.clone(),
            SECONDARY,
        )?;
        self.text(
            &clip.title,
            rect(left + 16.0, top + 31.0, menu.right - 16.0, top + 58.0),
            &self.body.clone(),
            PRIMARY,
        )?;

        let mut row_top = top + 66.0;
        self.context_menu_row(
            rect(left + 8.0, row_top, menu.right - 8.0, row_top + 40.0),
            "Clip bearbeiten",
            Action::EditClip(context.clip),
            false,
        )?;
        row_top += 44.0;
        self.context_menu_row(
            rect(left + 8.0, row_top, menu.right - 8.0, row_top + 40.0),
            "Umbenennen",
            Action::RenameClip(context.clip),
            false,
        )?;
        row_top += 44.0;

        if visible_collections > 0 {
            self.text(
                "In Sammlung verschieben",
                rect(
                    left + 16.0,
                    row_top + 4.0,
                    menu.right - 16.0,
                    row_top + 28.0,
                ),
                &self.small.clone(),
                SECONDARY,
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
            BORDER,
            0.0,
        )?;
        row_top += 6.0;
        self.context_menu_row(
            rect(left + 8.0, row_top, menu.right - 8.0, row_top + 40.0),
            "Clip löschen",
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
                    mix(SURFACE_HOVER, DANGER, 0.28)
                } else {
                    mix(SURFACE_HOVER, ACCENT, self.hover_progress * 0.10)
                },
                9.0,
            )?;
        }
        self.text(
            label,
            rect(area.left + 12.0, area.top, area.right - 12.0, area.bottom),
            &self.body.clone(),
            if dangerous { DANGER } else { PRIMARY },
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
        self.fill(modal, SURFACE_RAISED, 12.0)?;
        self.stroke(modal, BORDER, 12.0, 1.0)?;
        let (title, detail, confirmation) = match target {
            DeleteTarget::Clip(index) => {
                let name = model
                    .clips
                    .get(*index)
                    .map_or("this clip", |clip| clip.title.as_str());
                (
                    "Clip löschen?",
                    format!(
                        "{name} wird dauerhaft entfernt. Das kann nicht rückgängig gemacht werden."
                    ),
                    "Clip löschen",
                )
            }
            DeleteTarget::Collection(path) => {
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("this collection");
                (
                    "Sammlung löschen?",
                    format!(
                        "{name} wird entfernt; enthaltene Clips kommen zurück in die Bibliothek."
                    ),
                    "Sammlung löschen",
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
            "Abbrechen",
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
            DANGER,
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
        self.fill(modal, SURFACE_RAISED, 12.0)?;
        self.stroke(modal, BORDER, 12.0, 1.0)?;
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
        self.stroke(field, ACCENT, 10.0, 1.0)?;
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
            "Strg+A alles wählen · Strg+C/X/V · Enter bestätigen · Esc abbrechen",
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
            "Abbrechen",
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
            ACCENT,
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
        let area = rect(left, top, right, top + SETTINGS_ROW_HEIGHT);
        self.fill(
            area,
            if self.is_hovered(&action) {
                mix(SURFACE, ACCENT, self.hover_progress * 0.055)
            } else {
                SURFACE
            },
            11.0,
        )?;
        self.stroke(area, BORDER, 11.0, 1.0)?;
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
                ACCENT_MUTED
            } else if self.is_hovered(&action) {
                mix(SURFACE_HOVER, ACCENT, self.hover_progress * 0.13)
            } else {
                SURFACE_HOVER
            },
            9.0,
        )?;
        self.stroke(
            control_area,
            if enabled_toggle { ACCENT } else { BORDER },
            9.0,
            1.0,
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
            SettingControl::Button => self.text(
                value,
                rect(
                    control_area.left + 10.0,
                    control_area.top,
                    control_area.right - 10.0,
                    control_area.bottom,
                ),
                &self.body_center.clone(),
                PRIMARY,
            )?,
            SettingControl::Toggle => {
                self.text(value, control_area, &self.body_center.clone(), PRIMARY)?
            }
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
        let hovered = action
            .as_ref()
            .is_some_and(|candidate| self.is_hovered(candidate));
        let is_accent = background == ACCENT;
        let rendered_background = if hovered {
            if is_accent {
                mix(ACCENT, ACCENT_HOVER, self.hover_progress)
            } else {
                mix(background, ACCENT, self.hover_progress * 0.10)
            }
        } else {
            background
        };
        self.fill(area, rendered_background, 8.0)?;
        if !is_accent {
            self.stroke(area, BORDER, 8.0, 1.0)?;
        }
        self.text(label, area, &self.body_center.clone(), foreground)?;
        if let Some(action) = action {
            self.hits.push(HitRegion { rect: area, action });
        }
        Ok(())
    }

    /// Only the glyph moves on hover; the hit target keeps its full size.
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
        let progress = if hovered { self.hover_progress } else { 0.0 };
        let lift = progress * 2.5;
        let label_area = rect(area.left, area.top - lift, area.right, area.bottom - lift);
        let color = mix(foreground, PRIMARY, progress * 0.72);
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
            mix(foreground, PRIMARY, self.hover_progress * 0.72)
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
        let rounded = |left: f32, top: f32, right: f32, bottom: f32, radius: f32| unsafe {
            target.DrawRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: point(left, top).X,
                        top: point(left, top).Y,
                        right: point(right, bottom).X,
                        bottom: point(right, bottom).Y,
                    },
                    radiusX: radius,
                    radiusY: radius,
                },
                &brush,
                stroke,
                None::<&ID2D1StrokeStyle>,
            );
        };
        let dot = |x: f32, y: f32| unsafe {
            target.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: point(x - 1.0, y - 1.0).X,
                        top: point(x - 1.0, y - 1.0).Y,
                        right: point(x + 1.0, y + 1.0).X,
                        bottom: point(x + 1.0, y + 1.0).Y,
                    },
                    radiusX: 0.7,
                    radiusY: 0.7,
                },
                &brush,
            );
        };
        match glyph {
            Glyph::Home => {
                line(3.5, 11.0, 12.0, 4.0);
                line(12.0, 4.0, 20.5, 11.0);
                line(5.5, 9.5, 5.5, 20.0);
                line(5.5, 20.0, 18.5, 20.0);
                line(18.5, 20.0, 18.5, 9.5);
                line(10.0, 20.0, 10.0, 14.0);
                line(10.0, 14.0, 14.0, 14.0);
                line(14.0, 14.0, 14.0, 20.0);
            }
            Glyph::Library => {
                rounded(3.0, 4.0, 21.0, 20.0, 2.5);
                line(7.5, 4.5, 7.5, 19.5);
                line(16.5, 4.5, 16.5, 19.5);
                for y in [7.0, 12.0, 17.0] {
                    dot(5.25, y);
                    dot(18.75, y);
                }
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
                unsafe {
                    target.DrawEllipse(
                        &D2D1_ELLIPSE {
                            point: point(12.0, 12.0),
                            radiusX: width * 0.29,
                            radiusY: height * 0.29,
                        },
                        &brush,
                        stroke,
                        None::<&ID2D1StrokeStyle>,
                    );
                    target.DrawEllipse(
                        &D2D1_ELLIPSE {
                            point: point(12.0, 12.0),
                            radiusX: width * 0.095,
                            radiusY: height * 0.095,
                        },
                        &brush,
                        stroke,
                        None::<&ID2D1StrokeStyle>,
                    );
                }
                for (x1, y1, x2, y2) in [
                    (12.0, 2.0, 12.0, 5.0),
                    (12.0, 19.0, 12.0, 22.0),
                    (2.0, 12.0, 5.0, 12.0),
                    (19.0, 12.0, 22.0, 12.0),
                    (4.9, 4.9, 7.0, 7.0),
                    (17.0, 17.0, 19.1, 19.1),
                    (19.1, 4.9, 17.0, 7.0),
                    (7.0, 17.0, 4.9, 19.1),
                ] {
                    line(x1, y1, x2, y2);
                }
            }
            Glyph::Sliders => {
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
            Glyph::Folder => {
                line(3.0, 8.0, 9.0, 8.0);
                line(9.0, 8.0, 11.0, 5.0);
                line(11.0, 5.0, 19.0, 5.0);
                line(19.0, 5.0, 21.0, 8.0);
                line(21.0, 8.0, 21.0, 19.0);
                line(21.0, 19.0, 3.0, 19.0);
                line(3.0, 19.0, 3.0, 8.0);
            }
            Glyph::Record => unsafe {
                target.DrawEllipse(
                    &D2D1_ELLIPSE {
                        point: point(12.0, 12.0),
                        radiusX: width * 0.36,
                        radiusY: height * 0.36,
                    },
                    &brush,
                    stroke,
                    None::<&ID2D1StrokeStyle>,
                );
                target.FillEllipse(
                    &D2D1_ELLIPSE {
                        point: point(12.0, 12.0),
                        radiusX: width * 0.12,
                        radiusY: height * 0.12,
                    },
                    &brush,
                );
            },
            Glyph::Search => unsafe {
                target.DrawEllipse(
                    &D2D1_ELLIPSE {
                        point: point(10.5, 10.5),
                        radiusX: width * 0.27,
                        radiusY: height * 0.27,
                    },
                    &brush,
                    stroke,
                    None::<&ID2D1StrokeStyle>,
                );
                line(15.0, 15.0, 21.0, 21.0);
            },
            Glyph::Grid => {
                for (x, y) in [(4.0, 4.0), (14.0, 4.0), (4.0, 14.0), (14.0, 14.0)] {
                    unsafe {
                        target.DrawRectangle(
                            &rect(
                                area.left + width * x / 24.0,
                                area.top + height * y / 24.0,
                                area.left + width * (x + 6.0) / 24.0,
                                area.top + height * (y + 6.0) / 24.0,
                            )
                            .d2d(),
                            &brush,
                            stroke,
                            None::<&ID2D1StrokeStyle>,
                        );
                    }
                }
            }
            Glyph::List => {
                for y in [6.0, 12.0, 18.0] {
                    line(4.0, y, 20.0, y);
                }
            }
            Glyph::More => unsafe {
                for x in [6.0, 12.0, 18.0] {
                    target.FillEllipse(
                        &D2D1_ELLIPSE {
                            point: point(x, 12.0),
                            radiusX: 1.3,
                            radiusY: 1.3,
                        },
                        &brush,
                    );
                }
            },
            Glyph::Plus => {
                line(12.0, 4.0, 12.0, 20.0);
                line(4.0, 12.0, 20.0, 12.0);
            }
            Glyph::Play => {
                line(8.0, 5.0, 18.0, 12.0);
                line(18.0, 12.0, 8.0, 19.0);
                line(8.0, 19.0, 8.0, 5.0);
            }
            Glyph::Pause => unsafe {
                target.FillRectangle(
                    &rect(
                        area.left + width * 7.0 / 24.0,
                        area.top + height * 5.0 / 24.0,
                        area.left + width * 10.0 / 24.0,
                        area.top + height * 19.0 / 24.0,
                    )
                    .d2d(),
                    &brush,
                );
                target.FillRectangle(
                    &rect(
                        area.left + width * 14.0 / 24.0,
                        area.top + height * 5.0 / 24.0,
                        area.left + width * 17.0 / 24.0,
                        area.top + height * 19.0 / 24.0,
                    )
                    .d2d(),
                    &brush,
                );
            },
            Glyph::ChevronLeft => {
                line(15.0, 5.0, 8.0, 12.0);
                line(8.0, 12.0, 15.0, 19.0);
            }
            Glyph::ChevronRight => {
                line(9.0, 5.0, 16.0, 12.0);
                line(16.0, 12.0, 9.0, 19.0);
            }
            Glyph::Fullscreen => {
                line(4.0, 9.0, 4.0, 4.0);
                line(4.0, 4.0, 9.0, 4.0);
                line(15.0, 4.0, 20.0, 4.0);
                line(20.0, 4.0, 20.0, 9.0);
                line(20.0, 15.0, 20.0, 20.0);
                line(20.0, 20.0, 15.0, 20.0);
                line(9.0, 20.0, 4.0, 20.0);
                line(4.0, 20.0, 4.0, 15.0);
            }
            Glyph::Clock => unsafe {
                target.DrawEllipse(
                    &D2D1_ELLIPSE {
                        point: point(12.0, 12.0),
                        radiusX: width * 0.36,
                        radiusY: height * 0.36,
                    },
                    &brush,
                    stroke,
                    None::<&ID2D1StrokeStyle>,
                );
                line(12.0, 12.0, 12.0, 6.0);
                line(12.0, 12.0, 17.0, 15.0);
            },
            Glyph::Monitor => {
                line(3.0, 4.0, 21.0, 4.0);
                line(21.0, 4.0, 21.0, 17.0);
                line(21.0, 17.0, 3.0, 17.0);
                line(3.0, 17.0, 3.0, 4.0);
                line(12.0, 17.0, 12.0, 21.0);
                line(8.0, 21.0, 16.0, 21.0);
            }
            Glyph::Audio => {
                line(4.0, 10.0, 8.0, 10.0);
                line(8.0, 10.0, 13.0, 5.0);
                line(13.0, 5.0, 13.0, 19.0);
                line(13.0, 19.0, 8.0, 14.0);
                line(8.0, 14.0, 4.0, 14.0);
                line(4.0, 14.0, 4.0, 10.0);
                line(16.0, 9.0, 18.0, 11.0);
                line(18.0, 11.0, 18.0, 13.0);
                line(18.0, 13.0, 16.0, 15.0);
            }
            Glyph::Pencil => {
                line(5.0, 18.0, 7.0, 13.0);
                line(7.0, 13.0, 16.5, 3.5);
                line(16.5, 3.5, 20.5, 7.5);
                line(20.5, 7.5, 11.0, 17.0);
                line(11.0, 17.0, 5.0, 18.0);
                line(7.0, 13.0, 11.0, 17.0);
            }
            Glyph::ChevronDown => {
                line(5.0, 9.0, 12.0, 16.0);
                line(12.0, 16.0, 19.0, 9.0);
            }
            Glyph::Close => {
                line(8.5, 8.5, 15.5, 15.5);
                line(15.5, 8.5, 8.5, 15.5);
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
                    radiusX: 7.0,
                    radiusY: 7.0,
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

    fn draw_home_girl(&mut self, destination: LogicalRect) -> Result<(), String> {
        if self.home_girl.is_none() {
            self.home_girl = Some(self.load_embedded_png(HOME_GIRL_PNG)?);
        }
        let bitmap = self.home_girl.as_ref().expect("home girl was loaded");
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
        Ok(())
    }

    fn draw_settings_sticker(&mut self, destination: LogicalRect) -> Result<(), String> {
        if self.settings_sticker.is_none() {
            self.settings_sticker = Some(self.load_embedded_png(SETTINGS_STICKER_PNG)?);
        }
        let bitmap = self
            .settings_sticker
            .as_ref()
            .expect("settings sticker was loaded");
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
        Ok(())
    }

    fn load_embedded_png(&self, bytes: &[u8]) -> Result<ID2D1Bitmap, String> {
        let stream =
            unsafe { self.wic_factory.CreateStream() }.map_err(|error| error.to_string())?;
        unsafe { stream.InitializeFromMemory(bytes) }.map_err(|error| error.to_string())?;
        let decoder = unsafe {
            self.wic_factory.CreateDecoderFromStream(
                &stream,
                std::ptr::null(),
                WICDecodeMetadataCacheOnLoad,
            )
        }
        .map_err(|error| error.to_string())?;
        let frame = unsafe { decoder.GetFrame(0) }.map_err(|error| error.to_string())?;
        let converter = unsafe { self.wic_factory.CreateFormatConverter() }
            .map_err(|error| error.to_string())?;
        unsafe {
            converter.Initialize(
                &frame,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapDitherTypeNone,
                None::<&IWICPalette>,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
        }
        .map_err(|error| error.to_string())?;
        let target = self.target.as_ref().expect("render target exists");
        unsafe { target.CreateBitmapFromWicBitmap(&converter, None) }
            .map_err(|error| error.to_string())
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

fn grid_capacity(width: f32, height: f32) -> usize {
    let columns = if width >= 900.0 {
        4
    } else if width >= 650.0 {
        3
    } else if width >= 450.0 {
        2
    } else {
        1
    };
    let gap = 18.0;
    let card_width = (width - gap * (columns - 1) as f32) / columns as f32;
    let card_height = card_width * 9.0 / 16.0 + 66.0;
    let rows = ((height + gap) / (card_height + gap)).floor() as usize;
    columns * rows.max(1)
}

fn compact_settings_row_height(panel: LogicalRect) -> f32 {
    if panel.bottom - panel.top < 300.0 {
        42.0
    } else {
        52.0
    }
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
    let panel_space = (panels_bottom - top - gap).max(450.0);
    let top_height = (panel_space * 0.46).clamp(225.0, 314.0);
    let bottom_top = top + top_height + gap;
    [
        rect(left, top, middle - gap / 2.0, top + top_height),
        rect(middle + gap / 2.0, top, right, top + top_height),
        rect(left, bottom_top, middle - gap / 2.0, panels_bottom),
        rect(middle + gap / 2.0, bottom_top, right, panels_bottom),
    ]
}

fn settings_control_area(panel: LogicalRect, index: usize) -> LogicalRect {
    let row_height = compact_settings_row_height(panel);
    let top = panel.top + 44.0 + index as f32 * row_height;
    let control_width = ((panel.right - panel.left) * 0.34).clamp(128.0, 205.0);
    rect(
        panel.right - control_width - 20.0,
        top + 6.0,
        panel.right - 20.0,
        top + row_height - 6.0,
    )
}

fn settings_gain_rail_in_panel(panel: LogicalRect, index: usize) -> LogicalRect {
    let control = settings_control_area(panel, index);
    let center = (control.top + control.bottom) / 2.0;
    rect(
        control.left + 5.0,
        center - 2.0,
        control.right - 48.0,
        center + 2.0,
    )
}

fn ellipsize(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_owned();
    }
    let visible = max_chars.saturating_sub(1);
    let mut shortened = value.chars().take(visible).collect::<String>();
    shortened.push('…');
    shortened
}

fn sidebar_width(width: f32, expanded: bool) -> f32 {
    if !expanded || width < 1_080.0 {
        SIDEBAR_COMPACT
    } else {
        SIDEBAR_EXPANDED
    }
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

fn hotkey_capture_label(modifiers: &[String]) -> String {
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
        "Press shortcut…".into()
    } else {
        format!("{} + …", modifiers.join(" + "))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        SETTINGS_ROW_HEIGHT, format_bytes, format_storage_limit, home_girl_layout, rect,
        settings_gain_percent, settings_row_top, settings_sticker_layout,
    };

    #[test]
    fn storage_sizes_use_only_mb_and_gb_labels() {
        assert_eq!(format_bytes(512 * 1_024), "0.5 MB");
        assert_eq!(format_bytes(20 * 1_048_576), "20.0 MB");
        assert_eq!(format_bytes(5 * 1_073_741_824), "5.0 GB");
        assert_eq!(format_storage_limit(512), "512 MB");
        assert_eq!(format_storage_limit(10_240), "10 GB");
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

    #[test]
    fn home_girl_is_flush_with_the_window_and_leaves_room_for_status() {
        let (girl, content_right) = home_girl_layout(1_424.0, 853.0, 136.0, 1_376.0);

        assert_eq!(girl.right, 1_425.0);
        assert_eq!(girl.bottom, 924.0);
        assert!(girl.top > 400.0);
        assert!(content_right <= girl.left - 18.0);
        assert!(content_right - 136.0 >= 540.0);

        let (compact_girl, compact_content_right) = home_girl_layout(900.0, 620.0, 100.0, 872.0);
        assert_eq!(compact_girl.right, 901.0);
        assert_eq!(compact_girl.bottom, 691.0);
        assert!(compact_content_right <= compact_girl.left - 18.0);
        assert!(compact_content_right - 100.0 >= 420.0);
    }

    /// Decoration: it disappears rather than overlap a control on a short window.
    #[test]
    fn the_settings_sticker_never_reaches_the_controls() {
        let rows_bottom = settings_row_top(2) + SETTINGS_ROW_HEIGHT;

        let sticker = settings_sticker_layout(136.0, 1_376.0, 853.0).expect("room at this size");
        assert!(sticker.top > rows_bottom);
        assert_eq!(sticker.right, 1_376.0);
        assert!(sticker.bottom < 853.0);
        assert!(sticker.left > 136.0);

        assert!(settings_sticker_layout(136.0, 1_376.0, 620.0).is_none());
    }
}
