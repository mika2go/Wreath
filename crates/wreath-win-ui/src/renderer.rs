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
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory, IWICPalette,
    WICBitmapDitherTypeNone, WICBitmapIgnoreAlpha, WICBitmapPaletteTypeCustom,
    WICDecodeMetadataCacheOnLoad,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, IBindCtx};
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK,
};
use windows::Win32::UI::WindowsAndMessaging::{
    SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
};
use windows::core::{PCWSTR, w};
use windows_numerics::Vector2;

use crate::model::{
    Action, DeleteTarget, Page, SettingsMenuKind, SettingsSection, TextInput, UiModel,
    quality_label,
};

// Near-black clip canvas, raised neutral controls, light actions. Color is
// reserved for capture status, warnings and destructive operations.
const CANVAS: u32 = 0x09090b;
const STAGE: u32 = 0x131316;
const SURFACE: u32 = 0x1c1c20;
const SURFACE_RAISED: u32 = 0x242429;
const SURFACE_HOVER: u32 = 0x2d2d33;
const BORDER: u32 = 0x393940;
const PRIMARY: u32 = 0xf5f5f7;
const SECONDARY: u32 = 0xa3a3ad;
const ACCENT: u32 = 0xedeef2;
const ACCENT_HOVER: u32 = 0xffffff;
const ACCENT_MUTED: u32 = 0x35353c;
const READY: u32 = 0x35d07f;
const WARNING: u32 = 0xf0b849;
const SELECTION: u32 = 0x424854;
const DANGER: u32 = 0xf15b68;
const SETTINGS_ROW_TOP: f32 = 270.0;
const SETTINGS_ROW_HEIGHT: f32 = 76.0;
const SETTINGS_ROW_GAP: f32 = 12.0;
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

