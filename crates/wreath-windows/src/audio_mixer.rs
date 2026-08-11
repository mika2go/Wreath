use std::collections::VecDeque;
use std::time::Duration;

use crate::audio::{AudioError, Pcm16Chunk};

/// Timestamp-aware PCM16 mixer. The desktop stream is the clock master; the
/// microphone alignment is established once and held, because re-deriving it per
/// packet stepped the waveform whenever the sub-sample phase crossed a frame.
pub struct PcmMixer {
    sample_rate: u32,
    channels: u16,
    master_gain_percent: u16,
    gain_percent: u16,
    microphone_converter: Option<PcmStreamConverter>,
    auxiliary: VecDeque<i16>,
    /// Capture time of the frame the buffer started at.
    auxiliary_origin: Option<Duration>,
    /// Frames dropped from the front since that origin.
    auxiliary_consumed: u64,
    /// End of the most recently appended microphone packet.
    auxiliary_end: Option<Duration>,
    aligned: bool,
    fade_frames_remaining: u32,
    fade_frames_total: u32,
}

impl PcmMixer {
    /// Ordinary clock drift needs minutes to cover this.
    const ALIGNMENT_TOLERANCE: Duration = Duration::from_millis(10);
    /// Longest microphone gap bridged with silence instead of restarting.
    const MAX_BRIDGE: Duration = Duration::from_millis(200);

    pub fn new(sample_rate: u32, channels: u16, gain_percent: u16) -> Result<Self, AudioError> {
        Self::new_with_gains(sample_rate, channels, 100, gain_percent)
    }

