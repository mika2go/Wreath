use crate::audio::AudioError;
#[cfg(target_os = "windows")]
use crate::audio::Pcm16Chunk;

/// 192 kbit/s, the highest rate the Microsoft AAC encoder accepts.
///
/// AAC-LC at 128 kbit/s is not transparent on stereo material, and speech over
/// game audio is exactly the noisy, broadband content its artefacts show up on
/// as a faint gritty edge. The extra 8 kB/s is nothing against the video.
pub const AAC_BYTES_PER_SECOND: u32 = 24_000;

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

/// Keeps the PCM handed to the encoder contiguous with the timeline that
/// describes it.
///
/// WASAPI hands over whatever frames it has; when the endpoint or the queue
/// loses some, the next packet simply carries a later timestamp. Submitting
/// that straight to the encoder splices two unrelated waveforms together while
/// the timeline jumps over the hole, so the AAC frames stop lining up with the
/// times attached to them. Bridging the hole with silence keeps the payload and
/// the timeline describing the same thing.
#[cfg(any(target_os = "windows", test))]
#[derive(Default)]
struct EncoderTimeline {
    next_timestamp: Option<std::time::Duration>,
    tail: Vec<i16>,
}

#[cfg(any(target_os = "windows", test))]
#[derive(Debug, PartialEq, Eq)]
enum TimelinePlan {
    /// This packet continues directly from the previous one.
    Continue { timestamp: std::time::Duration },
    /// Fill the hole ahead of this packet with silence.
    Bridge {
        timestamp: std::time::Duration,
        silent_frames: u32,
    },
    /// Too far to bridge; drop the old timeline and start over here.
    Restart { timestamp: std::time::Duration },
}

#[cfg(any(target_os = "windows", test))]
impl EncoderTimeline {
    /// Longest hole worth filling. Anything beyond this is a pause, a device
    /// change or a resume, where silence would only pad the clip.
    const MAX_BRIDGE: std::time::Duration = std::time::Duration::from_secs(1);

    fn plan(&self, timestamp: std::time::Duration, sample_rate: u32) -> TimelinePlan {
        let Some(expected) = self.next_timestamp else {
            return TimelinePlan::Restart { timestamp };
        };
        if timestamp <= expected {
            // Never move backwards: the encoder needs a monotonic timeline and
            // the payload is contiguous regardless of what the clock reported.
            return TimelinePlan::Continue {
                timestamp: expected,
            };
        }
        let gap = timestamp.saturating_sub(expected);
        if gap > Self::MAX_BRIDGE {
            return TimelinePlan::Restart { timestamp };
        }
        let silent_frames = duration_frames(gap, sample_rate);
        if silent_frames == 0 {
            return TimelinePlan::Continue {
                timestamp: expected,
            };
        }
        TimelinePlan::Bridge {
            timestamp: expected,
            silent_frames,
        }
    }

    fn advance(&mut self, timestamp: std::time::Duration, frames: u32, sample_rate: u32) {
        self.next_timestamp = Some(timestamp.saturating_add(frames_duration(frames, sample_rate)));
    }

    fn remember_tail(&mut self, data: &[u8], channels: u16) {
        let channels = usize::from(channels.max(1));
        self.tail.clear();
        let frame_bytes = channels * 2;
        if data.len() < frame_bytes {
            return;
        }
        for sample in data[data.len() - frame_bytes..].chunks_exact(2) {
            self.tail.push(i16::from_le_bytes([sample[0], sample[1]]));
        }
    }

    fn reset(&mut self) {
        self.next_timestamp = None;
        self.tail.clear();
    }
}

#[cfg(any(target_os = "windows", test))]
fn duration_frames(duration: std::time::Duration, sample_rate: u32) -> u32 {
    u32::try_from(duration.as_nanos().saturating_mul(u128::from(sample_rate)) / 1_000_000_000)
        .unwrap_or(u32::MAX)
}

#[cfg(any(target_os = "windows", test))]
fn frames_duration(frames: u32, sample_rate: u32) -> std::time::Duration {
    std::time::Duration::from_nanos(
        u64::from(frames).saturating_mul(1_000_000_000) / u64::from(sample_rate.max(1)),
    )
}

/// Number of frames a splice is ramped over, so neither edge of a bridged hole
/// is a step in the waveform.
#[cfg(any(target_os = "windows", test))]
fn splice_fade_frames(sample_rate: u32) -> u32 {
    (sample_rate / 500).max(1)
}

