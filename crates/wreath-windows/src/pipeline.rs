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
#[cfg(target_os = "windows")]
const TARGET_PROBE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

#[cfg(any(target_os = "windows", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TargetVerdict {
    Keep,
    Rebuild,
    WaitForStableSize,
}

#[cfg(any(target_os = "windows", test))]
fn target_verdict(
    switched: bool,
    resized: bool,
    window_target: bool,
    size_already_seen: bool,
) -> TargetVerdict {
    if switched {
        return TargetVerdict::Rebuild;
    }
    if !resized {
        return TargetVerdict::Keep;
    }
    if window_target && !size_already_seen {
        return TargetVerdict::WaitForStableSize;
    }
    TargetVerdict::Rebuild
}
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
    pub source: Option<String>,
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
            source: None,
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
struct StageContext<'a> {
    runtime: &'a crate::video::VideoRuntime,
    config: &'a wreath_core::config::Config,
    codec: crate::video::HardwareCodec,
}

#[cfg(target_os = "windows")]
#[derive(Debug, Clone, PartialEq, Eq)]
enum TargetIdentity {
    Monitor(String),
    Window(isize),
}

#[cfg(target_os = "windows")]
struct DesiredTarget {
    source: crate::capture::CaptureSource,
    identity: TargetIdentity,
    probe_size: (u32, u32),
    game: Option<String>,
}

#[cfg(target_os = "windows")]
struct VideoStage {
    capture: crate::capture::SourceCapture,
    info: crate::capture::CaptureInfo,
    frames: crossbeam_channel::Receiver<crate::capture::CapturedFrame>,
    converter: crate::conversion::GpuColorConverter,
    encoder: crate::encoder::HardwareVideoEncoder,
    available_surfaces: Vec<crate::conversion::Nv12Surface>,
    in_flight_surfaces: std::collections::VecDeque<crate::conversion::Nv12Surface>,
    input_requests: usize,
    identity: TargetIdentity,
    probe_size: (u32, u32),
    game: Option<String>,
    bitrate_kbps: u32,
}

#[cfg(target_os = "windows")]
impl VideoStage {
    fn start(
        context: &StageContext<'_>,
        target: DesiredTarget,
    ) -> Result<Self, crate::video::VideoError> {
        use crate::capture::SourceCapture;
        use crate::conversion::GpuColorConverter;
        use crate::encoder::{EncoderSettings, HardwareVideoEncoder};

        let StageContext {
            runtime,
            config,
            codec,
        } = *context;
        let (capture, info, frames) = SourceCapture::start(
            runtime.device(),
            &target.source,
            config.capture.frames_per_second,
            config.capture.cursor,
        )?;
        let encoded_width = info.width & !1;
        let encoded_height = info.height & !1;
        let settings = EncoderSettings {
            width: encoded_width,
            height: encoded_height,
            frames_per_second: config.capture.frames_per_second,
            bitrate_kbps: target_bitrate_kbps(config, encoded_width, encoded_height),
        }
        .validate()?;
        let converter = GpuColorConverter::initialize(
            runtime.device(),
            runtime.context(),
            (info.width, info.height),
            (settings.width, settings.height),
            settings.frames_per_second,
        )?;
        let mut available_surfaces = Vec::with_capacity(ENCODER_SURFACE_COUNT);
        for _ in 0..ENCODER_SURFACE_COUNT {
            available_surfaces.push(converter.create_output_surface()?);
        }
        let encoder = HardwareVideoEncoder::initialize(runtime.device(), codec, settings)?;
        Ok(Self {
            capture,
            info,
            frames,
            converter,
            encoder,
            available_surfaces,
            in_flight_surfaces: std::collections::VecDeque::with_capacity(ENCODER_SURFACE_COUNT),
            input_requests: 0,
            identity: target.identity,
            probe_size: target.probe_size,
            game: target.game,
            bitrate_kbps: settings.bitrate_kbps,
        })
    }

    fn rebuild(
        self,
        context: &StageContext<'_>,
        target: DesiredTarget,
    ) -> Result<Self, crate::video::VideoError> {
        drop(self);
        Self::start(context, target)
    }

    fn restart_capture(
        &mut self,
        context: &StageContext<'_>,
        target: &DesiredTarget,
    ) -> Result<bool, crate::video::VideoError> {
        let StageContext {
            runtime, config, ..
        } = *context;
        if target.identity != self.identity {
            return Ok(false);
        }
        let (capture, info, frames) = crate::capture::SourceCapture::start(
            runtime.device(),
            &target.source,
            config.capture.frames_per_second,
            config.capture.cursor,
        )?;
        if info.width != self.info.width || info.height != self.info.height {
            return Ok(false);
        }
        self.capture = capture;
        self.frames = frames;
        self.probe_size = target.probe_size;
        Ok(true)
    }

