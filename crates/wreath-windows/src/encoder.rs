use crate::video::VideoError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncoderEvent {
    NeedInput,
    HaveOutput,
    DrainComplete,
    Other(u32),
}

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
    events: windows::Win32::Media::MediaFoundation::IMFMediaEventGenerator,
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
            IMFTransform, MF_LOW_LATENCY, MF_TRANSFORM_ASYNC_UNLOCK, MFCreateDXGIDeviceManager,
            MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, MFT_MESSAGE_NOTIFY_START_OF_STREAM,
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
        let events = transform.cast().map_err(initialization_error)?;
        let attributes = unsafe { transform.GetAttributes() }
            .map_err(|error| VideoError::Initialization(error.to_string()))?;
        unsafe { attributes.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1) }
            .map_err(|error| VideoError::Initialization(error.to_string()))?;
        unsafe { attributes.SetUINT32(&MF_LOW_LATENCY, 1) }
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
        configure_rate_control(&transform, settings);
        unsafe { transform.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0) }
            .map_err(|error| VideoError::Initialization(error.to_string()))?;
        unsafe { transform.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0) }
            .map_err(|error| VideoError::Initialization(error.to_string()))?;

        Ok(Self {
            transform,
            events,
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

    pub fn output_media_type(
        &self,
    ) -> Result<windows::Win32::Media::MediaFoundation::IMFMediaType, VideoError> {
        unsafe { self.transform.GetOutputCurrentType(0) }.map_err(initialization_error)
    }

    pub fn submit_texture(
        &self,
        texture: &windows::Win32::Graphics::Direct3D11::ID3D11Texture2D,
        timestamp: std::time::Duration,
    ) -> Result<(), VideoError> {
        use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
        use windows::Win32::Media::MediaFoundation::{MFCreateDXGISurfaceBuffer, MFCreateSample};
        use windows::core::Interface;

        let buffer = unsafe { MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, texture, 0, false) }
            .map_err(initialization_error)?;
        let sample = unsafe { MFCreateSample() }.map_err(initialization_error)?;
        let submit = || -> windows::core::Result<()> {
            unsafe {
                sample.AddBuffer(&buffer)?;
                sample.SetSampleTime(duration_to_hns(timestamp))?;
                sample.SetSampleDuration(self.frame_duration_hns())?;
                self.transform.ProcessInput(0, &sample, 0)?;
            }
            Ok(())
        };
        submit().map_err(initialization_error)
    }

    pub fn take_packet(
        &self,
    ) -> Result<Option<wreath_core::replay_buffer::EncodedPacket>, VideoError> {
        use std::mem::ManuallyDrop;
        use std::time::Duration;

        use windows::Win32::Media::MediaFoundation::{
            IMFSample, MF_E_TRANSFORM_NEED_MORE_INPUT, MFCreateMemoryBuffer, MFCreateSample,
            MFSampleExtension_CleanPoint, MFT_OUTPUT_DATA_BUFFER,
            MFT_OUTPUT_STREAM_PROVIDES_SAMPLES,
        };

        let stream_info =
            unsafe { self.transform.GetOutputStreamInfo(0) }.map_err(initialization_error)?;
        let provides_sample =
            stream_info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 != 0;
        let supplied_sample = if provides_sample {
            None
        } else {
            let sample = unsafe { MFCreateSample() }.map_err(initialization_error)?;
            let buffer = unsafe { MFCreateMemoryBuffer(stream_info.cbSize.max(1)) }
                .map_err(initialization_error)?;
            unsafe { sample.AddBuffer(&buffer) }.map_err(initialization_error)?;
            Some(sample)
        };
        let mut output = MFT_OUTPUT_DATA_BUFFER {
            dwStreamID: 0,
            pSample: ManuallyDrop::new(supplied_sample),
            ..Default::default()
        };
        let mut status = 0_u32;
        let process_result = unsafe {
            self.transform
                .ProcessOutput(0, std::slice::from_mut(&mut output), &mut status)
        };
        let output_sample = unsafe { ManuallyDrop::take(&mut output.pSample) };
        unsafe { ManuallyDrop::drop(&mut output.pEvents) };
        if let Err(error) = process_result {
            if error.code() == MF_E_TRANSFORM_NEED_MORE_INPUT {
                return Ok(None);
            }
            return Err(initialization_error(error));
        }
        let sample: IMFSample = output_sample.ok_or_else(|| {
            VideoError::Initialization("hardware encoder produced no output sample".into())
        })?;
        let buffer = unsafe { sample.ConvertToContiguousBuffer() }.map_err(initialization_error)?;
        let mut data = std::ptr::null_mut();
        let mut length = 0_u32;
        unsafe { buffer.Lock(&mut data, None, Some(&mut length)) }.map_err(initialization_error)?;
        let payload: std::sync::Arc<[u8]> = if data.is_null() || length == 0 {
            std::sync::Arc::from([])
        } else {
            std::sync::Arc::from(unsafe { std::slice::from_raw_parts(data, length as usize) })
        };
        unsafe { buffer.Unlock() }.map_err(initialization_error)?;

        let timestamp_hns = unsafe { sample.GetSampleTime() }.unwrap_or_default().max(0) as u64;
        let duration_hns = unsafe { sample.GetSampleDuration() }
            .unwrap_or(self.frame_duration_hns())
            .max(0) as u64;
        let keyframe =
            unsafe { sample.GetUINT32(&MFSampleExtension_CleanPoint) }.unwrap_or_default() != 0;
        Ok(Some(wreath_core::replay_buffer::EncodedPacket {
            track: wreath_core::replay_buffer::TrackKind::Video,
            timestamp: Duration::from_nanos(timestamp_hns.saturating_mul(100)),
            duration: Duration::from_nanos(duration_hns.saturating_mul(100)),
            keyframe,
            payload,
        }))
    }

    /// Blocks on Media Foundation's event queue; no encoder polling is needed.
    pub fn wait_for_event(&self) -> Result<EncoderEvent, VideoError> {
        use windows::Win32::Media::MediaFoundation::MEDIA_EVENT_GENERATOR_GET_EVENT_FLAGS;

        let event = unsafe {
            self.events
                .GetEvent(MEDIA_EVENT_GENERATOR_GET_EVENT_FLAGS(0))
        }
        .map_err(initialization_error)?;
        unsafe { event.GetStatus() }
            .map_err(initialization_error)?
            .ok()
            .map_err(initialization_error)?;
        let event_type = unsafe { event.GetType() }.map_err(initialization_error)?;
        Ok(classify_event(event_type))
    }

    /// Reads one queued encoder event without waiting. This is called only when
    /// the pipeline was already woken by a captured frame or control command.
    pub fn try_next_event(&self) -> Result<Option<EncoderEvent>, VideoError> {
        use windows::Win32::Media::MediaFoundation::{
            MF_E_NO_EVENTS_AVAILABLE, MF_EVENT_FLAG_NO_WAIT,
        };

        let event = match unsafe { self.events.GetEvent(MF_EVENT_FLAG_NO_WAIT) } {
            Ok(event) => event,
            Err(error) if error.code() == MF_E_NO_EVENTS_AVAILABLE => return Ok(None),
            Err(error) => return Err(initialization_error(error)),
        };
        unsafe { event.GetStatus() }
            .map_err(initialization_error)?
            .ok()
            .map_err(initialization_error)?;
        let event_type = unsafe { event.GetType() }.map_err(initialization_error)?;
        Ok(Some(classify_event(event_type)))
    }

    pub fn drain(&self) -> Result<(), VideoError> {
        use windows::Win32::Media::MediaFoundation::MFT_MESSAGE_COMMAND_DRAIN;

        unsafe { self.transform.ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0) }
            .map_err(initialization_error)
    }

    pub fn flush(&self) -> Result<(), VideoError> {
        use windows::Win32::Media::MediaFoundation::{
            MFT_MESSAGE_COMMAND_FLUSH, MFT_MESSAGE_NOTIFY_START_OF_STREAM,
        };

        unsafe { self.transform.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0) }
            .map_err(initialization_error)?;
        while self.try_next_event()?.is_some() {}
        unsafe {
            self.transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
        }
        .map_err(initialization_error)
    }

    fn frame_duration_hns(&self) -> i64 {
        10_000_000_i64 / i64::from(self.settings.frames_per_second)
    }
}

