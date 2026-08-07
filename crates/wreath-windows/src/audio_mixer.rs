use std::collections::VecDeque;
use std::time::Duration;

use crate::audio::{AudioError, Pcm16Chunk};

const MAX_AUXILIARY_CHUNKS: usize = 32;

/// Timestamp-aware PCM16 mixer. The desktop stream is the clock master and the
/// much smaller microphone queue is capped so a delayed consumer cannot grow
/// memory indefinitely.
pub struct PcmMixer {
    sample_rate: u32,
    channels: u16,
    gain_percent: u16,
    microphone_converter: Option<PcmStreamConverter>,
    auxiliary: VecDeque<Pcm16Chunk>,
}

impl PcmMixer {
    pub fn new(sample_rate: u32, channels: u16, gain_percent: u16) -> Result<Self, AudioError> {
        if sample_rate == 0 || channels == 0 {
            return Err(AudioError("mixer format must not be empty".into()));
        }
        if gain_percent > 200 {
            return Err(AudioError("microphone gain exceeds 200 percent".into()));
        }
        Ok(Self {
            sample_rate,
            channels,
            gain_percent,
            microphone_converter: None,
            auxiliary: VecDeque::with_capacity(MAX_AUXILIARY_CHUNKS),
        })
    }

    pub fn push_auxiliary(
        &mut self,
        chunk: Pcm16Chunk,
        sample_rate: u32,
        channels: u16,
    ) -> Result<(), AudioError> {
        let converter = match &mut self.microphone_converter {
            Some(converter) => {
                converter.ensure_source_format(sample_rate, channels)?;
                converter
            }
            None => self
                .microphone_converter
                .insert(PcmStreamConverter::new_voice(
                    sample_rate,
                    channels,
                    self.sample_rate,
                    self.channels,
                )?),
        };
        let Some(converted) = converter.push(chunk)? else {
            return Ok(());
        };
        if self.auxiliary.len() == MAX_AUXILIARY_CHUNKS {
            self.auxiliary.pop_front();
        }
        self.auxiliary.push_back(converted);
        Ok(())
    }

    pub fn mix(&mut self, mut master: Pcm16Chunk) -> Result<Pcm16Chunk, AudioError> {
        validate_pcm16(&master, self.channels)?;
        let master_end = master
            .timestamp
            .saturating_add(frames_duration(master.frames, self.sample_rate));
        while self
            .auxiliary
            .front()
            .is_some_and(|chunk| chunk_end(chunk, self.sample_rate) <= master.timestamp)
        {
            self.auxiliary.pop_front();
        }

        for auxiliary in &self.auxiliary {
            if auxiliary.timestamp >= master_end {
                break;
            }
            mix_overlap(
                &mut master,
                auxiliary,
                self.sample_rate,
                self.channels,
                self.gain_percent,
            );
        }
        Ok(master)
    }

    #[cfg(test)]
    fn queued_chunks(&self) -> usize {
        self.auxiliary.len()
    }
}

/// Continuous PCM converter for a live capture stream. Unlike a packet-local
/// resampler it carries the fractional source position and the final source
/// frame across WASAPI packet boundaries, preventing a repeated/skipped sample
/// at every callback. The voice mode downmixes microphone arrays to one clean
/// voice channel before duplicating it into the output layout.
pub struct PcmStreamConverter {
    source_rate: u32,
    source_channels: u16,
    target_rate: u32,
    target_channels: u16,
    voice: bool,
    source: VecDeque<i16>,
    source_start_frame: u64,
    source_frames_received: u64,
    output_frames_emitted: u64,
    epoch_timestamp: Option<Duration>,
    expected_source_timestamp: Option<Duration>,
    restart_fade_frames_remaining: u32,
    restart_fade_frames_total: u32,
}

impl PcmStreamConverter {
    pub fn new_voice(
        source_rate: u32,
        source_channels: u16,
        target_rate: u32,
        target_channels: u16,
    ) -> Result<Self, AudioError> {
        Self::new(
            source_rate,
            source_channels,
            target_rate,
            target_channels,
            true,
        )
    }

