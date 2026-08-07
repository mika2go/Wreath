use crate::video::VideoError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncoderSettings {
    pub width: u32,
    pub height: u32,
    pub frames_per_second: u16,
    pub bitrate_kbps: u32,
}

impl EncoderSettings {
    pub fn validate(self) -> Result<Self, VideoError> {
        if self.width == 0 || self.height == 0 {
            return Err(VideoError::Initialization(
                "encoder dimensions must be non-zero".into(),
            ));
        }
        if self.width % 2 != 0 || self.height % 2 != 0 {
            return Err(VideoError::Initialization(
                "NV12 encoder dimensions must be even".into(),
            ));
        }
        if self.frames_per_second == 0 {
            return Err(VideoError::Initialization(
                "encoder frame rate must be non-zero".into(),
            ));
        }
        if self.bitrate_kbps == 0 {
            return Err(VideoError::Initialization(
                "encoder bitrate must be non-zero".into(),
            ));
        }
        Ok(self)
    }

    pub fn keyframe_interval_frames(self) -> u32 {
        u32::from(self.frames_per_second).saturating_mul(2)
    }
}

#[cfg(target_os = "windows")]
pub struct HardwareVideoEncoder {
    transform: windows::Win32::Media::MediaFoundation::IMFTransform,
    _device_manager: windows::Win32::Media::MediaFoundation::IMFDXGIDeviceManager,
    settings: EncoderSettings,
    codec: crate::video::HardwareCodec,
}

#[cfg(target_os = "windows")]
impl HardwareVideoEncoder {
    pub fn initialize(
        device: &windows::Win32::Graphics::Direct3D11::ID3D11Device,
        codec: crate::video::HardwareCodec,
        settings: EncoderSettings,
    ) -> Result<Self, VideoError> {
        use windows::Win32::Media::MediaFoundation::{
            IMFTransform, MF_TRANSFORM_ASYNC_UNLOCK, MFCreateDXGIDeviceManager,
            MFT_MESSAGE_SET_D3D_MANAGER,
        };
        use windows::core::Interface;

        let settings = settings.validate()?;
        let activation = crate::video::hardware_encoder_activations(codec.media_subtype())?
            .into_iter()
            .next()
            .ok_or(VideoError::NoHardwareEncoder(match codec {
                crate::video::HardwareCodec::H264 => wreath_core::config::Codec::H264,
                crate::video::HardwareCodec::Hevc => wreath_core::config::Codec::Hevc,
                crate::video::HardwareCodec::Av1 => wreath_core::config::Codec::Av1,
            }))?;
        let transform: IMFTransform = unsafe { activation.ActivateObject() }
            .map_err(|error| VideoError::Initialization(error.to_string()))?;
        let attributes = unsafe { transform.GetAttributes() }
            .map_err(|error| VideoError::Initialization(error.to_string()))?;
        unsafe { attributes.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1) }
            .map_err(|error| VideoError::Initialization(error.to_string()))?;

        let mut reset_token = 0_u32;
        let mut device_manager = None;
        unsafe { MFCreateDXGIDeviceManager(&mut reset_token, &mut device_manager) }
            .map_err(|error| VideoError::Initialization(error.to_string()))?;
        let device_manager = device_manager.ok_or_else(|| {
            VideoError::Initialization("Media Foundation returned no DXGI device manager".into())
        })?;
        unsafe { device_manager.ResetDevice(device, reset_token) }
            .map_err(|error| VideoError::Initialization(error.to_string()))?;
        unsafe {
            transform.ProcessMessage(
                MFT_MESSAGE_SET_D3D_MANAGER,
                Interface::as_raw(&device_manager) as usize,
            )
        }
        .map_err(|error| VideoError::Initialization(error.to_string()))?;