#[cfg(target_os = "windows")]
fn duration_to_hns(duration: std::time::Duration) -> i64 {
    i64::try_from(duration.as_nanos() / 100).unwrap_or(i64::MAX)
}

#[cfg(target_os = "windows")]
fn initialization_error(error: windows::core::Error) -> VideoError {
    VideoError::Initialization(error.to_string())
}

#[cfg(target_os = "windows")]
fn classify_event(event_type: u32) -> EncoderEvent {
    use windows::Win32::Media::MediaFoundation::{
        METransformDrainComplete, METransformHaveOutput, METransformNeedInput,
    };

    if event_type == METransformNeedInput.0 as u32 {
        EncoderEvent::NeedInput
    } else if event_type == METransformHaveOutput.0 as u32 {
        EncoderEvent::HaveOutput
    } else if event_type == METransformDrainComplete.0 as u32 {
        EncoderEvent::DrainComplete
    } else {
        EncoderEvent::Other(event_type)
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

/// Asks the hardware encoder for peak-constrained variable bitrate.
///
/// Only the average bitrate was ever set, and Media Foundation hardware
/// encoders default to constant bitrate, so a still menu or a static desktop
/// spent exactly as many bits as a fast pan. Variable bitrate holds the same
/// average target for buffer sizing while letting quiet frames cost a fraction
/// of it, which is where most of a replay clip's size actually goes.
///
/// Every setting is best effort. Encoders differ in what they expose and a
/// refusal only means the previous default stays in place.
#[cfg(target_os = "windows")]
fn configure_rate_control(
    transform: &windows::Win32::Media::MediaFoundation::IMFTransform,
    settings: EncoderSettings,
) {
    use windows::Win32::Media::MediaFoundation::{
        CODECAPI_AVEncCommonMaxBitRate, CODECAPI_AVEncCommonMeanBitRate,
        CODECAPI_AVEncCommonQualityVsSpeed, CODECAPI_AVEncCommonRateControlMode, ICodecAPI,
        eAVEncCommonRateControlMode_PeakConstrainedVBR,
    };
    use windows::core::Interface;

    let codec = match transform.cast::<ICodecAPI>() {
        Ok(codec) => codec,
        Err(error) => {
            wreath_core::diagnostic!(
                "Wreath video encoder: rate control is not configurable ({error}); keeping the encoder default"
            );
            return;
        }
    };
    let mean = settings.bitrate_kbps.saturating_mul(1_000);
    // Headroom for complex frames without letting a single scene run away.
    let peak = mean.saturating_add(mean / 2);
    let apply = |name: &str, key: &windows::core::GUID, value: u32| {
        let variant = unsigned_variant(value);
        if let Err(error) = unsafe { codec.SetValue(key, &variant) } {
            wreath_core::diagnostic!("Wreath video encoder: {name} was refused ({error})");
        }
    };
    apply(
        "variable bitrate",
        &CODECAPI_AVEncCommonRateControlMode,
        eAVEncCommonRateControlMode_PeakConstrainedVBR.0 as u32,
    );
    apply("mean bitrate", &CODECAPI_AVEncCommonMeanBitRate, mean);
    apply("peak bitrate", &CODECAPI_AVEncCommonMaxBitRate, peak);
    // Leans on compression efficiency rather than encoder speed; a replay
    // buffer has a whole frame interval to spare.
    apply("quality bias", &CODECAPI_AVEncCommonQualityVsSpeed, 70);
    wreath_core::diagnostic!(
        "Wreath video encoder: peak-constrained VBR, mean {mean} bit/s, peak {peak} bit/s"
    );
}

/// Wraps an unsigned value the way `ICodecAPI` expects its settings.
#[cfg(target_os = "windows")]
fn unsigned_variant(value: u32) -> windows::Win32::System::Variant::VARIANT {
    use windows::Win32::System::Variant::{VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_UI4};

    VARIANT {
        Anonymous: VARIANT_0 {
            Anonymous: std::mem::ManuallyDrop::new(VARIANT_0_0 {
                vt: VT_UI4,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: VARIANT_0_0_0 { ulVal: value },
            }),
        },
    }
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
