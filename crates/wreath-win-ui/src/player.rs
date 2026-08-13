use std::cell::Cell;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, SIZE, WPARAM};
use windows::Win32::Media::MediaFoundation::{
    IMFPMediaPlayer, IMFPMediaPlayerCallback, IMFPMediaPlayerCallback_Impl, MFP_EVENT_HEADER,
    MFP_EVENT_TYPE_MEDIAITEM_SET, MFP_EVENT_TYPE_PLAYBACK_ENDED, MFP_MEDIAPLAYER_STATE_PLAYING,
    MFP_OPTION_NONE, MFP_POSITIONTYPE_100NS, MFPCreateMediaPlayer, MFVideoARMode_PreservePicture,
};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};
use windows::core::{PCWSTR, implement};

pub const PLAYER_EVENT: u32 = WM_APP + 21;

#[implement(IMFPMediaPlayerCallback)]
struct PlayerCallback {
    owner: HWND,
}

impl IMFPMediaPlayerCallback_Impl for PlayerCallback_Impl {
    fn OnMediaPlayerEvent(&self, event: *const MFP_EVENT_HEADER) {
        if event.is_null() {
            return;
        }
        let event = unsafe { &*event };
        unsafe {
            let _ = PostMessageW(
                Some(self.owner),
                PLAYER_EVENT,
                WPARAM(event.eEventType.0 as usize),
                LPARAM(event.hrEvent.0 as isize),
            );
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PlayerSnapshot {
    pub ready: bool,
    pub playing: bool,
    pub position_seconds: f64,
    pub duration_seconds: f64,
    pub aspect_ratio: f32,
    pub video_width: u32,
    pub video_height: u32,
}

pub struct Player {
    // Keep our callback COM object alive for exactly as long as the player.
    _callback: IMFPMediaPlayerCallback,
    media: IMFPMediaPlayer,
    loaded: bool,
    load_error: Option<String>,
    ready: bool,
    should_play: Cell<bool>,
    volume: Cell<f32>,
}

impl Player {
    pub fn new(window: HWND, owner: HWND) -> Result<Self, String> {
        let callback: IMFPMediaPlayerCallback = PlayerCallback { owner }.into();
        let mut media = None;
        unsafe {
            MFPCreateMediaPlayer(
                PCWSTR::null(),
                false,
                MFP_OPTION_NONE,
                &callback,
                Some(window),
                Some(&mut media),
            )
        }
        .map_err(|error| format!("Media Foundation player initialization failed: {error}"))?;
        let media = media.ok_or_else(|| "Media Foundation returned no player".to_owned())?;

        // Presentation only, and rejected until a media item is attached, so they
        // must not decide whether the clip counts as loaded.
        let _ = unsafe { media.SetAspectRatioMode(MFVideoARMode_PreservePicture.0 as u32) };
        let _ = unsafe { media.SetBorderColor(COLORREF(0x0010_1012)) };

        Ok(Self {
            _callback: callback,
            media,
            loaded: false,
            load_error: None,
            ready: false,
            should_play: Cell::new(false),
            volume: Cell::new(1.0),
        })
    }

    pub fn open(&mut self, path: &Path) -> Result<(), String> {
        self.ready = false;
        self.loaded = false;
        self.load_error = None;
        self.should_play.set(true);
        let path = path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let result = (|| {
            // A replaced clip keeps its path, and without this Media Foundation
            // keeps reporting the pre-cut duration.
            let _ = unsafe { self.media.ClearMediaItem() };
            let mut item = None;
            unsafe {
                self.media
                    .CreateMediaItemFromURL(PCWSTR(path.as_ptr()), true, 0, Some(&mut item))
            }
            .map_err(|error| format!("Media Foundation cannot open the clip: {error}"))?;
            let item = item.ok_or_else(|| "Media Foundation returned no media item".to_owned())?;
            unsafe { self.media.SetMediaItem(&item) }
                .map_err(|error| format!("Media Foundation cannot load the clip: {error}"))
        })();
        if let Err(error) = result {
            self.should_play.set(false);
            self.load_error = Some(error.clone());
            return Err(error);
        }
        self.loaded = true;
        Ok(())
    }

    pub fn toggle(&self) -> Result<(), String> {
        if !self.loaded {
            return Err(self
                .load_error
                .clone()
                .unwrap_or_else(|| "No clip is loaded".into()));
        }
        if !self.ready {
            return Ok(());
        }
        unsafe {
            if self.media.GetState().map_err(|error| error.to_string())?
                == MFP_MEDIAPLAYER_STATE_PLAYING
            {
                self.should_play.set(false);
                self.media.Pause()
            } else {
                let position = time_value(self.media.GetPosition(&MFP_POSITIONTYPE_100NS));
                let duration = time_value(self.media.GetDuration(&MFP_POSITIONTYPE_100NS));
                if duration > 0.0 && position + 0.05 >= duration {
                    self.seek_fraction(0.0)?;
                }
                self.should_play.set(true);
                self.media.Play()
            }
        }
        .map_err(|error| error.to_string())
    }

    pub fn play(&self) -> Result<(), String> {
        if !self.loaded {
            return Err(self
                .load_error
                .clone()
                .unwrap_or_else(|| "No clip is loaded".into()));
        }
        if !self.ready {
            self.should_play.set(true);
            return Ok(());
        }
        self.should_play.set(true);
        unsafe { self.media.Play() }.map_err(|error| error.to_string())
    }

    pub fn stop(&self) -> Result<(), String> {
        if !self.loaded {
            self.should_play.set(false);
            return Ok(());
        }
        self.should_play.set(false);
        unsafe { self.media.Stop() }.map_err(|error| error.to_string())
    }

    pub fn close(&mut self) {
        self.should_play.set(false);
        if !self.loaded {
            return;
        }
        let _ = unsafe { self.media.ClearMediaItem() };
        self.loaded = false;
        self.ready = false;
        self.load_error = None;
    }

    pub fn handle_event(&mut self, event_type: i32, result: i32) -> Result<(), String> {
        if result < 0 {
            self.ready = false;
            let error =
                windows::core::Error::from_hresult(windows::core::HRESULT(result)).to_string();
            self.load_error = Some(error.clone());
            return Err(error);
        }
        if event_type == MFP_EVENT_TYPE_MEDIAITEM_SET.0 {
            self.ready = true;
            unsafe { self.media.SetVolume(self.volume.get()) }
                .map_err(|error| error.to_string())?;
            if self.should_play.get() {
                unsafe { self.media.Play() }.map_err(|error| error.to_string())?;
            }
        } else if event_type == MFP_EVENT_TYPE_PLAYBACK_ENDED.0 {
            // The ended state cannot be stopped, and seeking from here made the
            // session emit an invalid-state error; the next Play restarts.
            self.should_play.set(false);
        }
        Ok(())
    }

    pub fn snapshot(&self) -> PlayerSnapshot {
        if !self.loaded {
            return PlayerSnapshot::default();
        }
        let playing = unsafe { self.media.GetState() }
            .is_ok_and(|state| state == MFP_MEDIAPLAYER_STATE_PLAYING);
        let position_seconds =
            time_value(unsafe { self.media.GetPosition(&MFP_POSITIONTYPE_100NS) });
        let duration_seconds =
            time_value(unsafe { self.media.GetDuration(&MFP_POSITIONTYPE_100NS) });
        let mut video_size = SIZE::default();
        let mut aspect_size = SIZE::default();
        let (aspect_ratio, video_width, video_height) = if unsafe {
            self.media
                .GetNativeVideoSize(Some(&mut video_size), Some(&mut aspect_size))
        }
        .is_ok()
        {
            let size = if aspect_size.cx > 0 && aspect_size.cy > 0 {
                aspect_size
            } else {
                video_size
            };
            if size.cx > 0 && size.cy > 0 {
                (
                    size.cx as f32 / size.cy as f32,
                    size.cx as u32,
                    size.cy as u32,
                )
            } else {
                (16.0 / 9.0, 0, 0)
            }
        } else {
            (16.0 / 9.0, 0, 0)
        };
        PlayerSnapshot {
            ready: self.ready,
            playing,
            position_seconds,
            duration_seconds,
            aspect_ratio,
            video_width,
            video_height,
        }
    }

    pub fn seek_fraction(&self, fraction: f64) -> Result<(), String> {
        if !self.loaded {
            return Err(self
                .load_error
                .clone()
                .unwrap_or_else(|| "No clip is loaded".into()));
        }
        if !self.ready {
            return Ok(());
        }
        let duration = time_value(unsafe { self.media.GetDuration(&MFP_POSITIONTYPE_100NS) });
        let position = (duration * fraction.clamp(0.0, 1.0) * 10_000_000.0).round() as i64;
        let value = windows::Win32::System::Com::StructuredStorage::PROPVARIANT::from(position);
        unsafe { self.media.SetPosition(&MFP_POSITIONTYPE_100NS, &value) }
            .map_err(|error| error.to_string())
    }

    pub fn set_volume(&self, percent: u8) -> Result<(), String> {
        let volume = f32::from(percent.min(100)) / 100.0;
        self.volume.set(volume);
        if !self.loaded || !self.ready {
            return Ok(());
        }
        unsafe { self.media.SetVolume(volume) }.map_err(|error| error.to_string())
    }

    pub fn update_video(&self) {
        let _ = unsafe { self.media.UpdateVideo() };
    }

    pub fn shutdown(&mut self) {
        self.ready = false;
        self.loaded = false;
        self.should_play.set(false);
        let _ = unsafe { self.media.Shutdown() };
    }
}

fn time_value(
    value: windows::core::Result<windows::Win32::System::Com::StructuredStorage::PROPVARIANT>,
) -> f64 {
    value
        .ok()
        .and_then(|value| i64::try_from(&value).ok())
        .map_or(0.0, |value| value.max(0) as f64 / 10_000_000.0)
}

impl Drop for Player {
    fn drop(&mut self) {
        self.shutdown();
    }
}