    fn new(
        source_rate: u32,
        source_channels: u16,
        target_rate: u32,
        target_channels: u16,
        voice: bool,
    ) -> Result<Self, AudioError> {
        if source_rate == 0 || target_rate == 0 || source_channels == 0 || target_channels == 0 {
            return Err(AudioError("PCM stream format must not be empty".into()));
        }
        Ok(Self {
            source_rate,
            source_channels,
            target_rate,
            target_channels,
            voice,
            source: VecDeque::new(),
            source_start_frame: 0,
            source_frames_received: 0,
            output_frames_emitted: 0,
            epoch_timestamp: None,
            expected_source_timestamp: None,
            restart_fade_frames_remaining: 0,
            restart_fade_frames_total: 0,
        })
    }

    pub fn ensure_source_format(&self, sample_rate: u32, channels: u16) -> Result<(), AudioError> {
        if self.source_rate == sample_rate && self.source_channels == channels {
            Ok(())
        } else {
            Err(AudioError(format!(
                "microphone format changed from {} Hz/{} channels to {sample_rate} Hz/{channels} channels",
                self.source_rate, self.source_channels
            )))
        }
    }

    pub fn push(&mut self, chunk: Pcm16Chunk) -> Result<Option<Pcm16Chunk>, AudioError> {
        validate_pcm16(&chunk, self.source_channels)?;
        let restarted = self.epoch_timestamp.is_some()
            && (chunk.discontinuous || self.discontinuous_at(chunk.timestamp));
        if restarted {
            self.reset(chunk.timestamp, true);
        }
        let source_end = chunk
            .timestamp
            .saturating_add(frames_duration(chunk.frames, self.source_rate));
        self.expected_source_timestamp = Some(source_end);
        if self.epoch_timestamp.is_none() {
            self.epoch_timestamp = Some(chunk.timestamp);
        }

        let mapped = map_channels(
            &chunk.data,
            chunk.frames,
            self.source_channels,
            self.target_channels,
            self.voice,
        );
        if self.source_rate == self.target_rate {
            let mut data = mapped
                .into_iter()
                .flat_map(i16::to_le_bytes)
                .collect::<Vec<_>>();
            self.apply_restart_fade(&mut data, chunk.frames);
            return Ok(Some(Pcm16Chunk {
                timestamp: chunk.timestamp,
                frames: chunk.frames,
                discontinuous: restarted,
                data: data.into_boxed_slice(),
            }));
        }

        self.source.extend(mapped);
        self.source_frames_received = self
            .source_frames_received
            .saturating_add(u64::from(chunk.frames));
        let first_output_frame = self.output_frames_emitted;
        let mut output = Vec::new();
        loop {
            let source_position = self
                .output_frames_emitted
                .saturating_mul(u64::from(self.source_rate));
            let first_frame = source_position / u64::from(self.target_rate);
            let second_frame = first_frame.saturating_add(1);
            if second_frame >= self.source_frames_received {
                break;
            }
            let fraction = source_position % u64::from(self.target_rate);
            for channel in 0..self.target_channels {
                let first = self.buffered_sample(first_frame, channel)?;
                let second = self.buffered_sample(second_frame, channel)?;
                let interpolated = (i64::from(first)
                    * (i64::from(self.target_rate) - fraction as i64)
                    + i64::from(second) * fraction as i64)
                    / i64::from(self.target_rate);
                output.extend_from_slice(&(interpolated as i16).to_le_bytes());
            }
            self.output_frames_emitted = self.output_frames_emitted.saturating_add(1);
        }
        self.discard_consumed_source();
        let frames = self
            .output_frames_emitted
            .saturating_sub(first_output_frame);
        if frames == 0 {
            return Ok(None);
        }
        let timestamp = self
            .epoch_timestamp
            .unwrap_or(chunk.timestamp)
            .saturating_add(frames_duration_u64(first_output_frame, self.target_rate));
        let frames = u32::try_from(frames)
            .map_err(|_| AudioError("converted audio packet is too large".into()))?;
        self.apply_restart_fade(&mut output, frames);
        Ok(Some(Pcm16Chunk {
            timestamp,
            frames,
            discontinuous: restarted,
            data: output.into_boxed_slice(),
        }))
    }

    fn discontinuous_at(&self, timestamp: Duration) -> bool {
        const RESET_THRESHOLD: Duration = Duration::from_millis(100);
        self.expected_source_timestamp
            .is_some_and(|expected| expected.abs_diff(timestamp) > RESET_THRESHOLD)
    }

