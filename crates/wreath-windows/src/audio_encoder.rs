use crate::audio::AudioError;
#[cfg(target_os = "windows")]
use crate::audio::Pcm16Chunk;

pub const AAC_BYTES_PER_SECOND: u32 = 16_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioEncoderSettings {
    pub sample_rate: u32,
    pub channels: u16,
    pub bytes_per_second: u32,
}

impl AudioEncoderSettings {
    pub fn for_capture(sample_rate: u32, channels: u16) -> Result<Self, AudioError> {
        let settings = Self {
            sample_rate,
            channels,
            bytes_per_second: if channels == 6 {
                AAC_BYTES_PER_SECOND * 6
            } else {
                AAC_BYTES_PER_SECOND
            },
        };
        settings.validate()
    }

    pub fn validate(self) -> Result<Self, AudioError> {
        if !matches!(self.sample_rate, 44_100 | 48_000) {
            return Err(AudioError(format!(
                "AAC requires a 44.1 or 48 kHz endpoint; got {} Hz",
                self.sample_rate
            )));
        }
        if !matches!(self.channels, 1 | 2 | 6) {
            return Err(AudioError(format!(
                "AAC requires 1, 2, or 6 channels; got {}",
                self.channels
            )));
        }
        let base_rate = self.bytes_per_second / u32::from(self.channels.max(1));
        let valid_rate = if self.channels == 6 {
            self.bytes_per_second % 6 == 0 && matches!(base_rate, 12_000 | 16_000 | 20_000 | 24_000)
        } else {
            matches!(self.bytes_per_second, 12_000 | 16_000 | 20_000 | 24_000)
        };
        if !valid_rate {
            return Err(AudioError(format!(
                "unsupported AAC byte rate: {}",
                self.bytes_per_second
            )));
        }
        Ok(self)
    }

    pub fn block_align(self) -> u32 {
        u32::from(self.channels) * 2
    }

    pub fn pcm_bytes_per_second(self) -> u32 {
        self.sample_rate.saturating_mul(self.block_align())
    }
}

#[cfg(target_os = "windows")]
pub struct AacEncoder {
    transform: windows::Win32::Media::MediaFoundation::IMFTransform,
    settings: AudioEncoderSettings,
}

#[cfg(target_os = "windows")]
impl AacEncoder {
    pub fn initialize(settings: AudioEncoderSettings) -> Result<Self, AudioError> {
        use windows::Win32::Media::MediaFoundation::{
            AACMFTEncoder, IMFTransform, MFT_MESSAGE_NOTIFY_BEGIN_STREAMING,
            MFT_MESSAGE_NOTIFY_START_OF_STREAM,
        };
        use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};