    pub fn new_with_gains(
        sample_rate: u32,
        channels: u16,
        master_gain_percent: u16,
        gain_percent: u16,
    ) -> Result<Self, AudioError> {
        if sample_rate == 0 || channels == 0 {
            return Err(AudioError("mixer format must not be empty".into()));
        }
        if master_gain_percent > 200 || gain_percent > 200 {
            return Err(AudioError("mixer gain exceeds 200 percent".into()));
        }
        Ok(Self {
            sample_rate,
            channels,
            master_gain_percent,
            gain_percent,
            microphone_converter: None,
            auxiliary: VecDeque::new(),
            auxiliary_origin: None,
            auxiliary_consumed: 0,
            auxiliary_end: None,
            aligned: false,
            fade_frames_remaining: 0,
            fade_frames_total: 0,
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

        match self.auxiliary_end {
            // A hole would splice unrelated waveforms, so bridge it or start over.
            Some(end) if converted.timestamp > end => {
                let gap = converted.timestamp.saturating_sub(end);
                if gap > Self::MAX_BRIDGE {
                    self.restart_auxiliary(converted.timestamp);
                } else {
                    let silent_frames = duration_frames(gap, self.sample_rate);
                    self.auxiliary.extend(std::iter::repeat_n(
                        0_i16,
                        usize::try_from(silent_frames).unwrap_or_default()
                            * usize::from(self.channels),
                    ));
                }
            }
            Some(_) if converted.discontinuous => self.restart_auxiliary(converted.timestamp),
            None => self.restart_auxiliary(converted.timestamp),
            Some(_) => {}
        }
        self.auxiliary_end = Some(chunk_end(&converted, self.sample_rate));
        self.auxiliary.extend(
            converted
                .data
                .chunks_exact(2)
                .map(|sample| i16::from_le_bytes([sample[0], sample[1]])),
        );
        self.bound_auxiliary();
        Ok(())
    }

    pub fn mix(&mut self, mut master: Pcm16Chunk) -> Result<Pcm16Chunk, AudioError> {
        validate_pcm16(&master, self.channels)?;
        // Headroom comes from the whole master packet; scaling only the overlap
        // jumped the desktop level at the edge of every microphone gap.
        let denominator = i64::from(self.master_gain_percent)
            .saturating_add(i64::from(self.gain_percent))
            .max(1);
        scale_pcm16(
            &mut master,
            i64::from(self.master_gain_percent),
            denominator,
        );

        let Some(front) = self.auxiliary_front_timestamp() else {
            return Ok(master);
        };
        let mut master_offset = 0_u32;
        if !self.aligned || front.abs_diff(master.timestamp) > Self::ALIGNMENT_TOLERANCE {
            if front < master.timestamp {
                let stale =
                    duration_frames(master.timestamp.saturating_sub(front), self.sample_rate);
                self.drop_frames(u64::from(stale));
            } else {
                master_offset =
                    duration_frames(front.saturating_sub(master.timestamp), self.sample_rate);
            }
            self.begin_fade();
            self.aligned = true;
        }
        let Some(wanted) = master.frames.checked_sub(master_offset).filter(|w| *w > 0) else {
            return Ok(master);
        };
        let frames = wanted.min(self.buffered_frames());
        if frames == 0 {
            self.aligned = false;
            return Ok(master);
        }
        self.add_microphone(&mut master, master_offset, frames, denominator);
        self.drop_frames(u64::from(frames));
        if frames < wanted {
            // Re-deriving drops the stale remainder instead of shifting the voice.
            self.aligned = false;
        }
        Ok(master)
    }

    fn restart_auxiliary(&mut self, timestamp: Duration) {
        self.auxiliary.clear();
        self.auxiliary_origin = Some(timestamp);
        self.auxiliary_consumed = 0;
        self.aligned = false;
    }

    fn auxiliary_front_timestamp(&self) -> Option<Duration> {
        if self.auxiliary.is_empty() {
            return None;
        }
        self.auxiliary_origin.map(|origin| {
            origin.saturating_add(frames_duration_u64(
                self.auxiliary_consumed,
                self.sample_rate,
            ))
        })
    }

    fn buffered_frames(&self) -> u32 {
        u32::try_from(self.auxiliary.len() / usize::from(self.channels)).unwrap_or(u32::MAX)
    }

    fn drop_frames(&mut self, frames: u64) {
        let available = u64::from(self.buffered_frames());
        let frames = frames.min(available);
        let samples = usize::try_from(frames).unwrap_or_default() * usize::from(self.channels);
        self.auxiliary.drain(..samples.min(self.auxiliary.len()));
        self.auxiliary_consumed = self.auxiliary_consumed.saturating_add(frames);
    }

    /// One second; a consumer further behind has a problem the buffer cannot fix.
    fn max_buffered_frames(&self) -> u64 {
        u64::from(self.sample_rate)
    }

    fn bound_auxiliary(&mut self) {
        let buffered = u64::from(self.buffered_frames());
        let limit = self.max_buffered_frames();
        if buffered > limit {
            self.drop_frames(buffered - limit);
            self.aligned = false;
        }
    }

    fn begin_fade(&mut self) {
        let frames = (self.sample_rate / 200).max(2);
        self.fade_frames_remaining = frames;
        self.fade_frames_total = frames;
    }

    fn add_microphone(
        &mut self,
        master: &mut Pcm16Chunk,
        master_offset: u32,
        frames: u32,
        denominator: i64,
    ) {
        let channels = usize::from(self.channels);
        let numerator = i64::from(self.gain_percent);
        let fade_total = i64::from(self.fade_frames_total.max(1));
        for frame in 0..frames {
            let faded = if self.fade_frames_remaining > 0 {
                let done = self
                    .fade_frames_total
                    .saturating_sub(self.fade_frames_remaining);
                self.fade_frames_remaining -= 1;
                i64::from(done).min(fade_total)
            } else {
                fade_total
            };
            for channel in 0..channels {
                let source = usize::try_from(frame).unwrap_or_default() * channels + channel;
                let target = (usize::try_from(master_offset + frame).unwrap_or_default()
                    * channels
                    + channel)
                    * 2;
                let microphone = i64::from(self.auxiliary[source]);
                let contribution = rounded_ratio(
                    rounded_ratio(microphone, numerator, denominator),
                    faded,
                    fade_total,
                );
                let base = i16::from_le_bytes([master.data[target], master.data[target + 1]]);
                let mixed = i64::from(base).saturating_add(contribution);
                master.data[target..target + 2].copy_from_slice(&clamp_to_i16(mixed).to_le_bytes());
            }
        }
    }

    #[cfg(test)]
    fn buffered_frame_count(&self) -> u32 {
        self.buffered_frames()
    }
}

/// Rounds rather than truncates: truncation is signal-correlated, so every
/// attenuation in the chain added its own layer of harmonic distortion.
fn rounded_ratio(value: i64, numerator: i64, denominator: i64) -> i64 {
    if denominator == 0 {
        return value;
    }
    let product = value.saturating_mul(numerator);
    let half = denominator / 2;
    if product >= 0 {
        (product + half) / denominator
    } else {
        (product - half) / denominator
    }
}

/// Band-limited polyphase kernel. Linear interpolation leaves everything above
/// the target Nyquist in the signal, where it folds back as a gritty edge.
struct SincResampler {
    taps: Box<[f32]>,
    taps_per_phase: usize,
    half_taps: u64,
}

impl SincResampler {
    /// 512 phases keep a full-scale 20 kHz tone's timing error near -52 dBFS.
    const PHASES: usize = 512;
    /// In *output* frames, before widening for a downsampling cutoff.
    const BASE_HALF_TAPS: usize = 24;
    const MAX_HALF_TAPS: usize = 128;

    fn new(source_rate: u32, target_rate: u32) -> Self {
        // Downsampling band-limits to the target Nyquist; upsampling reconstructs.
        let cutoff = if target_rate < source_rate {
            f64::from(target_rate) / f64::from(source_rate)
        } else {
            1.0
        };
        // A lower cutoff needs a proportionally longer kernel or the transition
        // band grows until unwanted content sits inside it.
        let half_taps = ((Self::BASE_HALF_TAPS as f64 / cutoff).ceil() as usize)
            .clamp(Self::BASE_HALF_TAPS, Self::MAX_HALF_TAPS);
        let taps_per_phase = half_taps * 2;
        let half_width = half_taps as f64;
        let mut taps = Vec::with_capacity(Self::PHASES * taps_per_phase);
        for phase in 0..Self::PHASES {
            let fraction = phase as f64 / Self::PHASES as f64;
            let mut row = Vec::with_capacity(taps_per_phase);
            let mut sum = 0.0_f64;
            for tap in 0..taps_per_phase {
                let offset = tap as f64 - (half_width - 1.0) - fraction;
                let value = cutoff * sinc(cutoff * offset) * blackman(offset / half_width);
                sum += value;
                row.push(value);
            }
            // Unity DC gain per phase keeps a periodic level ripple out.
            let normalizer = if sum.abs() > f64::EPSILON { sum } else { 1.0 };
            taps.extend(row.into_iter().map(|value| (value / normalizer) as f32));
        }
        Self {
            taps: taps.into_boxed_slice(),
            taps_per_phase,
            half_taps: half_taps as u64,
        }
    }

    /// Source frames the kernel reads ahead of the interpolated position.
    fn lookahead_frames(&self) -> u64 {
        self.half_taps
    }

    /// Source frames the kernel reads behind the interpolated position.
    fn history_frames(&self) -> u64 {
        self.half_taps.saturating_sub(1)
    }

    fn phase(&self, fraction: u64, target_rate: u32) -> usize {
        let phases = Self::PHASES as u64;
        usize::try_from(fraction.saturating_mul(phases) / u64::from(target_rate.max(1)))
            .unwrap_or_default()
            .min(Self::PHASES - 1)
    }

    fn interpolate(&self, phase: usize, first_frame: i64, sample: impl Fn(i64) -> i16) -> i16 {
        let base = phase * self.taps_per_phase;
        let start = first_frame - self.history_frames() as i64;
        let mut accumulator = 0.0_f32;
        for tap in 0..self.taps_per_phase {
            accumulator += self.taps[base + tap] * f32::from(sample(start + tap as i64));
        }
        clamp_to_i16(accumulator.round() as i64)
    }
}

fn sinc(value: f64) -> f64 {
    if value.abs() < 1e-9 {
        1.0
    } else {
        let scaled = std::f64::consts::PI * value;
        scaled.sin() / scaled
    }
}

/// Blackman window over the normalised range [-1, 1].
fn blackman(normalized: f64) -> f64 {
    if normalized.abs() >= 1.0 {
        return 0.0;
    }
    let angle = std::f64::consts::PI * normalized;
    0.42 + 0.5 * angle.cos() + 0.08 * (2.0 * angle).cos()
}

/// Carries the fractional source position and the surrounding frames across
/// WASAPI packet boundaries, so no sample is repeated or skipped per callback.
/// Voice mode selects one stable input instead of averaging array channels.
pub struct PcmStreamConverter {
    source_rate: u32,
    source_channels: u16,
    target_rate: u32,
    target_channels: u16,
    voice: bool,
    voice_channel: VoiceChannelSelector,
    resampler: Option<SincResampler>,
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
            voice_channel: VoiceChannelSelector::default(),
            resampler: (source_rate != target_rate)
                .then(|| SincResampler::new(source_rate, target_rate)),
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

        let selected_voice_channel = if self.voice {
            let changed =
                self.voice_channel
                    .observe(&chunk.data, chunk.frames, self.source_channels);
            if changed {
                self.begin_restart_fade();
            }
            Some(self.voice_channel.selected())
        } else {
            None
        };
        let mapped = map_channels(
            &chunk.data,
            chunk.frames,
            self.source_channels,
            self.target_channels,
            selected_voice_channel,
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
        let mut emitted = self.output_frames_emitted;
        {
            let resampler = self
                .resampler
                .as_ref()
                .ok_or_else(|| AudioError("resampler is unavailable".into()))?;
            let lookahead = resampler.lookahead_frames();
            loop {
                let source_position = emitted.saturating_mul(u64::from(self.source_rate));
                let first_frame = source_position / u64::from(self.target_rate);
                // The kernel is centred, so a frame waits for its trailing half.
                if first_frame.saturating_add(lookahead) >= self.source_frames_received {
                    break;
                }
                let phase = resampler.phase(
                    source_position % u64::from(self.target_rate),
                    self.target_rate,
                );
                let first_frame = i64::try_from(first_frame)
                    .map_err(|_| AudioError("resampler source position overflow".into()))?;
                for channel in 0..self.target_channels {
                    let sample = resampler.interpolate(phase, first_frame, |frame| {
                        self.buffered_sample(frame, channel)
                    });
                    output.extend_from_slice(&sample.to_le_bytes());
                }
                emitted = emitted.saturating_add(1);
            }
        }
        self.output_frames_emitted = emitted;
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
            self.begin_restart_fade();
        } else {
            self.restart_fade_frames_remaining = 0;
            self.restart_fade_frames_total = 0;
        }
    }

    fn begin_restart_fade(&mut self) {
        let frames = (self.target_rate / 200).max(2);
        self.restart_fade_frames_remaining = frames;
        self.restart_fade_frames_total = frames;
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

    /// Clamped to the stream edges: the kernel reaches past both ends at the
    /// start of an epoch, where extending the edge sample is inaudible.
    fn buffered_sample(&self, frame: i64, channel: u16) -> i16 {
        let last_frame = self.source_frames_received.saturating_sub(1);
        let first_frame = self.source_start_frame.min(last_frame);
        let clamped = u64::try_from(frame.max(0))
            .unwrap_or_default()
            .clamp(first_frame, last_frame);
        let Some(relative) = clamped.checked_sub(self.source_start_frame) else {
            return 0;
        };
        relative
            .checked_mul(u64::from(self.target_channels))
            .and_then(|index| index.checked_add(u64::from(channel)))
            .and_then(|index| usize::try_from(index).ok())
            .and_then(|index| self.source.get(index).copied())
            .unwrap_or_default()
    }

    fn discard_consumed_source(&mut self) {
        let history = self
            .resampler
            .as_ref()
            .map_or(0, SincResampler::history_frames);
        let next_position = self
            .output_frames_emitted
            .saturating_mul(u64::from(self.source_rate));
        // Keep the frames the next output frame's kernel still reaches back to.
        let keep_from = (next_position / u64::from(self.target_rate)).saturating_sub(history);
        while self.source_start_frame < keep_from {
            for _ in 0..self.target_channels {
                let _ = self.source.pop_front();
            }
            self.source_start_frame += 1;
        }
    }
}

#[derive(Default)]
struct VoiceChannelSelector {
    selected: u16,
    candidate: Option<u16>,
    candidate_packets: u8,
    locked: bool,
}

impl VoiceChannelSelector {
    const SIGNAL_LEVEL: u64 = 512;
    const SWITCH_CONFIRMATION_PACKETS: u8 = 3;

    fn selected(&self) -> u16 {
        self.selected
    }

    fn observe(&mut self, data: &[u8], frames: u32, channels: u16) -> bool {
        if channels <= 1 || frames == 0 {
            self.selected = 0;
            self.locked = true;
            return false;
        }
        if self.selected >= channels {
            self.selected = 0;
            self.locked = false;
        }

        let metrics = (0..channels)
            .map(|channel| channel_metrics(data, frames, channels, channel))
            .collect::<Vec<_>>();
        let current = metrics[usize::from(self.selected)];
        if !self.locked && current.mean_absolute >= Self::SIGNAL_LEVEL {
            self.locked = true;
            self.clear_candidate();
            return false;
        }

        let best_alternative = (0..channels)
            .filter(|channel| *channel != self.selected)
            .max_by_key(|channel| metrics[usize::from(*channel)].mean_absolute);
        let candidate = best_alternative.filter(|channel| {
            let alternative = metrics[usize::from(*channel)];
            let current_is_clipped =
                current.clipped_samples.saturating_mul(10) >= u64::from(frames);
            let clean_clipping_fallback = current_is_clipped
                && alternative.clipped_samples == 0
                && alternative.mean_absolute >= Self::SIGNAL_LEVEL;
            let inactive_default_fallback = !self.locked
                && alternative.mean_absolute >= Self::SIGNAL_LEVEL
                && alternative.mean_absolute >= current.mean_absolute.saturating_mul(3);
            clean_clipping_fallback || inactive_default_fallback
        });

        let Some(candidate) = candidate else {
            self.clear_candidate();
            return false;
        };
        if self.candidate == Some(candidate) {
            self.candidate_packets = self.candidate_packets.saturating_add(1);
        } else {
            self.candidate = Some(candidate);
            self.candidate_packets = 1;
        }
        if self.candidate_packets < Self::SWITCH_CONFIRMATION_PACKETS {
            return false;
        }

        self.selected = candidate;
        self.locked = true;
        self.clear_candidate();
        wreath_core::diagnostic!(
            "Wreath microphone: selected input channel {} from {} native channels",
            self.selected + 1,
            channels
        );
        true
    }

    fn clear_candidate(&mut self) {
        self.candidate = None;
        self.candidate_packets = 0;
    }
}

#[derive(Clone, Copy)]
struct ChannelMetrics {
    mean_absolute: u64,
    clipped_samples: u64,
}

fn channel_metrics(data: &[u8], frames: u32, channels: u16, channel: u16) -> ChannelMetrics {
    let mut absolute_sum = 0_u64;
    let mut clipped_samples = 0_u64;
    for frame in 0..frames {
        let sample = read_sample(data, frame, channels, channel);
        let absolute = u64::from(sample.unsigned_abs());
        absolute_sum = absolute_sum.saturating_add(absolute);
        if absolute >= 32_000 {
            clipped_samples = clipped_samples.saturating_add(1);
        }
    }
    ChannelMetrics {
        mean_absolute: absolute_sum / u64::from(frames.max(1)),
        clipped_samples,
    }
}

fn map_channels(
    data: &[u8],
    frames: u32,
    source_channels: u16,
    target_channels: u16,
    voice_channel: Option<u16>,
) -> Vec<i16> {
    let mut mapped = Vec::with_capacity(
        usize::try_from(frames).unwrap_or_default() * usize::from(target_channels),
    );
    for frame in 0..frames {
        if let Some(voice_channel) = voice_channel {
            let voice = read_sample(data, frame, source_channels, voice_channel);
            mapped.extend(std::iter::repeat_n(voice, usize::from(target_channels)));
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

pub fn apply_gain_pcm16(
    chunk: &mut Pcm16Chunk,
    channels: u16,
    gain_percent: u16,
) -> Result<(), AudioError> {
    validate_pcm16(chunk, channels)?;
    if gain_percent > 200 {
        return Err(AudioError("audio gain exceeds 200 percent".into()));
    }
    let effective_gain = i64::from(gain_percent);
    for sample in chunk.data.chunks_exact_mut(2) {
        let value = i16::from_le_bytes([sample[0], sample[1]]);
        let adjusted = rounded_ratio(i64::from(value), effective_gain, 100);
        sample.copy_from_slice(&clamp_to_i16(adjusted).to_le_bytes());
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

fn scale_pcm16(chunk: &mut Pcm16Chunk, numerator: i64, denominator: i64) {
    if numerator == denominator || denominator == 0 {
        return;
    }
    for sample in chunk.data.chunks_exact_mut(2) {
        let value = i16::from_le_bytes([sample[0], sample[1]]);
        let scaled = rounded_ratio(i64::from(value), numerator, denominator);
        sample.copy_from_slice(&clamp_to_i16(scaled).to_le_bytes());
    }
}

fn clamp_to_i16(value: i64) -> i16 {
    value.clamp(i64::from(i16::MIN), i64::from(i16::MAX)) as i16
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

    fn tone(frames: usize, frequency: f64, sample_rate: f64) -> Vec<i16> {
        (0..frames)
            .map(|frame| {
                let phase = 2.0 * std::f64::consts::PI * frequency * frame as f64 / sample_rate;
                (phase.sin() * 12_000.0) as i16
            })
            .collect()
    }

    fn root_mean_square(samples: &[i16]) -> f64 {
        if samples.is_empty() {
            return 0.0;
        }
        let total = samples
            .iter()
            .map(|sample| f64::from(*sample) * f64::from(*sample))
            .sum::<f64>();
        (total / samples.len() as f64).sqrt()
    }

    /// Linear interpolation folded 30 kHz back to 18 kHz at nearly full level.
    #[test]
    fn downsampling_removes_content_above_the_target_nyquist() {
        let mut converter = PcmStreamConverter::new_voice(96_000, 1, 48_000, 1).unwrap();
        let input = tone(9_600, 30_000.0, 96_000.0);

        let converted = converter.push(chunk(0, 1, &input)).unwrap().unwrap();
        let output = samples(&converted);
        let settled = &output[output.len() / 4..output.len() * 3 / 4];

        assert!(
            root_mean_square(settled) < root_mean_square(&input) / 20.0,
            "aliased energy survived: {} vs {}",
            root_mean_square(settled),
            root_mean_square(&input)
        );
    }

    #[test]
    fn upsampling_preserves_an_in_band_tone() {
        let mut converter = PcmStreamConverter::new_voice(24_000, 1, 48_000, 1).unwrap();
        let input = tone(4_800, 1_000.0, 24_000.0);

        let converted = converter.push(chunk(0, 1, &input)).unwrap().unwrap();
        let output = samples(&converted);
        let settled = &output[output.len() / 4..output.len() * 3 / 4];
        let ratio = root_mean_square(settled) / root_mean_square(&input);

        assert!((0.95..=1.05).contains(&ratio), "level drifted by {ratio}");
    }

    #[test]
    fn streaming_resampler_is_continuous_across_packet_edges() {
        let input = tone(2_400, 1_000.0, 24_000.0);
        let mut whole = PcmStreamConverter::new_voice(24_000, 1, 48_000, 1).unwrap();
        let mut split = PcmStreamConverter::new_voice(24_000, 1, 48_000, 1).unwrap();

        let single = whole.push(chunk(0, 1, &input)).unwrap().unwrap();
        let first = split.push(chunk(0, 1, &input[..1_200])).unwrap().unwrap();
        let second = split.push(chunk(50, 1, &input[1_200..])).unwrap().unwrap();

        let mut joined = samples(&first);
        joined.extend(samples(&second));
        assert_eq!(joined, samples(&single));
        assert_eq!(first.timestamp, Duration::ZERO);
        assert_eq!(
            second.timestamp,
            Duration::from_nanos(u64::from(first.frames) * 1_000_000_000 / 48_000)
        );
    }

    #[test]
    fn voice_converter_does_not_mix_an_unrelated_second_input() {
        let mut converter = PcmStreamConverter::new_voice(48_000, 2, 48_000, 2).unwrap();
        let converted = converter
            .push(chunk(0, 2, &[1_000, -1_000, 3_000, 1_000]))
            .unwrap()
            .unwrap();

        assert_eq!(samples(&converted), [1_000, 1_000, 3_000, 3_000]);
    }

    #[test]
    fn voice_converter_selects_an_active_non_default_input_stably() {
        let mut converter = PcmStreamConverter::new_voice(48_000, 2, 48_000, 1).unwrap();
        for packet in 0..2 {
            let converted = converter
                .push(chunk(packet, 2, &[20, 4_000, -20, -4_000]))
                .unwrap()
                .unwrap();
            assert_eq!(samples(&converted), [20, -20]);
        }

        let switched = converter
            .push(chunk(2, 2, &[20, 4_000, -20, -4_000]))
            .unwrap()
            .unwrap();
        let switched_samples = samples(&switched);
        assert_eq!(switched_samples[0], 0);
        assert!(switched_samples[1] < 0);

        let remains_selected = converter
            .push(chunk(3, 2, &[8_000, 1_000, -8_000, -1_000]))
            .unwrap()
            .unwrap();
        assert!(samples(&remains_selected)[0] > 0);
    }

    #[test]
    fn voice_converter_keeps_a_working_first_input() {
        let mut converter = PcmStreamConverter::new_voice(48_000, 2, 48_000, 1).unwrap();
        for packet in 0..4 {
            let converted = converter
                .push(chunk(packet, 2, &[2_000, 12_000, -2_000, -12_000]))
                .unwrap()
                .unwrap();
            assert_eq!(samples(&converted), [2_000, -2_000]);
        }
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
        let input = tone(1_200, 1_000.0, 24_000.0);
        let _ = converter.push(chunk(0, 1, &input)).unwrap();
        let restarted = converter.push(chunk(1_000, 1, &input)).unwrap().unwrap();

        assert_eq!(restarted.timestamp, Duration::from_secs(1));
        assert!(restarted.discontinuous);
        assert_eq!(samples(&restarted)[0], 0);
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
        let mixed = samples(&mixed);

        // Every frame carries the same headroom; the microphone only reaches its own.
        assert_eq!(mixed[0], 333);
        assert_eq!(mixed[1], 3_333);
        assert!(mixed[2] > 3_333, "the microphone reached the third frame");
    }

    /// Headroom taken only from covered frames jumped the desktop level at gaps.
    #[test]
    fn desktop_level_does_not_step_at_a_microphone_gap() {
        let mut mixer = PcmMixer::new(1_000, 1, 100).unwrap();
        mixer
            .push_auxiliary(chunk(2, 1, &[0, 0]), 1_000, 1)
            .unwrap();

        let mixed = mixer.mix(chunk(0, 1, &[10_000; 4])).unwrap();
        assert_eq!(samples(&mixed), [5_000; 4]);
    }

    #[test]
    fn microphone_buffer_stays_bounded() {
        let mut mixer = PcmMixer::new(1_000, 1, 100).unwrap();
        // Three seconds of microphone audio with nothing consuming it.
        for packet in 0..30 {
            mixer
                .push_auxiliary(chunk(packet * 100, 1, &[0; 100]), 1_000, 1)
                .unwrap();
        }

        assert_eq!(mixer.buffered_frame_count(), 1_000);
    }

    /// Re-deriving the alignment from two truncated durations per packet repeated
    /// or skipped a sample several times a second.
    #[test]
    fn a_drifting_master_clock_never_repeats_or_skips_a_microphone_sample() {
        let mut mixer = PcmMixer::new(48_000, 1, 100).unwrap();
        let mut heard = Vec::new();
        for packet in 0..12_u64 {
            let ramp = (0..480)
                .map(|frame| ((packet as i64 * 480 + frame) * 4) as i16)
                .collect::<Vec<_>>();
            mixer
                .push_auxiliary(
                    Pcm16Chunk {
                        timestamp: Duration::from_nanos(packet * 10_000_000),
                        frames: 480,
                        discontinuous: false,
                        data: ramp
                            .iter()
                            .flat_map(|sample| sample.to_le_bytes())
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                    },
                    48_000,
                    1,
                )
                .unwrap();
            // The master clock creeps forward a fraction of a sample per packet.
            let mixed = mixer
                .mix(Pcm16Chunk {
                    // 40 us of slip per packet crosses a frame boundary often.
                    timestamp: Duration::from_nanos(packet * 10_000_000 + packet * 40_000),
                    frames: 480,
                    discontinuous: false,
                    data: vec![0; 480 * 2].into_boxed_slice(),
                })
                .unwrap();
            heard.extend(samples(&mixed));
        }

        // Past the first alignment every step must be the ramp's own step.
        let settled = &heard[480..];
        let steps = settled
            .windows(2)
            .filter(|pair| pair[1] != pair[0] + 2)
            .count();
        assert_eq!(
            steps,
            0,
            "microphone stream was disturbed {steps} times: {:?}",
            &settled[..32]
        );
    }

    #[test]
    fn standalone_stream_gain_can_attenuate_or_boost() {
        let mut audio = chunk(0, 1, &[20_000, -20_000]);
        apply_gain_pcm16(&mut audio, 1, 200).unwrap();
        assert_eq!(samples(&audio), [32_767, -32_768]);
        apply_gain_pcm16(&mut audio, 1, 50).unwrap();
        assert_eq!(samples(&audio), [16_384, -16_384]);
        assert!(apply_gain_pcm16(&mut audio, 1, 201).is_err());
    }
}
