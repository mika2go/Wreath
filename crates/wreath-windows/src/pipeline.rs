use crate::video::HardwareCodec;

pub const MAX_REPLAY_MEMORY_BYTES: u64 = 512 * 1_048_576;
const MIN_REPLAY_MEMORY_BYTES: u64 = 8 * 1_048_576;
#[cfg(target_os = "windows")]
const ENCODER_SURFACE_COUNT: usize = 3;

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
            buffered_seconds: 0,
            encoded_bytes: 0,
            error: None,
        }
    }
}

pub fn replay_memory_budget(estimated_bytes: u64) -> usize {
    usize::try_from(estimated_bytes.clamp(MIN_REPLAY_MEMORY_BYTES, MAX_REPLAY_MEMORY_BYTES))
        .unwrap_or(usize::MAX)
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

        let (command_sender, command_receiver) = crossbeam_channel::unbounded();
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
        self.send_command(PipelineCommandKind::Pause).map(|_| ())
    }

    pub fn resume(&self) -> Result<(), crate::video::VideoError> {
        self.send_command(PipelineCommandKind::Resume).map(|_| ())
    }

    pub fn save(&self) -> Result<std::path::PathBuf, crate::video::VideoError> {
        match self.send_command(PipelineCommandKind::Save)? {
            PipelineCommandResult::Saved(path) => Ok(path),
            PipelineCommandResult::Ok => Err(crate::video::VideoError::Initialization(
                "video pipeline returned no saved path".into(),
            )),
        }
    }

    fn send_command(
        &self,
        kind: PipelineCommandKind,
    ) -> Result<PipelineCommandResult, crate::video::VideoError> {
        let (reply_sender, reply_receiver) = crossbeam_channel::bounded(1);
        self.commands
            .send(PipelineCommand {
                kind,
                reply: reply_sender,
            })
            .map_err(|error| crate::video::VideoError::Initialization(error.to_string()))?;
        reply_receiver
            .recv()
            .map_err(|error| crate::video::VideoError::Initialization(error.to_string()))?
            .map_err(crate::video::VideoError::Initialization)
    }
}

