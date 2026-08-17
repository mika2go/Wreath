use crate::video::{GraphicsAdapterInfo, HardwareCodec};

pub const MAX_REPLAY_MEMORY_BYTES: u64 = 512 * 1_048_576;
const MIN_REPLAY_MEMORY_BYTES: u64 = 8 * 1_048_576;
#[cfg(target_os = "windows")]
const PIPELINE_COMMAND_CAPACITY: usize = 4;
#[cfg(target_os = "windows")]
const SAVE_COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
#[cfg(target_os = "windows")]
const CONTROL_COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
#[cfg(target_os = "windows")]
const ENCODER_SURFACE_COUNT: usize = 6;
#[cfg(target_os = "windows")]
const HEALTH_CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);
#[cfg(any(target_os = "windows", test))]
const CAPTURE_STALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
#[cfg(any(target_os = "windows", test))]
const ENCODER_STALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

#[cfg(any(target_os = "windows", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaptureVerdict {
    Healthy,
    EncoderStalled,
    RestartCapture,
}

#[cfg(any(target_os = "windows", test))]
fn capture_verdict(
    since_frame: std::time::Duration,
    since_packet: std::time::Duration,
    silent_for: std::time::Duration,
) -> CaptureVerdict {
    if since_frame >= CAPTURE_STALL_TIMEOUT {
        if silent_for >= CAPTURE_STALL_TIMEOUT {
            return CaptureVerdict::RestartCapture;
        }
        return CaptureVerdict::Healthy;
    }
    if since_packet >= ENCODER_STALL_TIMEOUT {
        return CaptureVerdict::EncoderStalled;
    }
    CaptureVerdict::Healthy
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineRunState {
    Starting,
    Recording,
    Paused,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineStatus {
    pub state: PipelineRunState,
    pub monitor: Option<String>,
    pub codec: Option<HardwareCodec>,
    pub adapter: Option<GraphicsAdapterInfo>,
    pub buffered_seconds: u16,
    pub encoded_bytes: usize,
    pub error: Option<String>,
}

impl Default for PipelineStatus {
    fn default() -> Self {
        Self {
            state: PipelineRunState::Starting,
            monitor: None,
            codec: None,
            adapter: None,
            buffered_seconds: 0,
            encoded_bytes: 0,
            error: None,
        }
    }
}

pub fn replay_memory_budget(estimated_bytes: u64) -> Result<usize, crate::video::VideoError> {
    if estimated_bytes > MAX_REPLAY_MEMORY_BYTES {
        return Err(crate::video::VideoError::Initialization(format!(
            "configured replay needs about {} MB, exceeding the {} MB Windows memory limit; reduce duration, frame rate, resolution, or quality",
            estimated_bytes.div_ceil(1_048_576),
            MAX_REPLAY_MEMORY_BYTES / 1_048_576
        )));
    }
    let generous = estimated_bytes.saturating_mul(5) / 2;
    let budget = generous.clamp(MIN_REPLAY_MEMORY_BYTES, MAX_REPLAY_MEMORY_BYTES);
    usize::try_from(budget).map_err(|_| {
        crate::video::VideoError::Initialization(
            "configured replay memory does not fit this Windows process".into(),
        )
    })
}

#[cfg(target_os = "windows")]
pub struct ReplayPipeline {
    commands: crossbeam_channel::Sender<PipelineCommand>,
    status: std::sync::Arc<std::sync::RwLock<PipelineStatus>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

#[cfg(target_os = "windows")]
impl ReplayPipeline {
    pub fn spawn(config: wreath_core::config::Config) -> Result<Self, crate::video::VideoError> {
        use std::sync::{Arc, RwLock, mpsc};

        let (command_sender, command_receiver) =
            crossbeam_channel::bounded(PIPELINE_COMMAND_CAPACITY);
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let status = Arc::new(RwLock::new(PipelineStatus::default()));
        let worker_status = Arc::clone(&status);
        let thread = std::thread::Builder::new()
            .name("wreath-video".into())
            .spawn(move || {
                let result = run_pipeline(config, command_receiver, &worker_status, ready_sender);
                if let Err(error) = result {
                    set_pipeline_error(&worker_status, error.to_string());
                }
            })
            .map_err(|error| crate::video::VideoError::Initialization(error.to_string()))?;

        match ready_receiver.recv() {
            Ok(Ok(())) => Ok(Self {
                commands: command_sender,
                status,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(error) => {
                let _ = thread.join();
                Err(crate::video::VideoError::Initialization(error.to_string()))
            }
        }
    }

    pub fn status(&self) -> PipelineStatus {
        self.status
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn pause(&self) -> Result<(), crate::video::VideoError> {
        self.send_command_with_timeout(PipelineCommandKind::Pause, CONTROL_COMMAND_TIMEOUT)
            .map(|_| ())
    }

    pub fn resume(&self) -> Result<(), crate::video::VideoError> {
        self.send_command_with_timeout(PipelineCommandKind::Resume, CONTROL_COMMAND_TIMEOUT)
            .map(|_| ())
    }

    pub fn save(&self) -> Result<std::path::PathBuf, crate::video::VideoError> {
        self.saver().save()
    }

    pub fn saver(&self) -> PipelineSaver {
        PipelineSaver {
            commands: self.commands.clone(),
        }
    }

    fn send_command_with_timeout(
        &self,
        kind: PipelineCommandKind,
        timeout: std::time::Duration,
    ) -> Result<PipelineCommandResult, crate::video::VideoError> {
        send_pipeline_command(&self.commands, kind, timeout)
    }
}

#[cfg(target_os = "windows")]
#[derive(Clone)]
pub struct PipelineSaver {
    commands: crossbeam_channel::Sender<PipelineCommand>,
}

#[cfg(target_os = "windows")]
impl PipelineSaver {
    pub fn save(&self) -> Result<std::path::PathBuf, crate::video::VideoError> {
        match send_pipeline_command(
            &self.commands,
            PipelineCommandKind::Save,
            SAVE_COMMAND_TIMEOUT,
        )? {
            PipelineCommandResult::Saved(path) => Ok(path),
            PipelineCommandResult::Ok => Err(crate::video::VideoError::Initialization(
                "video pipeline returned no saved path".into(),
            )),
        }
    }
}

#[cfg(target_os = "windows")]
fn send_pipeline_command(
    commands: &crossbeam_channel::Sender<PipelineCommand>,
    kind: PipelineCommandKind,
    timeout: std::time::Duration,
) -> Result<PipelineCommandResult, crate::video::VideoError> {
    use crossbeam_channel::{RecvTimeoutError, SendTimeoutError};

    let (reply_sender, reply_receiver) = crossbeam_channel::bounded(1);
    commands
        .send_timeout(
            PipelineCommand {
                kind,
                reply: reply_sender,
            },
            timeout,
        )
        .map_err(|error| match error {
            SendTimeoutError::Timeout(_) => crate::video::VideoError::Initialization(format!(
                "video pipeline did not accept the request within {} seconds",
                timeout.as_secs()
            )),
            SendTimeoutError::Disconnected(_) => {
                crate::video::VideoError::Initialization("video pipeline stopped".into())
            }
        })?;
    reply_receiver
        .recv_timeout(timeout)
        .map_err(|error| match error {
            RecvTimeoutError::Timeout => crate::video::VideoError::Initialization(format!(
                "video pipeline did not answer within {} seconds",
                timeout.as_secs()
            )),
            RecvTimeoutError::Disconnected => {
                crate::video::VideoError::Initialization("video pipeline stopped".into())
            }
        })?
        .map_err(crate::video::VideoError::Initialization)
}

#[cfg(target_os = "windows")]
impl Drop for ReplayPipeline {
    fn drop(&mut self) {
        let (reply_sender, reply_receiver) = crossbeam_channel::bounded(1);
        let _ = self.commands.send_timeout(
            PipelineCommand {
                kind: PipelineCommandKind::Stop,
                reply: reply_sender,
            },
            CONTROL_COMMAND_TIMEOUT,
        );
        if reply_receiver
            .recv_timeout(CONTROL_COMMAND_TIMEOUT)
            .is_err()
        {
            wreath_core::diagnostic!("Wreath capture: the video pipeline did not stop in time");
            return;
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(target_os = "windows")]
struct PipelineCommand {
    kind: PipelineCommandKind,
    reply: crossbeam_channel::Sender<Result<PipelineCommandResult, String>>,
}

#[cfg(target_os = "windows")]
enum PipelineCommandKind {
    Pause,
    Resume,
    Save,
    Stop,
}

#[cfg(target_os = "windows")]
enum PipelineCommandResult {
    Ok,
    Saved(std::path::PathBuf),
}

#[cfg(target_os = "windows")]
enum AudioCaptureSource {
    Desktop(crate::audio::LoopbackCapture),
    Microphone(crate::audio::MicrophoneCapture),
}

#[cfg(target_os = "windows")]
impl AudioCaptureSource {
    fn format(&self) -> crate::audio::AudioFormat {
        match self {
            Self::Desktop(capture) => capture.format(),
            Self::Microphone(capture) => capture.format(),
        }
    }

    fn receiver(&self) -> &crossbeam_channel::Receiver<crate::audio::PcmChunk> {
        match self {
            Self::Desktop(capture) => capture.receiver(),
            Self::Microphone(capture) => capture.receiver(),
        }
    }
}

#[cfg(target_os = "windows")]
struct PipelineAudio {
    master: AudioCaptureSource,
    auxiliary_microphone: Option<crate::audio::MicrophoneCapture>,
    mixer: Option<crate::audio_mixer::PcmMixer>,
    master_converter: Option<crate::audio_mixer::PcmStreamConverter>,
    output_sample_rate: u32,
    output_channels: u16,
    master_gain_percent: u16,
    pending_master: std::collections::VecDeque<crate::audio::Pcm16Chunk>,
    microphone_watermark: Option<std::time::Duration>,
    microphone_clipped: u64,
    encoder: crate::audio_encoder::AacEncoder,
}

#[cfg(target_os = "windows")]
impl PipelineAudio {
    fn initialize(
        config: &wreath_core::config::AudioConfig,
    ) -> Result<Option<Self>, crate::audio::AudioError> {
        let (master, auxiliary_microphone) = if config.desktop {
            let desktop = crate::audio::LoopbackCapture::spawn(config.desktop_device.as_deref())?;
            let microphone = config
                .microphone
                .then(|| {
                    crate::audio::MicrophoneCapture::spawn(config.microphone_device.as_deref())
                })
                .transpose()?;
            (AudioCaptureSource::Desktop(desktop), microphone)
        } else if config.microphone {
            (
                AudioCaptureSource::Microphone(crate::audio::MicrophoneCapture::spawn(
                    config.microphone_device.as_deref(),
                )?),
                None,
            )
        } else {
            return Ok(None);
        };
        let format = master.format();
        let (output_sample_rate, output_channels, master_converter) =
            if matches!(&master, AudioCaptureSource::Microphone(_)) {
                let output_sample_rate =
                    crate::audio::preferred_microphone_sample_rate(format.sample_rate);
                (
                    output_sample_rate,
                    1,
                    Some(crate::audio_mixer::PcmStreamConverter::new_voice(
                        format.sample_rate,
                        format.channels,
                        output_sample_rate,
                        1,
                    )?),
                )
            } else {
                (format.sample_rate, format.channels, None)
            };
        let settings = crate::audio_encoder::AudioEncoderSettings::for_capture(
            output_sample_rate,
            output_channels,
        )?;
        let mixer = auxiliary_microphone
            .as_ref()
            .map(|_| {
                crate::audio_mixer::PcmMixer::new_with_gains(
                    output_sample_rate,
                    output_channels,
                    config.desktop_gain_percent,
                    config.microphone_gain_percent,
                )
            })
            .transpose()?;
        let encoder = crate::audio_encoder::AacEncoder::initialize(settings)?;
        wreath_core::diagnostic!(
            "Wreath audio pipeline: master {} Hz/{} ch -> encoder {} Hz/{} ch at {} B/s, microphone mixer {}, desktop gain {}%, microphone gain {}%",
            format.sample_rate,
            format.channels,
            output_sample_rate,
            output_channels,
            settings.bytes_per_second,
            if mixer.is_some() { "on" } else { "off" },
            config.desktop_gain_percent,
            config.microphone_gain_percent
        );
        let mixed = mixer.is_some();
        Ok(Some(Self {
            master,
            auxiliary_microphone,
            mixer,
            master_converter,
            output_sample_rate,
            output_channels,
            master_gain_percent: if config.desktop {
                if mixed {
                    100
                } else {
                    config.desktop_gain_percent
                }
            } else {
                config.microphone_gain_percent.min(100)
            },
            pending_master: std::collections::VecDeque::with_capacity(12),
            microphone_watermark: None,
            microphone_clipped: 0,
            encoder,
        }))
    }

    fn note_microphone_clipping(&mut self, data: &[u8]) {
        let clipped = clipped_samples(data);
        if clipped == 0 {
            return;
        }
        self.microphone_clipped = self.microphone_clipped.saturating_add(clipped);
        if self.microphone_clipped.is_power_of_two() {
            wreath_core::diagnostic!(
                "Wreath microphone: {} clipped samples so far; lower the Windows input level for this device",
                self.microphone_clipped
            );
        }
    }

    fn master_receiver(&self) -> &crossbeam_channel::Receiver<crate::audio::PcmChunk> {
        self.master.receiver()
    }

    fn microphone_receiver(&self) -> Option<&crossbeam_channel::Receiver<crate::audio::PcmChunk>> {
        self.auxiliary_microphone
            .as_ref()
            .map(crate::audio::MicrophoneCapture::receiver)
    }

    fn encode_master(
        &mut self,
        chunk: crate::audio::PcmChunk,
    ) -> Result<Vec<wreath_core::replay_buffer::EncodedPacket>, crate::audio::AudioError> {
        let format = self.master.format();
        let normalized = crate::audio::normalize_to_pcm16(format, chunk)?;
        if matches!(self.master, AudioCaptureSource::Microphone(_)) {
            self.note_microphone_clipping(&normalized.data);
        }
        let mut normalized = if let Some(converter) = &mut self.master_converter {
            let Some(converted) = converter.push(normalized)? else {
                return Ok(Vec::new());
            };
            converted
        } else {
            normalized
        };
        if self.master_gain_percent != 100 {
            crate::audio_mixer::apply_gain_pcm16(
                &mut normalized,
                self.output_channels,
                self.master_gain_percent,
            )?;
        }
        if self.mixer.is_none() {
            return self.encoder.encode(normalized);
        }
        self.pending_master.push_back(normalized);
        self.encode_synchronized_master()
    }

    fn push_microphone(
        &mut self,
        chunk: crate::audio::PcmChunk,
    ) -> Result<Vec<wreath_core::replay_buffer::EncodedPacket>, crate::audio::AudioError> {
        let capture = self.auxiliary_microphone.as_ref().ok_or_else(|| {
            crate::audio::AudioError("microphone packet arrived without a mixer".into())
        })?;
        let format = capture.format();
        let normalized = crate::audio::normalize_to_pcm16(format, chunk)?;
        self.note_microphone_clipping(&normalized.data);
        let microphone_end = pcm_chunk_end(&normalized, format.sample_rate);
        self.mixer
            .as_mut()
            .ok_or_else(|| crate::audio::AudioError("microphone mixer is unavailable".into()))?
            .push_auxiliary(normalized, format.sample_rate, format.channels)?;
        self.microphone_watermark = Some(
            self.microphone_watermark
                .map_or(microphone_end, |current| current.max(microphone_end)),
        );
        self.encode_synchronized_master()
    }

    fn encode_synchronized_master(
        &mut self,
    ) -> Result<Vec<wreath_core::replay_buffer::EncodedPacket>, crate::audio::AudioError> {
        const MAX_PENDING_MASTER_CHUNKS: usize = 12;
        let mut packets = Vec::new();
        while let Some(master) = self.pending_master.front() {
            let master_end = pcm_chunk_end(master, self.output_sample_rate);
            if !synchronized_master_ready(
                master_end,
                self.microphone_watermark,
                self.pending_master.len(),
                MAX_PENDING_MASTER_CHUNKS,
            ) {
                break;
            }
            let master = self
                .pending_master
                .pop_front()
                .expect("front master packet was checked");
            let mixed = self
                .mixer
                .as_mut()
                .ok_or_else(|| crate::audio::AudioError("microphone mixer is unavailable".into()))?
                .mix(master)?;
            packets.extend(self.encoder.encode(mixed)?);
        }
        Ok(packets)
    }

    fn output_media_type(
        &self,
    ) -> Result<windows::Win32::Media::MediaFoundation::IMFMediaType, crate::audio::AudioError>
    {
        self.encoder.output_media_type()
    }

    fn bytes_per_second(&self) -> u32 {
        self.encoder.settings().bytes_per_second
    }

    fn discard_queued(&mut self) {
        self.pending_master.clear();
        self.microphone_watermark = None;
        while self.master.receiver().try_recv().is_ok() {}
        if let Some(microphone) = &self.auxiliary_microphone {
            while microphone.receiver().try_recv().is_ok() {}
        }
    }

    fn start_new_epoch(&mut self) -> Result<(), crate::audio::AudioError> {
        self.discard_queued();
        self.encoder.flush()
    }
}

#[cfg(any(target_os = "windows", test))]
fn clipped_samples(data: &[u8]) -> u64 {
    data.chunks_exact(2)
        .filter(|sample| i16::from_le_bytes([sample[0], sample[1]]).unsigned_abs() >= 32_700)
        .count() as u64
}

#[cfg(target_os = "windows")]
fn pcm_chunk_end(chunk: &crate::audio::Pcm16Chunk, sample_rate: u32) -> std::time::Duration {
    chunk
        .timestamp
        .saturating_add(std::time::Duration::from_nanos(
            u64::from(chunk.frames).saturating_mul(1_000_000_000) / u64::from(sample_rate.max(1)),
        ))
}

#[cfg(any(target_os = "windows", test))]
fn synchronized_master_ready(
    master_end: std::time::Duration,
    microphone_watermark: Option<std::time::Duration>,
    pending_chunks: usize,
    maximum_pending_chunks: usize,
) -> bool {
    microphone_watermark.is_some_and(|watermark| watermark >= master_end)
        || pending_chunks > maximum_pending_chunks
}

#[cfg(target_os = "windows")]
fn run_pipeline(
    config: wreath_core::config::Config,
    commands: crossbeam_channel::Receiver<PipelineCommand>,
    status: &std::sync::Arc<std::sync::RwLock<PipelineStatus>>,
    ready: std::sync::mpsc::SyncSender<Result<(), crate::video::VideoError>>,
) -> Result<(), crate::video::VideoError> {
    use std::collections::VecDeque;
    use std::time::Duration;

    use crate::capture::MonitorCapture;
    use crate::conversion::GpuColorConverter;
    use crate::encoder::{EncoderSettings, HardwareVideoEncoder};
    use crate::video::{VideoError, VideoRuntime};

    let initialized = (|| -> Result<_, VideoError> {
        let runtime = VideoRuntime::initialize()?;
        let codec = runtime.select_encoder(config.capture.codec)?;
        let display = crate::display::select_display(config.capture.monitor.as_deref())?;
        let (capture, capture_info, frames) = MonitorCapture::start_primary(
            runtime.device(),
            display.handle,
            &display.target.name,
            config.capture.frames_per_second,
            config.capture.cursor,
        )?;
        let settings = EncoderSettings {
            width: capture_info.width,
            height: capture_info.height,
            frames_per_second: config.capture.frames_per_second,
            bitrate_kbps: target_bitrate_kbps(&config, capture_info.width, capture_info.height),
        }
        .validate()?;
        let converter = GpuColorConverter::initialize(
            runtime.device(),
            runtime.context(),
            settings.width,
            settings.height,
            settings.frames_per_second,
        )?;
        let mut available_surfaces = Vec::with_capacity(ENCODER_SURFACE_COUNT);
        for _ in 0..ENCODER_SURFACE_COUNT {
            available_surfaces.push(converter.create_output_surface()?);
        }
        let encoder = HardwareVideoEncoder::initialize(runtime.device(), codec, settings)?;
        let audio = PipelineAudio::initialize(&config.audio)
            .map_err(|error| VideoError::Initialization(error.to_string()))?;
        let audio_bitrate_kbps = audio
            .as_ref()
            .map_or(0, |audio| audio.bytes_per_second() / 125);
        let memory_budget = replay_memory_budget(estimated_buffer_bytes(
            settings.bitrate_kbps.saturating_add(audio_bitrate_kbps),
            config.capture.duration_seconds,
        ))?;
        let buffer = wreath_core::replay_buffer::EncodedReplayBuffer::new(
            Duration::from_secs(u64::from(config.capture.duration_seconds)),
            memory_budget,
        )
        .ok_or_else(|| VideoError::Initialization("invalid replay buffer limits".into()))?;
        Ok((
            runtime,
            capture,
            capture_info,
            frames,
            converter,
            available_surfaces,
            encoder,
            audio,
            buffer,
        ))
    })();

    let (
        runtime,
        mut capture,
        capture_info,
        mut frames,
        converter,
        mut available_surfaces,
        encoder,
        mut audio,
        mut buffer,
    ) = match initialized {
        Ok(initialized) => initialized,
        Err(error) => {
            let _ = ready.send(Err(error.clone()));
            return Err(error);
        }
    };

    update_status(status, |pipeline| {
        pipeline.state = PipelineRunState::Recording;
        pipeline.monitor = Some(capture_info.monitor.clone());
        pipeline.codec = Some(encoder.codec());
        pipeline.adapter = Some(runtime.adapter().clone());
        pipeline.error = None;
    });
    if ready.send(Ok(())).is_err() {
        return Ok(());
    }

    let mut input_requests = 0_usize;
    let mut in_flight_surfaces = VecDeque::with_capacity(ENCODER_SURFACE_COUNT);
    let mut recording = true;
    let mut skipped_frames = 0_u64;
    let mut last_report = std::time::Instant::now();
    let mut health = CaptureHealth::started(std::time::Instant::now());
    loop {
        drain_encoder_events(
            &encoder,
            &mut input_requests,
            &mut buffer,
            &mut in_flight_surfaces,
            &mut available_surfaces,
            status,
            &mut health,
        )?;
        if recording {
            health.check(&runtime, &config, &capture_info, &mut capture, &mut frames)?;
        }
        if last_report.elapsed() >= std::time::Duration::from_secs(30) {
            last_report = std::time::Instant::now();
            crate::memory::report(
                "recorder",
                &format!(
                    "{} MB of encoded replay over {} s",
                    buffer.payload_bytes() / 1_048_576,
                    buffer.duration().as_secs()
                ),
            );
        }
        let mut selector = crossbeam_channel::Select::new();
        let command_index = selector.recv(&commands);
        let frame_index = recording.then(|| selector.recv(&frames));
        let audio_index = (recording && audio.is_some()).then(|| {
            selector.recv(
                audio
                    .as_ref()
                    .expect("audio presence checked")
                    .master_receiver(),
            )
        });
        let microphone_index = (recording
            && audio
                .as_ref()
                .is_some_and(|audio| audio.microphone_receiver().is_some()))
        .then(|| {
            selector.recv(
                audio
                    .as_ref()
                    .and_then(PipelineAudio::microphone_receiver)
                    .expect("microphone presence checked"),
            )
        });
        let Ok(operation) = selector.select_timeout(HEALTH_CHECK_INTERVAL) else {
            continue;
        };

        if operation.index() == command_index {
            let command = operation
                .recv(&commands)
                .map_err(|error| VideoError::Initialization(error.to_string()))?;
            match command.kind {
                PipelineCommandKind::Pause => {
                    recording = false;
                    if let Some(audio) = &mut audio {
                        audio.discard_queued();
                    }
                    update_status(status, |pipeline| pipeline.state = PipelineRunState::Paused);
                    let _ = command.reply.send(Ok(PipelineCommandResult::Ok));
                }
                PipelineCommandKind::Resume => {
                    while frames.try_recv().is_ok() {}
                    if let Err(error) = encoder.flush() {
                        let _ = command.reply.send(Err(error.to_string()));
                        continue;
                    }
                    available_surfaces.extend(in_flight_surfaces.drain(..));
                    input_requests = 0;
                    if let Some(audio) = &mut audio
                        && let Err(error) = audio.start_new_epoch()
                    {
                        let _ = command.reply.send(Err(error.to_string()));
                        continue;
                    }
                    buffer.reset();
                    update_buffer_status(status, &buffer);
                    recording = true;
                    health = CaptureHealth::started(std::time::Instant::now());
                    update_status(status, |pipeline| {
                        pipeline.state = PipelineRunState::Recording
                    });
                    let _ = command.reply.send(Ok(PipelineCommandResult::Ok));
                }
                PipelineCommandKind::Save => {
                    spawn_save(
                        config.storage.directory.clone(),
                        &encoder,
                        audio.as_ref(),
                        buffer.snapshot(),
                        command.reply,
                    );
                }
                PipelineCommandKind::Stop => {
                    let result = encoder
                        .drain()
                        .map(|()| PipelineCommandResult::Ok)
                        .map_err(|error| error.to_string());
                    let _ = command.reply.send(result);
                    break;
                }
            }
        } else if Some(operation.index()) == microphone_index {
            let chunk = operation
                .recv(
                    audio
                        .as_ref()
                        .and_then(PipelineAudio::microphone_receiver)
                        .expect("microphone selector is only registered with a microphone"),
                )
                .map_err(|error| VideoError::Initialization(error.to_string()))?;
            for packet in audio
                .as_mut()
                .expect("microphone selector requires audio")
                .push_microphone(chunk)
                .map_err(|error| VideoError::Initialization(error.to_string()))?
            {
                buffer.push(packet);
            }
            update_buffer_status(status, &buffer);
        } else if Some(operation.index()) == audio_index {
            let chunk = operation
                .recv(
                    audio
                        .as_ref()
                        .expect("audio selector is only registered with audio")
                        .master_receiver(),
                )
                .map_err(|error| VideoError::Initialization(error.to_string()))?;
            for packet in audio
                .as_mut()
                .expect("audio selector requires audio")
                .encode_master(chunk)
                .map_err(|error| VideoError::Initialization(error.to_string()))?
            {
                buffer.push(packet);
            }
            update_buffer_status(status, &buffer);
        } else if Some(operation.index()) == frame_index {
            let frame = operation
                .recv(&frames)
                .map_err(|error| VideoError::Initialization(error.to_string()))?;
            health.note_frame(std::time::Instant::now());
            drain_encoder_events(
                &encoder,
                &mut input_requests,
                &mut buffer,
                &mut in_flight_surfaces,
                &mut available_surfaces,
                status,
                &mut health,
            )?;
            if input_requests == 0 || available_surfaces.is_empty() {
                skipped_frames = skipped_frames.saturating_add(1);
                if skipped_frames.is_power_of_two() {
                    wreath_core::diagnostic!(
                        "Wreath video pipeline: {skipped_frames} frames skipped because the encoder was not ready"
                    );
                }
                continue;
            }
            if frame.width != capture_info.width || frame.height != capture_info.height {
                return Err(VideoError::Initialization(
                    "display resolution changed; reload Wreath to recreate the GPU pipeline".into(),
                ));
            }
            let surface = available_surfaces
                .pop()
                .expect("surface availability checked");
            converter.convert(&frame.texture, &surface)?;
            encoder.submit_texture(&surface.texture, frame.timestamp)?;
            in_flight_surfaces.push_back(surface);
            input_requests -= 1;
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn save_replay(
    directory: &std::path::Path,
    video_media_type: &windows::Win32::Media::MediaFoundation::IMFMediaType,
    audio_media_type: Option<&windows::Win32::Media::MediaFoundation::IMFMediaType>,
    packets: &[wreath_core::replay_buffer::EncodedPacket],
) -> Result<std::path::PathBuf, crate::video::VideoError> {
    use std::time::{SystemTime, UNIX_EPOCH};

    std::fs::create_dir_all(directory)
        .map_err(|error| crate::video::VideoError::Initialization(error.to_string()))?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| crate::video::VideoError::Initialization(error.to_string()))?
        .as_millis();
    let final_path = crate::mux::unique_clip_path(directory, timestamp);
    let stem = final_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("wreath-clip");
    let temporary_path = final_path.with_file_name(format!("{stem}.partial.mp4"));
    if let Err(error) = crate::mux::write_mp4(
        &temporary_path,
        video_media_type,
        audio_media_type,
        packets.iter(),
    ) {
        let _ = std::fs::remove_file(&temporary_path);
        return Err(error);
    }
    std::fs::rename(&temporary_path, &final_path)
        .map_err(|error| crate::video::VideoError::Initialization(error.to_string()))?;
    Ok(final_path)
}

#[cfg(target_os = "windows")]
fn spawn_save(
    directory: std::path::PathBuf,
    encoder: &crate::encoder::HardwareVideoEncoder,
    audio: Option<&PipelineAudio>,
    packets: Vec<wreath_core::replay_buffer::EncodedPacket>,
    reply: crossbeam_channel::Sender<Result<PipelineCommandResult, String>>,
) {
    use crate::video::VideoError;

    let video_media_type = encoder
        .output_media_type()
        .and_then(|media_type| MarshaledMediaType::new(&media_type));
    let audio_media_type = audio
        .map(PipelineAudio::output_media_type)
        .transpose()
        .map_err(|error| VideoError::Initialization(error.to_string()))
        .and_then(|media_type| media_type.as_ref().map(MarshaledMediaType::new).transpose());
    let (video_media_type, audio_media_type) = match (video_media_type, audio_media_type) {
        (Ok(video), Ok(audio)) => (video, audio),
        (Err(error), _) | (_, Err(error)) => {
            let _ = reply.send(Err(error.to_string()));
            return;
        }
    };

    let failure_reply = reply.clone();
    let spawn = std::thread::Builder::new()
        .name("wreath-save".into())
        .spawn(move || {
            use windows::Win32::System::Com::{
                COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize,
            };

            let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
                .ok()
                .map_err(|error| VideoError::Initialization(error.to_string()));
            let result = match com {
                Ok(()) => {
                    let result = {
                        let video_media_type = video_media_type.unmarshal();
                        let audio_media_type = audio_media_type
                            .map(MarshaledMediaType::unmarshal)
                            .transpose();
                        match (video_media_type, audio_media_type) {
                            (Ok(video), Ok(audio)) => {
                                save_replay(&directory, &video, audio.as_ref(), &packets)
                            }
                            (Err(error), _) | (_, Err(error)) => Err(error),
                        }
                    };
                    unsafe { CoUninitialize() };
                    result
                }
                Err(error) => Err(error),
            }
            .map(PipelineCommandResult::Saved)
            .map_err(|error| error.to_string());
            let _ = reply.send(result);
        });
    if let Err(error) = spawn {
        let _ = failure_reply.send(Err(format!("cannot start replay save worker: {error}")));
    }
}

#[cfg(target_os = "windows")]
struct MarshaledMediaType(Option<windows::Win32::System::Com::IStream>);

#[cfg(target_os = "windows")]
unsafe impl Send for MarshaledMediaType {}

#[cfg(target_os = "windows")]
impl MarshaledMediaType {
    fn new(
        media_type: &windows::Win32::Media::MediaFoundation::IMFMediaType,
    ) -> Result<Self, crate::video::VideoError> {
        use windows::Win32::System::Com::Marshal::CoMarshalInterThreadInterfaceInStream;
        use windows::core::Interface;

        let stream = unsafe {
            CoMarshalInterThreadInterfaceInStream(
                &windows::Win32::Media::MediaFoundation::IMFMediaType::IID,
                media_type,
            )
        }
        .map_err(|error| crate::video::VideoError::Initialization(error.to_string()))?;
        Ok(Self(Some(stream)))
    }

    fn unmarshal(
        mut self,
    ) -> Result<windows::Win32::Media::MediaFoundation::IMFMediaType, crate::video::VideoError>
    {
        use windows::Win32::System::Com::StructuredStorage::CoGetInterfaceAndReleaseStream;

        let stream = self.0.take().ok_or_else(|| {
            crate::video::VideoError::Initialization(
                "media type marshal stream was already consumed".into(),
            )
        })?;
        let media_type = unsafe { CoGetInterfaceAndReleaseStream(&stream) };
        std::mem::forget(stream);
        media_type.map_err(|error| crate::video::VideoError::Initialization(error.to_string()))
    }
}

#[cfg(target_os = "windows")]
struct CaptureHealth {
    last_frame: std::time::Instant,
    last_packet: std::time::Instant,
    capture_started: std::time::Instant,
    silent_restarts: u64,
}

#[cfg(target_os = "windows")]
impl CaptureHealth {
    fn started(now: std::time::Instant) -> Self {
        Self {
            last_frame: now,
            last_packet: now,
            capture_started: now,
            silent_restarts: 0,
        }
    }

    fn note_frame(&mut self, now: std::time::Instant) {
        self.last_frame = now;
    }

    fn note_packet(&mut self, now: std::time::Instant) {
        self.last_packet = now;
    }

    fn verdict(&self, now: std::time::Instant) -> CaptureVerdict {
        capture_verdict(
            now.saturating_duration_since(self.last_frame),
            now.saturating_duration_since(self.last_packet),
            now.saturating_duration_since(self.last_frame.max(self.capture_started)),
        )
    }

    fn check(
        &mut self,
        runtime: &crate::video::VideoRuntime,
        config: &wreath_core::config::Config,
        capture_info: &crate::capture::CaptureInfo,
        capture: &mut crate::capture::MonitorCapture,
        frames: &mut crossbeam_channel::Receiver<crate::capture::CapturedFrame>,
    ) -> Result<(), crate::video::VideoError> {
        use crate::video::VideoError;

        let now = std::time::Instant::now();
        if let Err(error) = unsafe { runtime.device().GetDeviceRemovedReason() } {
            return Err(VideoError::Initialization(format!(
                "the graphics device was lost: {error}"
            )));
        }
        match self.verdict(now) {
            CaptureVerdict::Healthy => {
                if now.saturating_duration_since(self.last_frame) < CAPTURE_STALL_TIMEOUT {
                    self.silent_restarts = 0;
                }
                return Ok(());
            }
            CaptureVerdict::EncoderStalled => {
                return Err(VideoError::Initialization(format!(
                    "the hardware encoder accepted frames but returned none for {} seconds",
                    ENCODER_STALL_TIMEOUT.as_secs()
                )));
            }
            CaptureVerdict::RestartCapture => {}
        }

        let display = crate::display::select_display(config.capture.monitor.as_deref())?;
        let (replacement, replacement_info, replacement_frames) =
            crate::capture::MonitorCapture::start_primary(
                runtime.device(),
                display.handle,
                &display.target.name,
                config.capture.frames_per_second,
                config.capture.cursor,
            )?;
        if replacement_info.width != capture_info.width
            || replacement_info.height != capture_info.height
        {
            return Err(VideoError::Initialization(
                "display resolution changed; reload Wreath to recreate the GPU pipeline".into(),
            ));
        }
        *capture = replacement;
        *frames = replacement_frames;
        self.capture_started = now;
        self.silent_restarts = self.silent_restarts.saturating_add(1);
        if self.silent_restarts.is_power_of_two() {
            wreath_core::diagnostic!(
                "Wreath capture: no frame for {} seconds, capture session restarted ({} restarts without a frame so far)",
                now.saturating_duration_since(self.last_frame).as_secs(),
                self.silent_restarts
            );
        }
        Ok(())
    }
}

#[cfg(target_os = "windows")]
fn drain_encoder_events(
    encoder: &crate::encoder::HardwareVideoEncoder,
    input_requests: &mut usize,
    buffer: &mut wreath_core::replay_buffer::EncodedReplayBuffer,
    in_flight: &mut std::collections::VecDeque<crate::conversion::Nv12Surface>,
    available: &mut Vec<crate::conversion::Nv12Surface>,
    status: &std::sync::Arc<std::sync::RwLock<PipelineStatus>>,
    health: &mut CaptureHealth,
) -> Result<(), crate::video::VideoError> {
    use crate::encoder::EncoderEvent;

    while let Some(event) = encoder.try_next_event()? {
        match event {
            EncoderEvent::NeedInput => *input_requests = input_requests.saturating_add(1),
            EncoderEvent::HaveOutput => {
                if let Some(packet) = encoder.take_packet()? {
                    health.note_packet(std::time::Instant::now());
                    buffer.push(packet);
                    if let Some(surface) = in_flight.pop_front() {
                        available.push(surface);
                    }
                    update_buffer_status(status, buffer);
                }
            }
            EncoderEvent::DrainComplete | EncoderEvent::Other(_) => {}
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn update_buffer_status(
    status: &std::sync::Arc<std::sync::RwLock<PipelineStatus>>,
    buffer: &wreath_core::replay_buffer::EncodedReplayBuffer,
) {
    update_status(status, |pipeline| {
        pipeline.buffered_seconds = buffer.duration().as_secs().min(u64::from(u16::MAX)) as u16;
        pipeline.encoded_bytes = buffer.payload_bytes();
    });
}

#[cfg(target_os = "windows")]
fn update_status(
    status: &std::sync::Arc<std::sync::RwLock<PipelineStatus>>,
    update: impl FnOnce(&mut PipelineStatus),
) {
    let mut status = status
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    update(&mut status);
}

#[cfg(target_os = "windows")]
fn set_pipeline_error(status: &std::sync::Arc<std::sync::RwLock<PipelineStatus>>, error: String) {
    update_status(status, |pipeline| {
        pipeline.state = PipelineRunState::Error;
        pipeline.error = Some(error);
    });
}

#[cfg(any(target_os = "windows", test))]
fn target_bitrate_kbps(config: &wreath_core::config::Config, width: u32, height: u32) -> u32 {
    let monitor = wreath_core::display::Monitor {
        id: 0,
        name: "windows-primary".into(),
        description: "Primary display".into(),
        make: String::new(),
        model: String::new(),
        serial: String::new(),
        width,
        height,
        refresh_rate: f64::from(config.capture.frames_per_second),
        focused: true,
        disabled: false,
    };
    wreath_core::replay::ReplaySpec::from_config(config, &monitor).target_bitrate_kbps()
}

#[cfg(any(target_os = "windows", test))]
fn estimated_buffer_bytes(bitrate_kbps: u32, duration_seconds: u16) -> u64 {
    u64::from(bitrate_kbps)
        .saturating_mul(1_000)
        .saturating_mul(u64::from(duration_seconds))
        / 8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_memory_has_a_floor_and_accepts_its_limit() {
        assert_eq!(
            replay_memory_budget(1).unwrap(),
            MIN_REPLAY_MEMORY_BYTES as usize
        );
        assert_eq!(
            replay_memory_budget(MAX_REPLAY_MEMORY_BYTES).unwrap(),
            MAX_REPLAY_MEMORY_BYTES as usize
        );
    }

    #[test]
    fn replay_memory_rejects_a_silently_shortened_configuration() {
        let error = replay_memory_budget(MAX_REPLAY_MEMORY_BYTES + 1).unwrap_err();

        assert!(error.to_string().contains("exceeding the 512 MB"));
    }

    #[test]
    fn bitrate_and_buffer_estimates_stay_in_the_encoded_domain() {
        let config = wreath_core::config::Config::default();
        let bitrate = target_bitrate_kbps(&config, 1920, 1080);
        let bytes = estimated_buffer_bytes(bitrate, config.capture.duration_seconds);

        assert!(bitrate >= 2_500);
        assert!(bytes < 100 * 1_048_576);
    }

    #[test]
    fn the_memory_budget_leaves_room_above_the_nominal_average() {
        let nominal = estimated_buffer_bytes(20_000, 30);

        let budget = replay_memory_budget(nominal).unwrap() as u64;

        assert!(budget > nominal.saturating_add(nominal / 2));
        assert!(budget <= MAX_REPLAY_MEMORY_BYTES);
    }

    #[test]
    fn clipping_is_counted_only_at_the_top_of_the_range() {
        let samples = [0_i16, 12_000, -12_000, 32_700, -32_768, 32_699]
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect::<Vec<_>>();

        assert_eq!(clipped_samples(&samples), 2);
        assert_eq!(clipped_samples(&[]), 0);
    }

    #[test]
    fn a_capture_that_went_silent_is_restarted() {
        use std::time::Duration;

        let recent = Duration::from_secs(1);

        assert_eq!(
            capture_verdict(recent, recent, recent),
            CaptureVerdict::Healthy
        );
        assert_eq!(
            capture_verdict(
                CAPTURE_STALL_TIMEOUT,
                CAPTURE_STALL_TIMEOUT,
                CAPTURE_STALL_TIMEOUT
            ),
            CaptureVerdict::RestartCapture
        );
    }

    #[test]
    fn a_screen_that_stays_still_is_not_restarted_every_tick() {
        use std::time::Duration;

        let silent_all_day = Duration::from_secs(60 * 60 * 8);

        assert_eq!(
            capture_verdict(silent_all_day, silent_all_day, Duration::from_secs(5)),
            CaptureVerdict::Healthy
        );
    }

    #[test]
    fn frames_going_in_with_nothing_coming_out_is_a_wedged_encoder() {
        use std::time::Duration;

        assert_eq!(
            capture_verdict(
                Duration::from_millis(20),
                ENCODER_STALL_TIMEOUT,
                Duration::from_millis(20)
            ),
            CaptureVerdict::EncoderStalled
        );
    }

    #[test]
    fn an_idle_encoder_without_frames_is_not_blamed() {
        use std::time::Duration;

        let silent = Duration::from_secs(60 * 60);

        assert_eq!(
            capture_verdict(silent, silent, Duration::from_secs(1)),
            CaptureVerdict::Healthy
        );
    }

    #[test]
    fn desktop_audio_waits_for_the_microphone_without_stalling_forever() {
        use std::time::Duration;

        let master_end = Duration::from_millis(120);
        assert!(!synchronized_master_ready(
            master_end,
            Some(Duration::from_millis(110)),
            4,
            12,
        ));
        assert!(synchronized_master_ready(
            master_end,
            Some(Duration::from_millis(120)),
            4,
            12,
        ));
        assert!(synchronized_master_ready(master_end, None, 13, 12));
    }
}