    fn describe(&self) -> String {
        match (&self.game, &self.identity) {
            (Some(game), TargetIdentity::Window(_)) => format!("the {game} window"),
            (Some(game), TargetIdentity::Monitor(monitor)) => format!("{game} on {monitor}"),
            (None, _) => self.info.monitor.clone(),
        }
    }
}

#[cfg(target_os = "windows")]
fn resolve_target(
    config: &wreath_core::config::Config,
    watch: &mut crate::game::GameWatch,
) -> Result<DesiredTarget, crate::video::VideoError> {
    use crate::capture::CaptureSource;
    use crate::game::WindowUsability;

    if config.capture.follow_game
        && let Some(game) = watch.look()
        && let monitor = crate::game::window_monitor(game.window)
        && let Some((monitor_name, monitor_width, monitor_height)) =
            crate::display::monitor_details(monitor)
    {
        let label = crate::game::display_name(&game.title, &game.executable);
        return Ok(
            match crate::game::window_usability(&game.facts, game.confidence) {
                WindowUsability::Capturable => DesiredTarget {
                    identity: TargetIdentity::Window(game.window.0 as isize),
                    probe_size: (game.facts.width, game.facts.height),
                    source: CaptureSource::Window {
                        handle: game.window,
                        title: label.clone(),
                        monitor: monitor_name,
                    },
                    game: Some(label),
                },
                WindowUsability::CoversMonitor | WindowUsability::TooSmall => DesiredTarget {
                    identity: TargetIdentity::Monitor(monitor_name.clone()),
                    probe_size: (monitor_width, monitor_height),
                    source: CaptureSource::Monitor {
                        handle: monitor,
                        name: monitor_name,
                    },
                    game: Some(label),
                },
            },
        );
    }
    let display = crate::display::select_display(config.capture.monitor.as_deref())?;
    Ok(DesiredTarget {
        identity: TargetIdentity::Monitor(display.target.name.clone()),
        probe_size: (display.target.width, display.target.height),
        source: CaptureSource::Monitor {
            handle: display.handle,
            name: display.target.name,
        },
        game: None,
    })
}

#[cfg(target_os = "windows")]
fn new_replay_buffer(
    config: &wreath_core::config::Config,
    video_bitrate_kbps: u32,
    audio: Option<&PipelineAudio>,
) -> Result<wreath_core::replay_buffer::EncodedReplayBuffer, crate::video::VideoError> {
    let audio_bitrate_kbps = audio.map_or(0, |audio| audio.bytes_per_second() / 125);
    let memory_budget = replay_memory_budget(estimated_buffer_bytes(
        video_bitrate_kbps.saturating_add(audio_bitrate_kbps),
        config.capture.duration_seconds,
    ))?;
    wreath_core::replay_buffer::EncodedReplayBuffer::new(
        std::time::Duration::from_secs(u64::from(config.capture.duration_seconds)),
        memory_budget,
    )
    .ok_or_else(|| crate::video::VideoError::Initialization("invalid replay buffer limits".into()))
}

#[cfg(target_os = "windows")]
fn rebuild_stage(
    context: &StageContext<'_>,
    stage: VideoStage,
    target: DesiredTarget,
    audio: &mut Option<PipelineAudio>,
    buffer: &mut wreath_core::replay_buffer::EncodedReplayBuffer,
    status: &std::sync::Arc<std::sync::RwLock<PipelineStatus>>,
    health: &mut CaptureHealth,
) -> Result<VideoStage, crate::video::VideoError> {
    let stage = stage.rebuild(context, target)?;
    *buffer = new_replay_buffer(context.config, stage.bitrate_kbps, audio.as_ref())?;
    if let Some(audio) = audio.as_mut() {
        audio.discard_queued();
        audio
            .start_new_epoch()
            .map_err(|error| crate::video::VideoError::Initialization(error.to_string()))?;
    }
    *health = CaptureHealth::started(std::time::Instant::now());
    update_status(status, |pipeline| {
        pipeline.monitor = Some(stage.info.monitor.clone());
        pipeline.source = Some(stage.describe());
        pipeline.buffered_seconds = 0;
        pipeline.encoded_bytes = 0;
    });
    wreath_core::diagnostic!(
        "Wreath capture: the pipeline now records {} at {}x{}",
        stage.describe(),
        stage.info.width,
        stage.info.height
    );
    Ok(stage)
}