#[cfg(target_os = "windows")]
impl Drop for ReplayPipeline {
    fn drop(&mut self) {
        let (reply_sender, reply_receiver) = crossbeam_channel::bounded(1);
        let _ = self.commands.send(PipelineCommand {
            kind: PipelineCommandKind::Stop,
            reply: reply_sender,
        });
        let _ = reply_receiver.recv();
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
    master_gain_percent: u16,
    pending_master: Option<crate::audio::Pcm16Chunk>,
    encoder: crate::audio_encoder::AacEncoder,
}

#[cfg(target_os = "windows")]
impl PipelineAudio {
    fn initialize(
        config: &wreath_core::config::AudioConfig,
    ) -> Result<Option<Self>, crate::audio::AudioError> {
        let (master, auxiliary_microphone) = if config.desktop {
            let desktop = crate::audio::LoopbackCapture::spawn()?;
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
        let settings = crate::audio_encoder::AudioEncoderSettings::for_capture(
            format.sample_rate,
            format.channels,
        )?;
        let mixer = auxiliary_microphone
            .as_ref()
            .map(|_| {
                crate::audio_mixer::PcmMixer::new(
                    format.sample_rate,
                    format.channels,
                    config.microphone_gain_percent,
                )
            })
            .transpose()?;
        let encoder = crate::audio_encoder::AacEncoder::initialize(settings)?;
        Ok(Some(Self {
            master,
            auxiliary_microphone,
            mixer,
            master_gain_percent: if config.desktop {
                100
            } else {
                config.microphone_gain_percent
            },
            pending_master: None,
            encoder,
        }))
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
        let mut normalized = crate::audio::normalize_to_pcm16(format, chunk)?;
        if self.master_gain_percent != 100 {
            crate::audio_mixer::apply_gain_pcm16(
                &mut normalized,
                format.channels,
                self.master_gain_percent,
            )?;
        }
        let ready = if let Some(mixer) = &mut self.mixer {
            self.pending_master
                .replace(normalized)
                .map(|pending| mixer.mix(pending))
                .transpose()?
        } else {
            Some(normalized)
        };
        ready.map_or_else(|| Ok(Vec::new()), |chunk| self.encoder.encode(chunk))
    }

    fn push_microphone(
        &mut self,
        chunk: crate::audio::PcmChunk,
    ) -> Result<(), crate::audio::AudioError> {
        let capture = self.auxiliary_microphone.as_ref().ok_or_else(|| {
            crate::audio::AudioError("microphone packet arrived without a mixer".into())
        })?;
        let format = capture.format();
        let normalized = crate::audio::normalize_to_pcm16(format, chunk)?;
        self.mixer
            .as_mut()
            .ok_or_else(|| crate::audio::AudioError("microphone mixer is unavailable".into()))?
            .push_auxiliary(normalized, format.sample_rate, format.channels)
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
        self.pending_master = None;
        while self.master.receiver().try_recv().is_ok() {}
        if let Some(microphone) = &self.auxiliary_microphone {
            while microphone.receiver().try_recv().is_ok() {}
        }
    }
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
        ));
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
        _runtime,
        _capture,
        capture_info,
        frames,
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
        pipeline.error = None;
    });
    if ready.send(Ok(())).is_err() {
        return Ok(());
    }

    let mut input_requests = 0_usize;
    let mut in_flight_surfaces = VecDeque::with_capacity(ENCODER_SURFACE_COUNT);
    let mut recording = true;
    loop {
        drain_encoder_events(
            &encoder,
            &mut input_requests,
            &mut buffer,
            &mut in_flight_surfaces,
            &mut available_surfaces,
            status,
        )?;
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
        let operation = selector.select();

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
                    if let Some(audio) = &mut audio {
                        audio.discard_queued();
                    }
                    recording = true;
                    update_status(status, |pipeline| {
                        pipeline.state = PipelineRunState::Recording
                    });
                    let _ = command.reply.send(Ok(PipelineCommandResult::Ok));
                }
                PipelineCommandKind::Save => {
                    let result =
                        save_replay(&config.storage.directory, &encoder, audio.as_ref(), &buffer)
                            .map(PipelineCommandResult::Saved)
                            .map_err(|error| error.to_string());
                    let _ = command.reply.send(result);
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
            audio
                .as_mut()
                .expect("microphone selector requires audio")
                .push_microphone(chunk)
                .map_err(|error| VideoError::Initialization(error.to_string()))?;
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
            drain_encoder_events(
                &encoder,
                &mut input_requests,
                &mut buffer,
                &mut in_flight_surfaces,
                &mut available_surfaces,
                status,
            )?;
            if input_requests == 0 || available_surfaces.is_empty() {
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
    encoder: &crate::encoder::HardwareVideoEncoder,
    audio: Option<&PipelineAudio>,
    buffer: &wreath_core::replay_buffer::EncodedReplayBuffer,
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
    let video_media_type = encoder.output_media_type()?;
    let audio_media_type = audio
        .map(PipelineAudio::output_media_type)
        .transpose()
        .map_err(|error| crate::video::VideoError::Initialization(error.to_string()))?;
    if let Err(error) = crate::mux::write_mp4(
        &temporary_path,
        &video_media_type,
        audio_media_type.as_ref(),
        buffer.packets(),
    ) {
        let _ = std::fs::remove_file(&temporary_path);
        return Err(error);
    }
    std::fs::rename(&temporary_path, &final_path)
        .map_err(|error| crate::video::VideoError::Initialization(error.to_string()))?;
    Ok(final_path)
}

#[cfg(target_os = "windows")]
fn drain_encoder_events(
    encoder: &crate::encoder::HardwareVideoEncoder,
    input_requests: &mut usize,
    buffer: &mut wreath_core::replay_buffer::EncodedReplayBuffer,
    in_flight: &mut std::collections::VecDeque<crate::conversion::Nv12Surface>,
    available: &mut Vec<crate::conversion::Nv12Surface>,
    status: &std::sync::Arc<std::sync::RwLock<PipelineStatus>>,
) -> Result<(), crate::video::VideoError> {
    use crate::encoder::EncoderEvent;

    while let Some(event) = encoder.try_next_event()? {
        match event {
            EncoderEvent::NeedInput => *input_requests = input_requests.saturating_add(1),
            EncoderEvent::HaveOutput => {
                if let Some(packet) = encoder.take_packet()? {
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
    fn replay_memory_is_never_unbounded() {
        assert_eq!(replay_memory_budget(1), MIN_REPLAY_MEMORY_BYTES as usize);
        assert_eq!(
            replay_memory_budget(u64::MAX),
            MAX_REPLAY_MEMORY_BYTES as usize
        );
    }

    #[test]
    fn bitrate_and_buffer_estimates_stay_in_the_encoded_domain() {
        let config = wreath_core::config::Config::default();
        let bitrate = target_bitrate_kbps(&config, 1920, 1080);
        let bytes = estimated_buffer_bytes(bitrate, config.capture.duration_seconds);

        assert!(bitrate >= 2_500);
        assert!(bytes < 100 * 1_048_576);
    }
}