        let settings = settings.validate()?;
        let transform: IMFTransform =
            unsafe { CoCreateInstance(&AACMFTEncoder, None, CLSCTX_INPROC_SERVER) }
                .map_err(audio_error)?;
        let output = output_media_type(settings)?;
        let input = input_media_type(settings)?;
        unsafe { transform.SetOutputType(0, &output, 0) }.map_err(audio_error)?;
        unsafe { transform.SetInputType(0, &input, 0) }.map_err(audio_error)?;
        unsafe { transform.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0) }
            .map_err(audio_error)?;
        unsafe { transform.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0) }
            .map_err(audio_error)?;
        Ok(Self {
            transform,
            settings,
        })
    }

    pub fn settings(&self) -> AudioEncoderSettings {
        self.settings
    }

    pub fn output_media_type(
        &self,
    ) -> Result<windows::Win32::Media::MediaFoundation::IMFMediaType, AudioError> {
        unsafe { self.transform.GetOutputCurrentType(0) }.map_err(audio_error)
    }

    pub fn encode(
        &self,
        chunk: Pcm16Chunk,
    ) -> Result<Vec<wreath_core::replay_buffer::EncodedPacket>, AudioError> {
        self.submit(chunk)?;
        self.take_available_packets()
    }

    pub fn flush(&self) -> Result<(), AudioError> {
        use windows::Win32::Media::MediaFoundation::{
            MFT_MESSAGE_COMMAND_FLUSH, MFT_MESSAGE_NOTIFY_START_OF_STREAM,
        };

        let flush = || -> windows::core::Result<()> {
            unsafe {
                self.transform
                    .ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0)?;
                self.transform
                    .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;
            }
            Ok(())
        };

        flush().map_err(audio_error)
    }

    fn submit(&self, chunk: Pcm16Chunk) -> Result<(), AudioError> {
        use windows::Win32::Media::MediaFoundation::{MFCreateMemoryBuffer, MFCreateSample};

        let expected_length = usize::try_from(chunk.frames)
            .ok()
            .and_then(|frames| frames.checked_mul(self.settings.block_align() as usize))
            .ok_or_else(|| AudioError("PCM packet size overflow".into()))?;
        if chunk.frames == 0 || chunk.data.len() != expected_length {
            return Err(AudioError(format!(
                "PCM packet has {} bytes; expected {expected_length}",
                chunk.data.len()
            )));
        }
        let payload_length = u32::try_from(chunk.data.len())
            .map_err(|_| AudioError("PCM packet exceeds Media Foundation limits".into()))?;
        let buffer = unsafe { MFCreateMemoryBuffer(payload_length) }.map_err(audio_error)?;
        let mut destination = std::ptr::null_mut();
        unsafe { buffer.Lock(&mut destination, None, None) }.map_err(audio_error)?;
        if destination.is_null() {
            let _ = unsafe { buffer.Unlock() };
            return Err(AudioError(
                "Media Foundation returned a null PCM buffer".into(),
            ));
        }
        unsafe {
            std::ptr::copy_nonoverlapping(chunk.data.as_ptr(), destination, chunk.data.len())
        };
        unsafe { buffer.Unlock() }.map_err(audio_error)?;
        unsafe { buffer.SetCurrentLength(payload_length) }.map_err(audio_error)?;

        let sample = unsafe { MFCreateSample() }.map_err(audio_error)?;
        let submit = || -> windows::core::Result<()> {
            unsafe {
                sample.AddBuffer(&buffer)?;
                sample.SetSampleTime(duration_to_hns(chunk.timestamp))?;
                sample.SetSampleDuration(frames_to_hns(chunk.frames, self.settings.sample_rate))?;
                self.transform.ProcessInput(0, &sample, 0)?;
            }
            Ok(())
        };
        submit().map_err(audio_error)
    }

    fn take_available_packets(
        &self,
    ) -> Result<Vec<wreath_core::replay_buffer::EncodedPacket>, AudioError> {
        let mut packets = Vec::new();
        while let Some(packet) = self.take_packet()? {
            packets.push(packet);
        }
        Ok(packets)
    }

    fn take_packet(&self) -> Result<Option<wreath_core::replay_buffer::EncodedPacket>, AudioError> {
        use std::mem::ManuallyDrop;
        use std::time::Duration;

        use windows::Win32::Media::MediaFoundation::{
            IMFSample, MF_E_TRANSFORM_NEED_MORE_INPUT, MFCreateMemoryBuffer, MFCreateSample,
            MFT_OUTPUT_DATA_BUFFER, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES,
        };

        let stream_info = unsafe { self.transform.GetOutputStreamInfo(0) }.map_err(audio_error)?;
        let provides_sample =
            stream_info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 != 0;
        let supplied_sample = if provides_sample {
            None
        } else {
            let sample = unsafe { MFCreateSample() }.map_err(audio_error)?;
            let buffer =
                unsafe { MFCreateMemoryBuffer(stream_info.cbSize.max(1)) }.map_err(audio_error)?;
            unsafe { sample.AddBuffer(&buffer) }.map_err(audio_error)?;
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
            return Err(audio_error(error));
        }
        let sample: IMFSample = output_sample
            .ok_or_else(|| AudioError("AAC encoder produced no output sample".into()))?;
        let buffer = unsafe { sample.ConvertToContiguousBuffer() }.map_err(audio_error)?;
        let mut data = std::ptr::null_mut();
        let mut length = 0_u32;
        unsafe { buffer.Lock(&mut data, None, Some(&mut length)) }.map_err(audio_error)?;
        let payload: std::sync::Arc<[u8]> = if data.is_null() || length == 0 {
            std::sync::Arc::from([])
        } else {
            std::sync::Arc::from(unsafe { std::slice::from_raw_parts(data, length as usize) })
        };
        unsafe { buffer.Unlock() }.map_err(audio_error)?;
        if payload.is_empty() {
            return Err(AudioError("AAC encoder returned an empty packet".into()));
        }

        let timestamp_hns = unsafe { sample.GetSampleTime() }.unwrap_or_default().max(0) as u64;
        let default_duration = frames_to_hns(1024, self.settings.sample_rate);
        let duration_hns = unsafe { sample.GetSampleDuration() }
            .unwrap_or(default_duration)
            .max(0) as u64;
        Ok(Some(wreath_core::replay_buffer::EncodedPacket {
            track: wreath_core::replay_buffer::TrackKind::Audio,
            timestamp: Duration::from_nanos(timestamp_hns.saturating_mul(100)),
            duration: Duration::from_nanos(duration_hns.saturating_mul(100)),
            keyframe: false,
            payload,
        }))
    }
}