    fn reset(&mut self, timestamp: Duration, fade_in: bool) {
        self.source.clear();
        self.source_start_frame = 0;
        self.source_frames_received = 0;
        self.output_frames_emitted = 0;
        self.epoch_timestamp = Some(timestamp);
        self.expected_source_timestamp = None;
        if fade_in {
            let frames = (self.target_rate / 200).max(2);
            self.restart_fade_frames_remaining = frames;
            self.restart_fade_frames_total = frames;
        } else {
            self.restart_fade_frames_remaining = 0;
            self.restart_fade_frames_total = 0;
        }
    }

    fn apply_restart_fade(&mut self, data: &mut [u8], frames: u32) {
        if self.restart_fade_frames_remaining == 0 {
            return;
        }
        let frames_to_fade = frames.min(self.restart_fade_frames_remaining);
        let fade_start = self
            .restart_fade_frames_total
            .saturating_sub(self.restart_fade_frames_remaining);
        let denominator = self.restart_fade_frames_total.saturating_sub(1).max(1);
        for frame in 0..frames_to_fade {
            let numerator = fade_start.saturating_add(frame).min(denominator);
            for channel in 0..self.target_channels {
                let index = (usize::try_from(frame).unwrap_or_default()
                    * usize::from(self.target_channels)
                    + usize::from(channel))
                    * 2;
                let sample = i16::from_le_bytes([data[index], data[index + 1]]);
                let faded = i64::from(sample) * i64::from(numerator) / i64::from(denominator);
                data[index..index + 2].copy_from_slice(&(faded as i16).to_le_bytes());
            }
        }
        self.restart_fade_frames_remaining = self
            .restart_fade_frames_remaining
            .saturating_sub(frames_to_fade);
    }

    fn buffered_sample(&self, frame: u64, channel: u16) -> Result<i16, AudioError> {
        let frame = frame.checked_sub(self.source_start_frame).ok_or_else(|| {
            AudioError("resampler discarded a source frame before consuming it".into())
        })?;
        let index = frame
            .checked_mul(u64::from(self.target_channels))
            .and_then(|index| index.checked_add(u64::from(channel)))
            .and_then(|index| usize::try_from(index).ok())
            .ok_or_else(|| AudioError("resampler source index overflow".into()))?;
        self.source
            .get(index)
            .copied()
            .ok_or_else(|| AudioError("resampler source frame is unavailable".into()))
    }

    fn discard_consumed_source(&mut self) {
        let next_position = self
            .output_frames_emitted
            .saturating_mul(u64::from(self.source_rate));
        let keep_from = next_position / u64::from(self.target_rate);
        while self.source_start_frame < keep_from {
            for _ in 0..self.target_channels {
                let _ = self.source.pop_front();
            }
            self.source_start_frame += 1;
        }
    }
}

fn map_channels(
    data: &[u8],
    frames: u32,
    source_channels: u16,
    target_channels: u16,
    voice: bool,
) -> Vec<i16> {
    let mut mapped = Vec::with_capacity(
        usize::try_from(frames).unwrap_or_default() * usize::from(target_channels),
    );
    for frame in 0..frames {
        if voice {
            let voice = (0..source_channels)
                .map(|channel| i64::from(read_sample(data, frame, source_channels, channel)))
                .sum::<i64>()
                / i64::from(source_channels);
            mapped.extend(std::iter::repeat_n(
                voice as i16,
                usize::from(target_channels),
            ));
        } else {
            for channel in 0..target_channels {
                mapped.push(mapped_sample(
                    data,
                    frame,
                    source_channels,
                    channel,
                    target_channels,
                ));
            }
        }
    }
    mapped
}