/// Silence that starts from wherever the last packet left off, so entering the
/// hole is a short ramp rather than a jump to zero.
#[cfg(any(target_os = "windows", test))]
fn bridging_silence(frames: u32, channels: u16, tail: &[i16], sample_rate: u32) -> Vec<u8> {
    let channels = usize::from(channels.max(1));
    let mut data = vec![0_u8; usize::try_from(frames).unwrap_or_default() * channels * 2];
    let fade = splice_fade_frames(sample_rate).min(frames);
    if fade == 0 || tail.len() < channels {
        return data;
    }
    for frame in 0..fade {
        let remaining = i64::from(fade - frame);
        let base = usize::try_from(frame).unwrap_or_default() * channels * 2;
        for (channel, last) in tail.iter().take(channels).enumerate() {
            let index = base + channel * 2;
            let ramped = i64::from(*last) * remaining / i64::from(fade);
            data[index..index + 2].copy_from_slice(&(ramped as i16).to_le_bytes());
        }
    }
    data
}

/// Ramps the first frames of a packet up from zero after a hole.
#[cfg(any(target_os = "windows", test))]
fn fade_in(data: &mut [u8], frames: u32, channels: u16, sample_rate: u32) {
    let channels = usize::from(channels.max(1));
    let fade = splice_fade_frames(sample_rate).min(frames);
    if fade <= 1 {
        return;
    }
    let denominator = i64::from(fade - 1);
    for frame in 0..fade {
        for channel in 0..channels {
            let index = (usize::try_from(frame).unwrap_or_default() * channels + channel) * 2;
            let sample = i16::from_le_bytes([data[index], data[index + 1]]);
            let ramped = i64::from(sample) * i64::from(frame) / denominator;
            data[index..index + 2].copy_from_slice(&(ramped as i16).to_le_bytes());
        }
    }
}

#[cfg(target_os = "windows")]
pub struct AacEncoder {
    transform: windows::Win32::Media::MediaFoundation::IMFTransform,
    settings: AudioEncoderSettings,
    track: wreath_core::replay_buffer::TrackKind,
    timeline: EncoderTimeline,
}

#[cfg(target_os = "windows")]
impl AacEncoder {
    pub fn initialize(settings: AudioEncoderSettings) -> Result<Self, AudioError> {
        Self::initialize_for_track(settings, wreath_core::replay_buffer::TrackKind::Audio)
    }

    pub fn initialize_for_track(
        settings: AudioEncoderSettings,
        track: wreath_core::replay_buffer::TrackKind,
    ) -> Result<Self, AudioError> {
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
            track,
            timeline: EncoderTimeline::default(),
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
        &mut self,
        mut chunk: Pcm16Chunk,
    ) -> Result<Vec<wreath_core::replay_buffer::EncodedPacket>, AudioError> {
        let sample_rate = self.settings.sample_rate;
        let channels = self.settings.channels;
        let mut packets = Vec::new();
        match self.timeline.plan(chunk.timestamp, sample_rate) {
            TimelinePlan::Continue { timestamp } => chunk.timestamp = timestamp,
            TimelinePlan::Bridge {
                timestamp,
                silent_frames,
            } => {
                let silence = Pcm16Chunk {
                    timestamp,
                    frames: silent_frames,
                    discontinuous: true,
                    data: bridging_silence(
                        silent_frames,
                        channels,
                        &self.timeline.tail,
                        sample_rate,
                    )
                    .into_boxed_slice(),
                };
                let silence_end =
                    timestamp.saturating_add(frames_duration(silent_frames, sample_rate));
                self.submit(&silence)?;
                packets.extend(self.take_available_packets()?);
                chunk.timestamp = silence_end;
                fade_in(&mut chunk.data, chunk.frames, channels, sample_rate);
                wreath_core::diagnostic!(
                    "Wreath audio encoder: bridged a {} ms capture hole with silence",
                    frames_duration(silent_frames, sample_rate).as_millis()
                );
            }
            TimelinePlan::Restart { timestamp } => {
                chunk.timestamp = timestamp;
                fade_in(&mut chunk.data, chunk.frames, channels, sample_rate);
            }
        }
        self.submit(&chunk)?;
        self.timeline.remember_tail(&chunk.data, channels);
        self.timeline
            .advance(chunk.timestamp, chunk.frames, sample_rate);
        packets.extend(self.take_available_packets()?);
        Ok(packets)
    }

    pub fn flush(&mut self) -> Result<(), AudioError> {
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

        let result = flush().map_err(audio_error);
        self.timeline.reset();
        result
    }