        let output_type = media_type(codec.media_subtype(), settings, true)?;
        let input_type = media_type(
            windows::Win32::Media::MediaFoundation::MFVideoFormat_NV12,
            settings,
            false,
        )?;
        unsafe { transform.SetOutputType(0, &output_type, 0) }
            .map_err(|error| VideoError::Initialization(error.to_string()))?;
        unsafe { transform.SetInputType(0, &input_type, 0) }
            .map_err(|error| VideoError::Initialization(error.to_string()))?;

        Ok(Self {
            transform,
            _device_manager: device_manager,
            settings,
            codec,
        })
    }

    pub fn settings(&self) -> EncoderSettings {
        self.settings
    }

    pub fn codec(&self) -> crate::video::HardwareCodec {
        self.codec
    }

    pub fn transform(&self) -> &windows::Win32::Media::MediaFoundation::IMFTransform {
        &self.transform
    }
}

#[cfg(target_os = "windows")]
fn media_type(
    subtype: windows::core::GUID,
    settings: EncoderSettings,
    compressed: bool,
) -> Result<windows::Win32::Media::MediaFoundation::IMFMediaType, VideoError> {
    use windows::Win32::Media::MediaFoundation::{
        MF_MT_AVG_BITRATE, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE,
        MF_MT_MAJOR_TYPE, MF_MT_MAX_KEYFRAME_SPACING, MF_MT_PIXEL_ASPECT_RATIO, MF_MT_SUBTYPE,
        MFCreateMediaType, MFMediaType_Video, MFVideoInterlace_Progressive,
    };

    let media_type = unsafe { MFCreateMediaType() }
        .map_err(|error| VideoError::Initialization(error.to_string()))?;
    let configure = || -> windows::core::Result<()> {
        unsafe {
            media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            media_type.SetGUID(&MF_MT_SUBTYPE, &subtype)?;
            media_type.SetUINT64(
                &MF_MT_FRAME_SIZE,
                pack_u32_pair(settings.width, settings.height),
            )?;
            media_type.SetUINT64(
                &MF_MT_FRAME_RATE,
                pack_u32_pair(u32::from(settings.frames_per_second), 1),
            )?;
            media_type.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack_u32_pair(1, 1))?;
            media_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            if compressed {
                media_type.SetUINT32(
                    &MF_MT_AVG_BITRATE,
                    settings.bitrate_kbps.saturating_mul(1_000),
                )?;
                media_type.SetUINT32(
                    &MF_MT_MAX_KEYFRAME_SPACING,
                    settings.keyframe_interval_frames(),
                )?;
            }
        }
        Ok(())
    };
    configure().map_err(|error| VideoError::Initialization(error.to_string()))?;
    Ok(media_type)
}

#[cfg(target_os = "windows")]
fn pack_u32_pair(high: u32, low: u32) -> u64 {
    (u64::from(high) << 32) | u64::from(low)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_even_nv12_dimensions() {
        let settings = EncoderSettings {
            width: 2560,
            height: 1440,
            frames_per_second: 60,
            bitrate_kbps: 20_000,
        };

        assert_eq!(settings.validate().unwrap(), settings);
        assert_eq!(settings.keyframe_interval_frames(), 120);
    }

    #[test]
    fn rejects_odd_or_empty_encoder_geometry() {
        for (width, height) in [(0, 1080), (1920, 0), (1919, 1080), (1920, 1079)] {
            let settings = EncoderSettings {
                width,
                height,
                frames_per_second: 60,
                bitrate_kbps: 10_000,
            };
            assert!(settings.validate().is_err());
        }
    }

    #[test]
    fn rejects_zero_rate_settings() {
        let no_frames = EncoderSettings {
            width: 1920,
            height: 1080,
            frames_per_second: 0,
            bitrate_kbps: 10_000,
        };
        let no_bits = EncoderSettings {
            frames_per_second: 60,
            bitrate_kbps: 0,
            ..no_frames
        };

        assert!(no_frames.validate().is_err());
        assert!(no_bits.validate().is_err());
    }
}