pub fn adapt_pcm16(
    chunk: Pcm16Chunk,
    source_rate: u32,
    source_channels: u16,
    target_rate: u32,
    target_channels: u16,
) -> Result<Pcm16Chunk, AudioError> {
    if source_rate == 0 || target_rate == 0 || source_channels == 0 || target_channels == 0 {
        return Err(AudioError("PCM conversion format must not be empty".into()));
    }
    validate_pcm16(&chunk, source_channels)?;
    if source_rate == target_rate && source_channels == target_channels {
        return Ok(chunk);
    }

    let output_frames = u64::from(chunk.frames)
        .saturating_mul(u64::from(target_rate))
        .saturating_add(u64::from(source_rate / 2))
        / u64::from(source_rate);
    let output_frames = u32::try_from(output_frames)
        .map_err(|_| AudioError("resampled audio packet is too large".into()))?;
    let sample_count = usize::try_from(output_frames)
        .ok()
        .and_then(|frames| frames.checked_mul(usize::from(target_channels)))
        .ok_or_else(|| AudioError("resampled audio packet size overflow".into()))?;
    let mut output = Vec::with_capacity(
        sample_count
            .checked_mul(2)
            .ok_or_else(|| AudioError("resampled audio byte size overflow".into()))?,
    );
    for output_frame in 0..output_frames {
        let source_position = u64::from(output_frame).saturating_mul(u64::from(source_rate));
        let first_frame = (source_position / u64::from(target_rate))
            .min(u64::from(chunk.frames.saturating_sub(1))) as u32;
        let second_frame = first_frame
            .saturating_add(1)
            .min(chunk.frames.saturating_sub(1));
        let fraction = source_position % u64::from(target_rate);
        for channel in 0..target_channels {
            let first = mapped_sample(
                &chunk.data,
                first_frame,
                source_channels,
                channel,
                target_channels,
            );
            let second = mapped_sample(
                &chunk.data,
                second_frame,
                source_channels,
                channel,
                target_channels,
            );
            let interpolated = (i64::from(first) * (i64::from(target_rate) - fraction as i64)
                + i64::from(second) * fraction as i64)
                / i64::from(target_rate);
            output.extend_from_slice(&(interpolated as i16).to_le_bytes());
        }
    }
    Ok(Pcm16Chunk {
        timestamp: chunk.timestamp,
        frames: output_frames,
        discontinuous: chunk.discontinuous,
        data: output.into_boxed_slice(),
    })
}

pub fn apply_gain_pcm16(
    chunk: &mut Pcm16Chunk,
    channels: u16,
    gain_percent: u16,
) -> Result<(), AudioError> {
    validate_pcm16(chunk, channels)?;
    if gain_percent > 200 {
        return Err(AudioError("microphone gain exceeds 200 percent".into()));
    }
    // Never amplify a microphone-only stream above its Windows capture level.
    // Digital boost raises the endpoint noise floor and then clips voice peaks;
    // values above 100 remain accepted for old configs but resolve to unity.
    let effective_gain = gain_percent.min(100);
    for sample in chunk.data.chunks_exact_mut(2) {
        let value = i16::from_le_bytes([sample[0], sample[1]]);
        let adjusted = i32::from(value) * i32::from(effective_gain) / 100;
        let adjusted = adjusted as i16;
        sample.copy_from_slice(&adjusted.to_le_bytes());
    }
    Ok(())
}

fn validate_pcm16(chunk: &Pcm16Chunk, channels: u16) -> Result<(), AudioError> {
    let expected = usize::try_from(chunk.frames)
        .ok()
        .and_then(|frames| frames.checked_mul(usize::from(channels)))
        .and_then(|samples| samples.checked_mul(2))
        .ok_or_else(|| AudioError("PCM16 packet size overflow".into()))?;
    if chunk.frames == 0 || chunk.data.len() != expected {
        return Err(AudioError(format!(
            "PCM16 packet has {} bytes; expected {expected}",
            chunk.data.len()
        )));
    }
    Ok(())
}

fn mapped_sample(
    data: &[u8],
    frame: u32,
    source_channels: u16,
    target_channel: u16,
    target_channels: u16,
) -> i16 {
    if target_channels == 1 && source_channels > 1 {
        let sum = (0..source_channels)
            .map(|channel| i64::from(read_sample(data, frame, source_channels, channel)))
            .sum::<i64>();
        return (sum / i64::from(source_channels)) as i16;
    }
    let source_channel = if source_channels == 1 {
        if target_channels <= 2 || target_channel < 2 {
            Some(0)
        } else {
            None
        }
    } else if target_channel < source_channels {
        Some(target_channel)
    } else {
        None
    };
    source_channel.map_or(0, |channel| {
        read_sample(data, frame, source_channels, channel)
    })
}

fn read_sample(data: &[u8], frame: u32, channels: u16, channel: u16) -> i16 {
    let sample_index =
        usize::try_from(frame).unwrap_or(usize::MAX) * usize::from(channels) + usize::from(channel);
    let byte_index = sample_index * 2;
    i16::from_le_bytes([data[byte_index], data[byte_index + 1]])
}