    fn submit(&self, chunk: &Pcm16Chunk) -> Result<(), AudioError> {
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
            track: self.track,
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
        assert_eq!(stereo.bytes_per_second, 24_000);
        assert_eq!(stereo.pcm_bytes_per_second(), 192_000);

        let surround = AudioEncoderSettings::for_capture(48_000, 6).unwrap();
        assert_eq!(surround.bytes_per_second, 144_000);
        assert_eq!(surround.block_align(), 12);
    }

    use std::time::Duration;

    fn timeline_at(next: Duration) -> EncoderTimeline {
        EncoderTimeline {
            next_timestamp: Some(next),
            tail: vec![10_000],
        }
    }

    #[test]
    fn a_contiguous_packet_keeps_the_timeline() {
        let timeline = timeline_at(Duration::from_millis(100));

        assert_eq!(
            timeline.plan(Duration::from_millis(100), 48_000),
            TimelinePlan::Continue {
                timestamp: Duration::from_millis(100)
            }
        );
    }

    /// A capture hole used to splice two unrelated waveforms together while the
    /// timeline jumped over it.
    #[test]
    fn a_capture_hole_is_bridged_with_silence() {
        let timeline = timeline_at(Duration::from_millis(100));

        assert_eq!(
            timeline.plan(Duration::from_millis(140), 48_000),
            TimelinePlan::Bridge {
                timestamp: Duration::from_millis(100),
                silent_frames: 1_920,
            }
        );
    }

    #[test]
    fn an_overlapping_packet_never_moves_the_timeline_backwards() {
        let timeline = timeline_at(Duration::from_millis(100));

        assert_eq!(
            timeline.plan(Duration::from_millis(80), 48_000),
            TimelinePlan::Continue {
                timestamp: Duration::from_millis(100)
            }
        );
    }

    #[test]
    fn a_pause_sized_hole_restarts_instead_of_padding() {
        let timeline = timeline_at(Duration::from_millis(100));

        assert_eq!(
            timeline.plan(Duration::from_secs(30), 48_000),
            TimelinePlan::Restart {
                timestamp: Duration::from_secs(30)
            }
        );
        assert_eq!(
            EncoderTimeline::default().plan(Duration::from_secs(5), 48_000),
            TimelinePlan::Restart {
                timestamp: Duration::from_secs(5)
            }
        );
    }

    #[test]
    fn the_timeline_advances_by_payload_and_clears_on_reset() {
        let mut timeline = EncoderTimeline::default();
        timeline.advance(Duration::from_millis(10), 480, 48_000);

        assert_eq!(
            timeline.plan(Duration::from_millis(20), 48_000),
            TimelinePlan::Continue {
                timestamp: Duration::from_millis(20)
            }
        );
        assert_eq!(
            frames_duration(480, 48_000),
            Duration::from_millis(10),
            "one packet is exactly its own frame count"
        );

        // Stereo tail keeps the final frame of each channel for the ramp.
        let data = [1_i16, 2, 3, 4]
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect::<Vec<_>>();
        timeline.remember_tail(&data, 2);
        assert_eq!(timeline.tail, [3, 4]);

        timeline.reset();
        assert!(timeline.tail.is_empty());
        assert_eq!(
            timeline.plan(Duration::from_millis(20), 48_000),
            TimelinePlan::Restart {
                timestamp: Duration::from_millis(20)
            }
        );
    }

    #[test]
    fn both_edges_of_a_bridged_hole_are_ramped() {
        let silence = bridging_silence(480, 1, &[10_000], 48_000);
        let ramp = silence
            .chunks_exact(2)
            .map(|sample| i16::from_le_bytes(sample.try_into().unwrap()))
            .collect::<Vec<_>>();

        assert_eq!(ramp[0], 10_000);
        assert!(ramp[1] < ramp[0]);
        assert_eq!(ramp[96], 0);
        assert_eq!(ramp[479], 0);

        let mut resumed = vec![0_u8; 480 * 2];
        for sample in resumed.chunks_exact_mut(2) {
            sample.copy_from_slice(&12_000_i16.to_le_bytes());
        }
        fade_in(&mut resumed, 480, 1, 48_000);
        let resumed = resumed
            .chunks_exact(2)
            .map(|sample| i16::from_le_bytes(sample.try_into().unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(resumed[0], 0);
        assert_eq!(resumed[95], 12_000);
        assert_eq!(resumed[479], 12_000);
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