#[cfg(target_os = "windows")]
fn input_media_type(
    settings: AudioEncoderSettings,
) -> Result<windows::Win32::Media::MediaFoundation::IMFMediaType, AudioError> {
    use windows::Win32::Media::MediaFoundation::{
        MF_MT_ALL_SAMPLES_INDEPENDENT, MF_MT_AUDIO_AVG_BYTES_PER_SECOND,
        MF_MT_AUDIO_BITS_PER_SAMPLE, MF_MT_AUDIO_BLOCK_ALIGNMENT, MF_MT_AUDIO_NUM_CHANNELS,
        MF_MT_AUDIO_SAMPLES_PER_SECOND, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MFAudioFormat_PCM,
        MFCreateMediaType, MFMediaType_Audio,
    };

    let media_type = unsafe { MFCreateMediaType() }.map_err(audio_error)?;
    let configure = || -> windows::core::Result<()> {
        unsafe {
            media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
            media_type.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM)?;
            media_type.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
            media_type.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, settings.sample_rate)?;
            media_type.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, u32::from(settings.channels))?;
            media_type.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, settings.block_align())?;
            media_type.SetUINT32(
                &MF_MT_AUDIO_AVG_BYTES_PER_SECOND,
                settings.pcm_bytes_per_second(),
            )?;
            media_type.SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 1)?;
        }
        Ok(())
    };
    configure().map_err(audio_error)?;
    Ok(media_type)
}

#[cfg(target_os = "windows")]
fn output_media_type(
    settings: AudioEncoderSettings,
) -> Result<windows::Win32::Media::MediaFoundation::IMFMediaType, AudioError> {
    use windows::Win32::Media::MediaFoundation::{
        MF_MT_AAC_AUDIO_PROFILE_LEVEL_INDICATION, MF_MT_AAC_PAYLOAD_TYPE,
        MF_MT_AUDIO_AVG_BYTES_PER_SECOND, MF_MT_AUDIO_BITS_PER_SAMPLE, MF_MT_AUDIO_NUM_CHANNELS,
        MF_MT_AUDIO_SAMPLES_PER_SECOND, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MFAudioFormat_AAC,
        MFCreateMediaType, MFMediaType_Audio,
    };

    let media_type = unsafe { MFCreateMediaType() }.map_err(audio_error)?;
    let configure = || -> windows::core::Result<()> {
        unsafe {
            media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
            media_type.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_AAC)?;
            media_type.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
            media_type.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, settings.sample_rate)?;
            media_type.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, u32::from(settings.channels))?;
            media_type.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, settings.bytes_per_second)?;
            media_type.SetUINT32(&MF_MT_AAC_PAYLOAD_TYPE, 0)?;
            media_type.SetUINT32(&MF_MT_AAC_AUDIO_PROFILE_LEVEL_INDICATION, 0x29)?;
        }
        Ok(())
    };
    configure().map_err(audio_error)?;
    Ok(media_type)
}

#[cfg(target_os = "windows")]
fn duration_to_hns(duration: std::time::Duration) -> i64 {
    i64::try_from(duration.as_nanos() / 100).unwrap_or(i64::MAX)
}

#[cfg(target_os = "windows")]
fn frames_to_hns(frames: u32, sample_rate: u32) -> i64 {
    i64::from(frames).saturating_mul(10_000_000) / i64::from(sample_rate.max(1))
}

#[cfg(target_os = "windows")]
fn audio_error(error: windows::core::Error) -> AudioError {
    AudioError(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_bounded_aac_lc_rates() {
        let stereo = AudioEncoderSettings::for_capture(48_000, 2).unwrap();
        assert_eq!(stereo.bytes_per_second, 16_000);
        assert_eq!(stereo.pcm_bytes_per_second(), 192_000);

        let surround = AudioEncoderSettings::for_capture(48_000, 6).unwrap();
        assert_eq!(surround.bytes_per_second, 96_000);
        assert_eq!(surround.block_align(), 12);
    }

    #[test]
    fn rejects_formats_outside_the_microsoft_aac_contract() {
        assert!(AudioEncoderSettings::for_capture(96_000, 2).is_err());
        assert!(AudioEncoderSettings::for_capture(48_000, 8).is_err());
        assert!(
            AudioEncoderSettings {
                sample_rate: 48_000,
                channels: 2,
                bytes_per_second: 99,
            }
            .validate()
            .is_err()
        );
    }
}
