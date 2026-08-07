use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, WPARAM};
use windows::Win32::Media::MediaFoundation::{
    IMFPMediaPlayer, IMFPMediaPlayerCallback, IMFPMediaPlayerCallback_Impl, MFP_EVENT_HEADER,
    MFP_MEDIAPLAYER_STATE_PLAYING, MFP_OPTION_NONE, MFPCreateMediaPlayer,
    MFVideoARMode_PreservePicture,
};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};
use windows::core::{PCWSTR, implement};

pub const PLAYER_EVENT: u32 = WM_APP + 21;

#[implement(IMFPMediaPlayerCallback)]
struct PlayerCallback {
    owner: HWND,
}

impl IMFPMediaPlayerCallback_Impl for PlayerCallback_Impl {
    fn OnMediaPlayerEvent(&self, _event: *const MFP_EVENT_HEADER) {
        unsafe {
            let _ = PostMessageW(Some(self.owner), PLAYER_EVENT, WPARAM(0), LPARAM(0));
        }
    }
}

pub struct Player {
    window: HWND,
    callback: IMFPMediaPlayerCallback,
    media: Option<IMFPMediaPlayer>,
}

impl Player {
    pub fn new(window: HWND, owner: HWND) -> Self {
        Self {
            window,
            callback: PlayerCallback { owner }.into(),
            media: None,
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
                true,
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
        Ok(())
    }

    pub fn toggle(&self) -> Result<(), String> {
        let Some(media) = &self.media else {
            return Err("No clip is loaded".into());
        };
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

    pub fn update_video(&self) {
        if let Some(media) = &self.media {
            let _ = unsafe { media.UpdateVideo() };
        }
    }

    pub fn shutdown(&mut self) {
        if let Some(media) = self.media.take() {
            let _ = unsafe { media.Shutdown() };
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.shutdown();
    }
}