fn mix_overlap(
    master: &mut Pcm16Chunk,
    auxiliary: &Pcm16Chunk,
    sample_rate: u32,
    channels: u16,
    gain_percent: u16,
) {
    let overlap_start = master.timestamp.max(auxiliary.timestamp);
    let master_end = chunk_end(master, sample_rate);
    let auxiliary_end = chunk_end(auxiliary, sample_rate);
    let overlap_end = master_end.min(auxiliary_end);
    if overlap_start >= overlap_end {
        return;
    }
    let master_offset =
        duration_frames(overlap_start.saturating_sub(master.timestamp), sample_rate);
    let auxiliary_offset = duration_frames(
        overlap_start.saturating_sub(auxiliary.timestamp),
        sample_rate,
    );
    let overlap_frames = duration_frames(overlap_end.saturating_sub(overlap_start), sample_rate)
        .min(master.frames.saturating_sub(master_offset))
        .min(auxiliary.frames.saturating_sub(auxiliary_offset));
    for frame in 0..overlap_frames {
        for channel in 0..channels {
            let (master_index, auxiliary_index) =
                overlap_sample_indices(master_offset, auxiliary_offset, frame, channel, channels);
            let base =
                i16::from_le_bytes([master.data[master_index], master.data[master_index + 1]]);
            let microphone = i16::from_le_bytes([
                auxiliary.data[auxiliary_index],
                auxiliary.data[auxiliary_index + 1],
            ]);
            let microphone_weight = i64::from(gain_percent);
            let denominator = 100 + microphone_weight;
            let mixed =
                (i64::from(base) * 100 + i64::from(microphone) * microphone_weight) / denominator;
            let mixed = mixed as i16;
            master.data[master_index..master_index + 2].copy_from_slice(&mixed.to_le_bytes());
        }
    }
}

fn overlap_sample_indices(
    master_offset: u32,
    auxiliary_offset: u32,
    frame: u32,
    channel: u16,
    channels: u16,
) -> (usize, usize) {
    let channel = usize::from(channel);
    let channels = usize::from(channels);
    let master_index = (usize::try_from(master_offset + frame).unwrap() * channels + channel) * 2;
    let auxiliary_index =
        (usize::try_from(auxiliary_offset + frame).unwrap() * channels + channel) * 2;
    (master_index, auxiliary_index)
}

fn chunk_end(chunk: &Pcm16Chunk, sample_rate: u32) -> Duration {
    chunk
        .timestamp
        .saturating_add(frames_duration(chunk.frames, sample_rate))
}

fn frames_duration(frames: u32, sample_rate: u32) -> Duration {
    frames_duration_u64(u64::from(frames), sample_rate)
}

fn frames_duration_u64(frames: u64, sample_rate: u32) -> Duration {
    Duration::from_nanos(frames.saturating_mul(1_000_000_000) / u64::from(sample_rate.max(1)))
}

