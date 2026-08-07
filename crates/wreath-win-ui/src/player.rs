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
}

pub struct Player {
    window: HWND,
    callback: IMFPMediaPlayerCallback,
    media: Option<IMFPMediaPlayer>,
    ready: bool,
}

impl Player {
    pub fn new(window: HWND, owner: HWND) -> Self {
        Self {
            window,
            callback: PlayerCallback { owner }.into(),
            media: None,
            ready: false,
        }
    }

    pub fn open(&mut self, path: &Path) -> Result<(), String> {
        self.shutdown();
        let path = path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let mut media = None;
        unsafe {
            MFPCreateMediaPlayer(
                PCWSTR(path.as_ptr()),
                false,
                MFP_OPTION_NONE,
                &self.callback,
                Some(self.window),
                Some(&mut media),
            )
        }
        .map_err(|error| format!("Media Foundation cannot open the clip: {error}"))?;
        let media = media.ok_or_else(|| "Media Foundation returned no player".to_owned())?;
        unsafe {
            media
                .SetAspectRatioMode(MFVideoARMode_PreservePicture.0 as u32)
                .map_err(|error| error.to_string())?;
            media
                .SetBorderColor(COLORREF(0x0010_1012))
                .map_err(|error| error.to_string())?;
        }
        self.media = Some(media);
        self.ready = false;
        Ok(())
    }

    pub fn toggle(&self) -> Result<(), String> {
        let Some(media) = &self.media else {
            return Err("No clip is loaded".into());
        };
        if !self.ready {
            return Ok(());
        }
        unsafe {
            if media.GetState().map_err(|error| error.to_string())? == MFP_MEDIAPLAYER_STATE_PLAYING
            {
                media.Pause()
            } else {
                media.Play()
            }
        }
        .map_err(|error| error.to_string())
    }

    pub fn handle_event(&mut self, event_type: i32, result: i32) -> Result<(), String> {
        if result < 0 {
            return Err(
                windows::core::Error::from_hresult(windows::core::HRESULT(result)).to_string(),
            );
        }
        if event_type == MFP_EVENT_TYPE_MEDIAITEM_SET.0 {
            self.ready = true;
            if let Some(media) = &self.media {
                unsafe { media.Play() }.map_err(|error| error.to_string())?;
            }
        } else if event_type == MFP_EVENT_TYPE_PLAYBACK_ENDED.0 {
            self.seek_fraction(0.0)?;
        }
        Ok(())
    }

    pub fn snapshot(&self) -> PlayerSnapshot {
        let Some(media) = &self.media else {
            return PlayerSnapshot::default();
        };
        let playing =
            unsafe { media.GetState() }.is_ok_and(|state| state == MFP_MEDIAPLAYER_STATE_PLAYING);
        let position_seconds = time_value(unsafe { media.GetPosition(&MFP_POSITIONTYPE_100NS) });
        let duration_seconds = time_value(unsafe { media.GetDuration(&MFP_POSITIONTYPE_100NS) });
        let mut video_size = SIZE::default();
        let mut aspect_size = SIZE::default();
        let aspect_ratio =
            if unsafe { media.GetNativeVideoSize(Some(&mut video_size), Some(&mut aspect_size)) }
                .is_ok()
            {
                let size = if aspect_size.cx > 0 && aspect_size.cy > 0 {
                    aspect_size
                } else {
                    video_size
                };
                if size.cx > 0 && size.cy > 0 {
                    size.cx as f32 / size.cy as f32
                } else {
                    16.0 / 9.0
                }
            } else {
                16.0 / 9.0
            };
        PlayerSnapshot {
            ready: self.ready,
            playing,
            position_seconds,
            duration_seconds,
            aspect_ratio,
        }
    }

    pub fn seek_fraction(&self, fraction: f64) -> Result<(), String> {
        let Some(media) = &self.media else {
            return Err("No clip is loaded".into());
        };
        if !self.ready {
            return Ok(());
        }
        let duration = time_value(unsafe { media.GetDuration(&MFP_POSITIONTYPE_100NS) });
        let position = (duration * fraction.clamp(0.0, 1.0) * 10_000_000.0).round() as i64;
        let value = windows::Win32::System::Com::StructuredStorage::PROPVARIANT::from(position);
        unsafe { media.SetPosition(&MFP_POSITIONTYPE_100NS, &value) }
            .map_err(|error| error.to_string())
    }

    pub fn update_video(&self) {
        if let Some(media) = &self.media {
            let _ = unsafe { media.UpdateVideo() };
        }
    }

    pub fn shutdown(&mut self) {
        self.ready = false;
        if let Some(media) = self.media.take() {
            let _ = unsafe { media.Shutdown() };
        }
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