#[cfg(target_os = "windows")]
fn run_pipeline(
    config: wreath_core::config::Config,
    commands: crossbeam_channel::Receiver<PipelineCommand>,
    status: &std::sync::Arc<std::sync::RwLock<PipelineStatus>>,
    ready: std::sync::mpsc::SyncSender<Result<(), crate::video::VideoError>>,
) -> Result<(), crate::video::VideoError> {
    use crate::video::{VideoError, VideoRuntime};

    let mut watch = crate::game::GameWatch::new(&config.capture.games);
    let initialized = (|| -> Result<_, VideoError> {
        let runtime = VideoRuntime::initialize()?;
        let codec = runtime.select_encoder(config.capture.codec)?;
        let target = resolve_target(&config, &mut watch)?;
        let stage = VideoStage::start(
            &StageContext {
                runtime: &runtime,
                config: &config,
                codec,
            },
            target,
        )?;
        let audio = PipelineAudio::initialize(&config.audio)
            .map_err(|error| VideoError::Initialization(error.to_string()))?;
        let buffer = new_replay_buffer(&config, stage.bitrate_kbps, audio.as_ref())?;
        Ok((runtime, codec, stage, audio, buffer))
    })();

    let (runtime, codec, mut stage, mut audio, mut buffer) = match initialized {
        Ok(initialized) => initialized,
        Err(error) => {
            let _ = ready.send(Err(error.clone()));
            return Err(error);
        }
    };

    update_status(status, |pipeline| {
        pipeline.state = PipelineRunState::Recording;
        pipeline.monitor = Some(stage.info.monitor.clone());
        pipeline.source = Some(stage.describe());
        pipeline.codec = Some(stage.encoder.codec());
        pipeline.adapter = Some(runtime.adapter().clone());
        pipeline.error = None;
    });
    if ready.send(Ok(())).is_err() {
        return Ok(());
    }

    let context = StageContext {
        runtime: &runtime,
        config: &config,
        codec,
    };
    let mut recording = true;
    let mut skipped_frames = 0_u64;
    let mut mismatched_frames = 0_u64;
    let mut last_report = std::time::Instant::now();
    let mut last_probe = std::time::Instant::now();
    let mut pending_size = None;
    let mut health = CaptureHealth::started(std::time::Instant::now());
    loop {
        drain_encoder_events(&mut stage, &mut buffer, status, &mut health)?;
        if recording {
            let now = std::time::Instant::now();
            if now.saturating_duration_since(last_probe) >= TARGET_PROBE_INTERVAL {
                last_probe = now;
                let target = resolve_target(&config, &mut watch)?;
                let switched = target.identity != stage.identity;
                let resized = target.probe_size != stage.probe_size;
                match target_verdict(
                    switched,
                    resized,
                    matches!(target.identity, TargetIdentity::Window(_)),
                    pending_size == Some(target.probe_size),
                ) {
                    TargetVerdict::Keep => pending_size = None,
                    TargetVerdict::WaitForStableSize => pending_size = Some(target.probe_size),
                    TargetVerdict::Rebuild => {
                        if switched {
                            wreath_core::diagnostic!(
                                "Wreath capture: recording moves from {} to {}",
                                stage.describe(),
                                target.game.as_deref().unwrap_or(target.source.label())
                            );
                        } else {
                            wreath_core::diagnostic!(
                                "Wreath capture: {} changed to {}x{}, rebuilding the capture pipeline",
                                stage.describe(),
                                target.probe_size.0,
                                target.probe_size.1
                            );
                        }
                        pending_size = None;
                        stage = rebuild_stage(
                            &context,
                            stage,
                            target,
                            &mut audio,
                            &mut buffer,
                            status,
                            &mut health,
                        )?;
                        continue;
                    }
                }
            }
            match health.check(context.runtime)? {
                CaptureAction::Continue => {}
                CaptureAction::RestartCapture => {
                    let target = resolve_target(&config, &mut watch)?;
                    if stage.restart_capture(&context, &target)? {
                        health.note_capture_restart(std::time::Instant::now());
                    } else {
                        wreath_core::diagnostic!(
                            "Wreath capture: the capture surface no longer matches {} at {}x{}, rebuilding the pipeline",
                            stage.describe(),
                            stage.info.width,
                            stage.info.height
                        );
                        stage = rebuild_stage(
                            &context,
                            stage,
                            target,
                            &mut audio,
                            &mut buffer,
                            status,
                            &mut health,
                        )?;
                    }
                    continue;
                }
            }
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
        let frames = stage.frames.clone();
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
                    if let Err(error) = stage.encoder.flush() {
                        let _ = command.reply.send(Err(error.to_string()));
                        continue;
                    }
                    stage
                        .available_surfaces
                        .extend(stage.in_flight_surfaces.drain(..));
                    stage.input_requests = 0;
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
                    last_probe = std::time::Instant::now();
                    pending_size = None;
                    update_status(status, |pipeline| {
                        pipeline.state = PipelineRunState::Recording
                    });
                    let _ = command.reply.send(Ok(PipelineCommandResult::Ok));
                }
                PipelineCommandKind::Save => {
                    spawn_save(
                        config.storage.directory.clone(),
                        &stage.encoder,
                        audio.as_ref(),
                        buffer.snapshot(),
                        command.reply,
                    );
                }
                PipelineCommandKind::Stop => {
                    let result = stage
                        .encoder
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
            if frame.width != stage.info.width || frame.height != stage.info.height {
                mismatched_frames = mismatched_frames.saturating_add(1);
                if mismatched_frames.is_power_of_two() {
                    wreath_core::diagnostic!(
                        "Wreath capture: {mismatched_frames} frames arrived as {}x{} while the pipeline encodes {}x{}, waiting for the display mode to settle",
                        frame.width,
                        frame.height,
                        stage.info.width,
                        stage.info.height
                    );
                }
                continue;
            }
            mismatched_frames = 0;
            health.note_frame(std::time::Instant::now());
            drain_encoder_events(&mut stage, &mut buffer, status, &mut health)?;
            if stage.input_requests == 0 || stage.available_surfaces.is_empty() {
                skipped_frames = skipped_frames.saturating_add(1);
                if skipped_frames.is_power_of_two() {
                    wreath_core::diagnostic!(
                        "Wreath video pipeline: {skipped_frames} frames skipped because the encoder was not ready"
                    );
                }
                continue;
            }
            let surface = stage
                .available_surfaces
                .pop()
                .expect("surface availability checked");
            stage.converter.convert(&frame.texture, &surface)?;
            stage
                .encoder
                .submit_texture(&surface.texture, frame.timestamp)?;
            stage.in_flight_surfaces.push_back(surface);
            stage.input_requests -= 1;
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
    ) -> Result<CaptureAction, crate::video::VideoError> {
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
                Ok(CaptureAction::Continue)
            }
            CaptureVerdict::EncoderStalled => Err(VideoError::Initialization(format!(
                "the hardware encoder accepted frames but returned none for {} seconds",
                ENCODER_STALL_TIMEOUT.as_secs()
            ))),
            CaptureVerdict::RestartCapture => Ok(CaptureAction::RestartCapture),
        }
    }

    fn note_capture_restart(&mut self, now: std::time::Instant) {
        self.capture_started = now;
        self.silent_restarts = self.silent_restarts.saturating_add(1);
        if self.silent_restarts.is_power_of_two() {
            wreath_core::diagnostic!(
                "Wreath capture: no frame for {} seconds, capture session restarted ({} restarts without a frame so far)",
                now.saturating_duration_since(self.last_frame).as_secs(),
                self.silent_restarts
            );
        }
    }
}