fn duration_frames(duration: Duration, sample_rate: u32) -> u32 {
    u32::try_from(duration.as_nanos().saturating_mul(u128::from(sample_rate)) / 1_000_000_000)
        .unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(timestamp_ms: u64, channels: u16, samples: &[i16]) -> Pcm16Chunk {
        Pcm16Chunk {
            timestamp: Duration::from_millis(timestamp_ms),
            frames: u32::try_from(samples.len() / usize::from(channels)).unwrap(),
            discontinuous: false,
            data: samples
                .iter()
                .flat_map(|sample| sample.to_le_bytes())
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        }
    }

    fn samples(chunk: &Pcm16Chunk) -> Vec<i16> {
        chunk
            .data
            .chunks_exact(2)
            .map(|sample| i16::from_le_bytes(sample.try_into().unwrap()))
            .collect()
    }

    #[test]
    fn resamples_and_duplicates_mono_microphone_audio() {
        let converted = adapt_pcm16(chunk(0, 1, &[0, 10_000]), 24_000, 1, 48_000, 2).unwrap();

        assert_eq!(converted.frames, 4);
        assert_eq!(
            samples(&converted),
            [0, 0, 5_000, 5_000, 10_000, 10_000, 10_000, 10_000]
        );
    }

    #[test]
    fn streaming_resampler_is_continuous_across_packet_edges() {
        let mut converter = PcmStreamConverter::new_voice(24_000, 1, 48_000, 1).unwrap();
        let first = converter.push(chunk(0, 1, &[0, 10_000])).unwrap().unwrap();
        let second = converter
            .push(chunk(0, 1, &[20_000, 30_000]))
            .unwrap()
            .unwrap();

        assert_eq!(samples(&first), [0, 5_000]);
        assert_eq!(samples(&second), [10_000, 15_000, 20_000, 25_000]);
        assert_eq!(
            second.timestamp,
            Duration::from_nanos(2 * 1_000_000_000 / 48_000)
        );
    }

    #[test]
    fn voice_converter_downmixes_microphone_arrays_before_duplication() {
        let mut converter = PcmStreamConverter::new_voice(48_000, 2, 48_000, 2).unwrap();
        let converted = converter
            .push(chunk(0, 2, &[1_000, -1_000, 3_000, 1_000]))
            .unwrap()
            .unwrap();

        assert_eq!(samples(&converted), [0, 0, 2_000, 2_000]);
    }

    #[test]
    fn stream_converter_rejects_midstream_format_changes() {
        let converter = PcmStreamConverter::new_voice(48_000, 1, 48_000, 2).unwrap();
        assert!(converter.ensure_source_format(44_100, 1).is_err());
        assert!(converter.ensure_source_format(48_000, 2).is_err());
        assert!(converter.ensure_source_format(48_000, 1).is_ok());
    }

    #[test]
    fn stream_converter_starts_a_new_epoch_after_a_capture_gap() {
        let mut converter = PcmStreamConverter::new_voice(24_000, 1, 48_000, 1).unwrap();
        let _ = converter.push(chunk(0, 1, &[0, 10_000])).unwrap();
        let restarted = converter
            .push(chunk(1_000, 1, &[20_000, 30_000]))
            .unwrap()
            .unwrap();

        assert_eq!(restarted.timestamp, Duration::from_secs(1));
        assert_eq!(samples(&restarted), [0, 104]);
        assert!(restarted.discontinuous);
    }

    #[test]
    fn stream_converter_softens_an_explicit_wasapi_discontinuity() {
        let mut converter = PcmStreamConverter::new_voice(48_000, 1, 48_000, 1).unwrap();
        let _ = converter.push(chunk(0, 1, &[10_000; 480])).unwrap();
        let mut interrupted = chunk(10, 1, &[20_000; 240]);
        interrupted.discontinuous = true;

        let restarted = converter.push(interrupted).unwrap().unwrap();
        let restarted_samples = samples(&restarted);

        assert_eq!(restarted_samples[0], 0);
        assert_eq!(restarted_samples[239], 20_000);
        assert!(restarted.discontinuous);
    }

    #[test]
    fn mixes_only_timestamp_overlap_with_stable_headroom() {
        let mut mixer = PcmMixer::new(1_000, 1, 200).unwrap();
        mixer
            .push_auxiliary(chunk(1, 1, &[20_000, 10_000]), 1_000, 1)
            .unwrap();

        let mixed = mixer.mix(chunk(0, 1, &[1_000, 10_000, 10_000])).unwrap();
        assert_eq!(samples(&mixed), [1_000, 16_666, 10_000]);
    }

    #[test]
    fn microphone_queue_stays_bounded() {
        let mut mixer = PcmMixer::new(48_000, 2, 100).unwrap();
        for index in 0..100 {
            mixer
                .push_auxiliary(chunk(index, 1, &[0]), 48_000, 1)
                .unwrap();
        }
        assert_eq!(mixer.queued_chunks(), MAX_AUXILIARY_CHUNKS);
    }

    #[test]
    fn standalone_microphone_level_never_digitally_boosts_noise() {
        let mut audio = chunk(0, 1, &[20_000, -20_000]);
        apply_gain_pcm16(&mut audio, 1, 200).unwrap();
        assert_eq!(samples(&audio), [20_000, -20_000]);
        apply_gain_pcm16(&mut audio, 1, 50).unwrap();
        assert_eq!(samples(&audio), [10_000, -10_000]);
        assert!(apply_gain_pcm16(&mut audio, 1, 201).is_err());
    }
}