pub fn player_bounds(
    width: u32,
    height: u32,
    aspect_ratio: f32,
    sidebar_expanded: bool,
) -> LogicalRect {
    let width = width as f32;
    let rail = sidebar_width(width, sidebar_expanded);
    let padding = if width < 1_080.0 {
        28.0
    } else if width < 1_300.0 {
        36.0
    } else {
        48.0
    };
    fit_aspect(
        rect(
            rail + padding + 96.0,
            184.0,
            width - padding - 96.0,
            (height as f32 - 112.0).max(330.0),
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
    let padding = if width < 1_080.0 {
        28.0
    } else if width < 1_300.0 {
        36.0
    } else {
        48.0
    };
    fit_aspect(
        rect(
            rail + padding,
            176.0,
            width - padding,
            (height as f32 - 274.0).max(330.0),
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
    let padding = if width_f < 1_080.0 {
        28.0
    } else if width_f < 1_300.0 {
        36.0
    } else {
        48.0
    };
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

pub fn player_timeline_rail(width: u32, height: u32, sidebar_expanded: bool) -> LogicalRect {
    let width = width as f32;
    let rail = sidebar_width(width, sidebar_expanded);
    let padding = if width < 1_080.0 {
        28.0
    } else if width < 1_300.0 {
        36.0
    } else {
        48.0
    };
    let left = rail + padding;
    let right = width - padding;
    let controls_top = height as f32 - 88.0;
    rect(
        left + 58.0,
        controls_top + 16.0,
        right - 160.0,
        controls_top + 22.0,
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
    let height = height as f32;
    rect(
        24.0,
        10.0,
        (width - 24.0).max(25.0),
        (height - 64.0).max(14.0),
    )
}

pub fn fullscreen_volume_rail(width: u32, height: u32) -> LogicalRect {
    let width = width as f32;
    let height = height as f32;
    rect(204.0, height - 34.0, width.min(334.0), height - 28.0)
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
    const MINIMUM_HEIGHT: f32 = 96.0;
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
            36.0,
            true,
            false,
        )?;
        let heading = text_format(
            &write_factory,
            w!("Segoe UI Variable Display"),
            25.0,
            true,
            false,
        )?;
        let section = text_format(
            &write_factory,
            w!("Segoe UI Variable Display"),
            18.0,
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
            10.5,
            true,
            false,
        )?;
        let body_center = text_format(
            &write_factory,
            w!("Segoe UI Variable Text"),
            13.0,
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

    fn render_frame(&mut self, model: &UiModel, width: u32, height: u32) -> Result<(), String> {
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

    fn render_fullscreen_controls(
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
        self.fill(rect(0.0, 0.0, rail, height), STAGE, 0.0)?;
        self.fill(rect(rail - 1.0, 0.0, rail, height), BORDER, 0.0)?;
        self.fill(rect(rail, 83.0, width, 84.0), BORDER, 0.0)?;

        self.text(
            "Local capture",
            rect(rail + 48.0, 20.0, rail + 290.0, 40.0),
            &self.small.clone(),
            SECONDARY,
        )?;
        self.text(
            "WREATH",
            rect(rail + 48.0, 39.0, rail + 240.0, 70.0),
            &self.section.clone(),
            PRIMARY,
        )?;

        if model.page == Page::Library {
            let search_width = if width < 1_100.0 { 208.0 } else { 244.0 };
            let search = rect(width - search_width - 36.0, 20.0, width - 36.0, 58.0);
            let search_action = Action::Search;
            let search_fill = if self.is_hovered(&search_action) {
                mix(SURFACE, ACCENT, self.hover_progress * 0.07)
            } else {
                SURFACE
            };
            self.fill(search, search_fill, 8.0)?;
            self.stroke(
                search,
                if model.search_focused {
                    ACCENT
                } else {
                    mix(BORDER, SURFACE, 0.38)
                },
                8.0,
                1.0,
            )?;
            self.hits.push(HitRegion {
                rect: search,
                action: search_action,
            });
            self.render_text_input(
                &model.search,
                rect(
                    search.left + 36.0,
                    search.top,
                    search.right - 64.0,
                    search.bottom,
                ),
                "Search your clips",
                model.search_focused,
                TextInputTarget::Search,
            )?;
            self.text(
                "⌕",
                rect(
                    search.left + 10.0,
                    search.top,
                    search.left + 32.0,
                    search.bottom,
                ),
                &self.body_center.clone(),
                if model.search_focused {
                    PRIMARY
                } else {
                    SECONDARY
                },
            )?;
            let shortcut = rect(
                search.right - 55.0,
                search.top + 8.0,
                search.right - 8.0,
                search.bottom - 8.0,
            );
            self.fill(shortcut, ACCENT_MUTED, 5.0)?;
            self.text("Ctrl K", shortcut, &self.small.clone(), SECONDARY)?;
        }

        let nav = [
            (Page::Home, Glyph::Home, "Home"),
            (Page::Library, Glyph::Library, "Library"),
            (Page::Collections, Glyph::Collections, "Collections"),
        ];
        for (offset, (page, icon, label)) in nav.iter().enumerate() {
            let top = 24.0 + offset as f32 * 58.0;
            let active = model.page == *page
                || (matches!(model.page, Page::Player | Page::Editor)
                    && model.previous_page == *page);
            let action = Action::Navigate(*page);
            let nav_area = rect((rail - 44.0) / 2.0, top, (rail + 44.0) / 2.0, top + 44.0);
            if active || self.is_hovered(&action) {
                let fill = if active {
                    ACCENT_MUTED
                } else {
                    mix(SURFACE_HOVER, ACCENT, self.hover_progress * 0.15)
                };
                self.fill(nav_area, fill, 12.0)?;
            }
            self.glyph(
                *icon,
                rect(
                    nav_area.left + 12.0,
                    top + 12.0,
                    nav_area.right - 12.0,
                    top + 32.0,
                ),
                if active || self.is_hovered(&action) {
                    PRIMARY
                } else {
                    SECONDARY
                },
            )?;
            let _ = label;
            self.hits.push(HitRegion {
                rect: nav_area,
                action,
            });
        }

        let settings_action = Action::Navigate(Page::Settings);
        let settings = rect(
            (rail - 44.0) / 2.0,
            height - 66.0,
            (rail + 44.0) / 2.0,
            height - 22.0,
        );
        let settings_active = model.page == Page::Settings;
        if settings_active || self.is_hovered(&settings_action) {
            let fill = if settings_active {
                ACCENT_MUTED
            } else {
                mix(SURFACE_HOVER, ACCENT, self.hover_progress * 0.15)
            };
            self.fill(settings, fill, 12.0)?;
        }
        self.glyph(
            Glyph::Settings,
            rect(
                settings.left + 12.0,
                settings.top + 12.0,
                settings.right - 12.0,
                settings.bottom - 12.0,
            ),
            if settings_active || self.is_hovered(&settings_action) {
                PRIMARY
            } else {
                SECONDARY
            },
        )?;
        self.hits.push(HitRegion {
            rect: settings,
            action: settings_action,
        });

        let padding = if width < 1_080.0 {
            28.0
        } else if width < 1_300.0 {
            36.0
        } else {
            48.0
        };
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

    fn render_library(
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

    fn render_player(
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

    fn render_editor(
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

    fn page_heading(
        &mut self,
        title: &str,
        subtitle: &str,
        left: f32,
        right: f32,
    ) -> Result<(), String> {
        self.text(
            subtitle,
            rect(left, 116.0, right, 138.0),
            &self.small.clone(),
            SECONDARY,
        )?;
        self.text(
            title,
            rect(left, 141.0, right, 181.0),
            &self.heading.clone(),
            PRIMARY,
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
        let preview_height = (card_width - 12.0) * 9.0 / 16.0;
        let card_height = preview_height + 58.0;
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
            let preview = rect(
                x + 6.0,
                y + 6.0,
                x + card_width - 6.0,
                y + 6.0 + preview_height,
            );
            self.fill(preview, STAGE, 7.0)?;
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
                    y + preview_height + 14.0,
                    x + card_width - 8.0,
                    y + preview_height + 34.0,
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
                    y + preview_height + 36.0,
                    x + card_width - 8.0,
                    y + preview_height + 52.0,
                ),
                &self.small.clone(),
                SECONDARY,
            )?;
            self.hits.push(HitRegion { rect: card, action });
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

        let rail = sidebar_width(width, model.sidebar_expanded);
        let padding = if width < 1_080.0 {
            28.0
        } else if width < 1_300.0 {
            36.0
        } else {
            48.0
        };
        let left = rail + padding;
        let right = width - padding;
        let column_middle = (left + right) / 2.0;
        let (anchor_left, anchor_right, row) = match menu_state.kind {
            SettingsMenuKind::DesktopGain => (left, column_middle - 6.0, 1),
            SettingsMenuKind::DesktopDevice => (left, column_middle - 6.0, 2),
            SettingsMenuKind::Microphone => (column_middle + 6.0, right, 1),
            SettingsMenuKind::MicrophoneGain => (column_middle + 6.0, right, 2),
            SettingsMenuKind::Display | SettingsMenuKind::Duration => (left, right, 0),
            SettingsMenuKind::FrameRate
            | SettingsMenuKind::Codec
            | SettingsMenuKind::StorageLimit => (left, right, 1),
            SettingsMenuKind::Quality => (left, right, 2),
        };
        let available = anchor_right - anchor_left;
        let control_width = (available * 0.38).clamp(190.0, 360.0);
        let row_top = settings_row_top(row);
        let anchor = rect(
            anchor_right - control_width - 16.0,
            row_top + 17.0,
            anchor_right - 16.0,
            row_top + 59.0,
        );
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
        let menu_left = (anchor.right - menu_width).max(anchor_left);
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
            "Clip actions",
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
            "Edit clip",
            Action::EditClip(context.clip),
            false,
        )?;
        row_top += 44.0;
        self.context_menu_row(
            rect(left + 8.0, row_top, menu.right - 8.0, row_top + 40.0),
            "Rename",
            Action::RenameClip(context.clip),
            false,
        )?;
        row_top += 44.0;

        if visible_collections > 0 {
            self.text(
                "Move to collection",
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
            "Delete clip",
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
            "Ctrl+A select all · Ctrl+C/X/V · Enter confirm · Esc cancel",
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
            SettingControl::Button | SettingControl::Toggle => {
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
        match glyph {
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

fn sidebar_width(width: f32, expanded: bool) -> f32 {
    let _ = expanded;
    if width < 1_080.0 { 72.0 } else { 88.0 }
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
        SETTINGS_ROW_HEIGHT, format_bytes, format_storage_limit, home_girl_layout,
        settings_row_top, settings_sticker_layout,
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