#[cfg(target_os = "windows")]
enum CaptureAction {
    Continue,
    RestartCapture,
}

#[cfg(target_os = "windows")]
fn drain_encoder_events(
    stage: &mut VideoStage,
    buffer: &mut wreath_core::replay_buffer::EncodedReplayBuffer,
    status: &std::sync::Arc<std::sync::RwLock<PipelineStatus>>,
    health: &mut CaptureHealth,
) -> Result<(), crate::video::VideoError> {
    use crate::encoder::EncoderEvent;

    while let Some(event) = stage.encoder.try_next_event()? {
        match event {
            EncoderEvent::NeedInput => {
                stage.input_requests = stage.input_requests.saturating_add(1)
            }
            EncoderEvent::HaveOutput => {
                if let Some(packet) = stage.encoder.take_packet()? {
                    health.note_packet(std::time::Instant::now());
                    buffer.push(packet);
                    if let Some(surface) = stage.in_flight_surfaces.pop_front() {
                        stage.available_surfaces.push(surface);
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
    fn a_new_target_is_rebuilt_at_once() {
        assert_eq!(
            target_verdict(true, false, true, false),
            TargetVerdict::Rebuild
        );
        assert_eq!(
            target_verdict(true, true, true, false),
            TargetVerdict::Rebuild
        );
    }

    #[test]
    fn an_unchanged_target_is_left_alone() {
        assert_eq!(
            target_verdict(false, false, false, false),
            TargetVerdict::Keep
        );
        assert_eq!(
            target_verdict(false, false, true, true),
            TargetVerdict::Keep
        );
    }

    #[test]
    fn a_display_mode_change_does_not_wait() {
        assert_eq!(
            target_verdict(false, true, false, false),
            TargetVerdict::Rebuild
        );
    }

    #[test]
    fn a_window_has_to_hold_its_new_size_for_one_probe() {
        assert_eq!(
            target_verdict(false, true, true, false),
            TargetVerdict::WaitForStableSize
        );
        assert_eq!(
            target_verdict(false, true, true, true),
            TargetVerdict::Rebuild
        );
    }

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
