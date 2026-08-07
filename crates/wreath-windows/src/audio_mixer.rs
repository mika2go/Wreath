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
            auxiliary: VecDeque::with_capacity(MAX_AUXILIARY_CHUNKS),
        })
    }

    pub fn push_auxiliary(
        &mut self,
        chunk: Pcm16Chunk,
        sample_rate: u32,
        channels: u16,
    ) -> Result<(), AudioError> {
        let converted = adapt_pcm16(
            chunk,
            sample_rate,
            channels,
            self.sample_rate,
            self.channels,
        )?;
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
    let peak = chunk
        .data
        .chunks_exact(2)
        .map(|sample| {
            let value = i16::from_le_bytes([sample[0], sample[1]]);
            i64::from(value) * i64::from(gain_percent) / 100
        })
        .map(i64::unsigned_abs)
        .max()
        .unwrap_or_default();
    for sample in chunk.data.chunks_exact_mut(2) {
        let value = i16::from_le_bytes([sample[0], sample[1]]);
        let amplified = i64::from(value) * i64::from(gain_percent) / 100;
        let adjusted = limit_peak(amplified, peak);
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
    let mut peak = 0_u64;
    for frame in 0..overlap_frames {
        for channel in 0..channels {
            let (master_index, auxiliary_index) =
                overlap_sample_indices(master_offset, auxiliary_offset, frame, channel, channels);
            peak = peak.max(
                combined_sample(
                    master,
                    auxiliary,
                    master_index,
                    auxiliary_index,
                    gain_percent,
                )
                .unsigned_abs(),
            );
        }
    }
    for frame in 0..overlap_frames {
        for channel in 0..channels {
            let (master_index, auxiliary_index) =
                overlap_sample_indices(master_offset, auxiliary_offset, frame, channel, channels);
            let combined = combined_sample(
                master,
                auxiliary,
                master_index,
                auxiliary_index,
                gain_percent,
            );
            let mixed = limit_peak(combined, peak);
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

fn combined_sample(
    master: &Pcm16Chunk,
    auxiliary: &Pcm16Chunk,
    master_index: usize,
    auxiliary_index: usize,
    gain_percent: u16,
) -> i64 {
    let base = i16::from_le_bytes([master.data[master_index], master.data[master_index + 1]]);
    let added = i16::from_le_bytes([
        auxiliary.data[auxiliary_index],
        auxiliary.data[auxiliary_index + 1],
    ]);
    i64::from(base) + i64::from(added) * i64::from(gain_percent) / 100
}

/// Applies one gain factor to the complete packet/overlap instead of clipping
/// individual samples. That preserves the microphone waveform at loud peaks
/// and avoids the flat-topped distortion produced by per-sample saturation.
fn limit_peak(sample: i64, peak: u64) -> i16 {
    const MAXIMUM: u64 = i16::MAX as u64;
    let limited = if peak > MAXIMUM {
        sample * i64::try_from(MAXIMUM).unwrap() / i64::try_from(peak).unwrap()
    } else {
        sample
    };
    limited.clamp(i64::from(i16::MIN), i64::from(i16::MAX)) as i16
}

fn chunk_end(chunk: &Pcm16Chunk, sample_rate: u32) -> Duration {
    chunk
        .timestamp
        .saturating_add(frames_duration(chunk.frames, sample_rate))
}

fn frames_duration(frames: u32, sample_rate: u32) -> Duration {
    Duration::from_nanos(
        u64::from(frames).saturating_mul(1_000_000_000) / u64::from(sample_rate.max(1)),
    )
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
    fn mixes_only_timestamp_overlap_without_flat_top_clipping() {
        let mut mixer = PcmMixer::new(1_000, 1, 200).unwrap();
        mixer
            .push_auxiliary(chunk(1, 1, &[20_000, 10_000]), 1_000, 1)
            .unwrap();

        let mixed = mixer.mix(chunk(0, 1, &[1_000, 10_000, 10_000])).unwrap();
        assert_eq!(samples(&mixed), [1_000, i16::MAX, 19_660]);
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
    fn standalone_microphone_gain_is_bounded_and_clamped() {
        let mut audio = chunk(0, 1, &[20_000, -20_000]);
        apply_gain_pcm16(&mut audio, 1, 200).unwrap();
        assert_eq!(samples(&audio), [i16::MAX, -i16::MAX]);
        assert!(apply_gain_pcm16(&mut audio, 1, 201).is_err());
    }
}
