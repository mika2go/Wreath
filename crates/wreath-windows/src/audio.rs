use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioFormat {
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub block_align: u16,
    pub floating_point: bool,
}

impl AudioFormat {
    pub fn bytes_for_frames(self, frames: u32) -> Option<usize> {
        usize::try_from(frames)
            .ok()?
            .checked_mul(usize::from(self.block_align))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PcmChunk {
    pub timestamp: std::time::Duration,
    pub frames: u32,
    pub discontinuous: bool,
    pub data: Box<[u8]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pcm16Chunk {
    pub timestamp: std::time::Duration,
    pub frames: u32,
    pub discontinuous: bool,
    pub data: Box<[u8]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioError(pub String);

impl fmt::Display for AudioError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Windows audio capture failed: {}", self.0)
    }
}

impl std::error::Error for AudioError {}

/// Converts one bounded WASAPI packet to packed, interleaved signed 16-bit PCM.
/// The AAC encoder only accepts this format, so unsupported layouts fail early
/// instead of being interpreted with the wrong sample width.
pub fn normalize_to_pcm16(format: AudioFormat, chunk: PcmChunk) -> Result<Pcm16Chunk, AudioError> {
    let source_sample_bytes = match (format.floating_point, format.bits_per_sample) {
        (true, 32) => 4,
        (false, 8) => 1,
        (false, 16) => 2,
        (false, 24) => 3,
        (false, 32) => 4,
        _ => {
            return Err(AudioError(format!(
                "unsupported WASAPI sample format: {}-bit {}",
                format.bits_per_sample,
                if format.floating_point {
                    "float"
                } else {
                    "integer"
                }
            )));
        }
    };
    if format.channels == 0 || format.sample_rate == 0 {
        return Err(AudioError("WASAPI returned an empty audio format".into()));
    }
    let packed_frame_bytes = usize::from(format.channels)
        .checked_mul(source_sample_bytes)
        .ok_or_else(|| AudioError("audio frame layout overflow".into()))?;
    if usize::from(format.block_align) < packed_frame_bytes {
        return Err(AudioError(
            "WASAPI block alignment is smaller than its channel layout".into(),
        ));
    }
    let expected_source_bytes = format
        .bytes_for_frames(chunk.frames)
        .ok_or_else(|| AudioError("audio packet size overflow".into()))?;
    if chunk.data.len() != expected_source_bytes {
        return Err(AudioError(format!(
            "WASAPI packet has {} bytes; expected {expected_source_bytes}",
            chunk.data.len()
        )));
    }

    let sample_count = usize::try_from(chunk.frames)
        .ok()
        .and_then(|frames| frames.checked_mul(usize::from(format.channels)))
        .ok_or_else(|| AudioError("audio sample count overflow".into()))?;
    let output_bytes = sample_count
        .checked_mul(2)
        .ok_or_else(|| AudioError("normalized audio packet size overflow".into()))?;
    let mut output = Vec::with_capacity(output_bytes);
    for frame in chunk.data.chunks_exact(usize::from(format.block_align)) {
        for channel in 0..usize::from(format.channels) {
            let offset = channel * source_sample_bytes;
            let sample = &frame[offset..offset + source_sample_bytes];
            let normalized = if format.floating_point {
                float_to_i16(f32::from_le_bytes(
                    sample.try_into().expect("four-byte float"),
                ))
            } else {
                integer_to_i16(sample)
            };
            output.extend_from_slice(&normalized.to_le_bytes());
        }
    }
    debug_assert_eq!(output.len(), output_bytes);
    Ok(Pcm16Chunk {
        timestamp: chunk.timestamp,
        frames: chunk.frames,
        discontinuous: chunk.discontinuous,
        data: output.into_boxed_slice(),
    })
}

fn float_to_i16(sample: f32) -> i16 {
    if !sample.is_finite() {
        0
    } else if sample <= -1.0 {
        i16::MIN
    } else if sample >= 1.0 {
        i16::MAX
    } else {
        (sample * f32::from(i16::MAX)).round() as i16
    }
}

fn integer_to_i16(sample: &[u8]) -> i16 {
    match sample {
        [value] => (i16::from(*value) - 128) << 8,
        [low, high] => i16::from_le_bytes([*low, *high]),
        [low, middle, high] => {
            let sign = if high & 0x80 == 0 { 0 } else { 0xff };
            (i32::from_le_bytes([*low, *middle, *high, sign]) >> 8) as i16
        }
        [byte0, byte1, byte2, byte3] => {
            (i32::from_le_bytes([*byte0, *byte1, *byte2, *byte3]) >> 16) as i16
        }
        _ => unreachable!("sample widths are validated before conversion"),
    }
}

/// Event-driven WASAPI loopback capture. Its queue is intentionally bounded;
/// lagging consumers lose old capture callbacks instead of growing memory.
#[cfg(target_os = "windows")]
pub struct LoopbackCapture {
    stream: CaptureStream,
}

/// Timer-driven WASAPI microphone capture. Windows' shared audio engine is
/// asked for a processed mono communications stream first; unusual drivers
/// transparently fall back to their native shared mix format.
#[cfg(target_os = "windows")]
pub struct MicrophoneCapture {
    stream: CaptureStream,
}

#[cfg(target_os = "windows")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MicrophoneTarget {
    pub id: String,
    pub name: String,
    pub default: bool,
}

/// Lists active WASAPI capture endpoints. This performs COM discovery only
/// for the duration of the call and does not start an audio stream.
#[cfg(target_os = "windows")]
pub fn microphones() -> Result<Vec<MicrophoneTarget>, AudioError> {
    use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
    use windows::Win32::Media::Audio::{
        DEVICE_STATE_ACTIVE, IMMDeviceEnumerator, MMDeviceEnumerator, eCapture, eCommunications,
    };
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
    };

    let uninitialize = match unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok() {
        Ok(()) => true,
        Err(error) if error.code() == RPC_E_CHANGED_MODE => false,
        Err(error) => return Err(AudioError(error.to_string())),
    };
    let result = (|| -> Result<Vec<MicrophoneTarget>, AudioError> {
        let enumerator: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
                .map_err(|error| AudioError(error.to_string()))?;
        let default_id = unsafe { enumerator.GetDefaultAudioEndpoint(eCapture, eCommunications) }
            .ok()
            .and_then(|device| device_id(&device).ok());
        let collection = unsafe { enumerator.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE) }
            .map_err(|error| AudioError(error.to_string()))?;
        let count =
            unsafe { collection.GetCount() }.map_err(|error| AudioError(error.to_string()))?;
        let mut targets = Vec::with_capacity(count as usize);
        for index in 0..count {
            let device =
                unsafe { collection.Item(index) }.map_err(|error| AudioError(error.to_string()))?;
            let id = device_id(&device)?;
            targets.push(MicrophoneTarget {
                default: default_id.as_deref() == Some(id.as_str()),
                name: device_name(&device).unwrap_or_else(|_| "Windows audio input".into()),
                id,
            });
        }
        Ok(targets)
    })();
    if uninitialize {
        unsafe { CoUninitialize() };
    }
    result
}

#[cfg(target_os = "windows")]
fn device_name(device: &windows::Win32::Media::Audio::IMMDevice) -> Result<String, AudioError> {
    use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
    use windows::Win32::System::Com::STGM_READ;
    use windows::Win32::System::Com::StructuredStorage::PropVariantToString;

    let store = unsafe { device.OpenPropertyStore(STGM_READ) }
        .map_err(|error| AudioError(error.to_string()))?;
    let value = unsafe { store.GetValue(&PKEY_Device_FriendlyName) }
        .map_err(|error| AudioError(error.to_string()))?;
    let mut buffer = [0_u16; 256];
    unsafe { PropVariantToString(&value, &mut buffer) }
        .map_err(|error| AudioError(error.to_string()))?;
    let length = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    Ok(String::from_utf16_lossy(&buffer[..length]))
}

#[cfg(target_os = "windows")]
fn device_id(device: &windows::Win32::Media::Audio::IMMDevice) -> Result<String, AudioError> {
    use windows::Win32::System::Com::CoTaskMemFree;

    let pointer = unsafe { device.GetId() }.map_err(|error| AudioError(error.to_string()))?;
    let id = unsafe { pointer.to_string() }.map_err(|error| AudioError(error.to_string()));
    unsafe { CoTaskMemFree(Some(pointer.0.cast())) };
    id
}

#[cfg(target_os = "windows")]
struct CaptureStream {
    format: AudioFormat,
    receiver: crossbeam_channel::Receiver<PcmChunk>,
    stop_event: windows::Win32::Foundation::HANDLE,
    thread: Option<std::thread::JoinHandle<()>>,
}

#[cfg(target_os = "windows")]
impl LoopbackCapture {
    pub fn spawn() -> Result<Self, AudioError> {
        CaptureStream::spawn(CaptureEndpoint::Loopback).map(|stream| Self { stream })
    }

    pub fn format(&self) -> AudioFormat {
        self.stream.format
    }

    pub fn receiver(&self) -> &crossbeam_channel::Receiver<PcmChunk> {
        &self.stream.receiver
    }
}

#[cfg(target_os = "windows")]
impl MicrophoneCapture {
    pub fn spawn(endpoint_id: Option<&str>) -> Result<Self, AudioError> {
        CaptureStream::spawn(CaptureEndpoint::Microphone {
            endpoint_id: endpoint_id.map(str::to_owned),
        })
        .map(|stream| Self { stream })
    }

    pub fn format(&self) -> AudioFormat {
        self.stream.format
    }

    pub fn receiver(&self) -> &crossbeam_channel::Receiver<PcmChunk> {
        &self.stream.receiver
    }
}

#[cfg(target_os = "windows")]
enum CaptureEndpoint {
    Loopback,
    Microphone { endpoint_id: Option<String> },
}

#[cfg(target_os = "windows")]
impl CaptureStream {
    fn spawn(endpoint: CaptureEndpoint) -> Result<Self, AudioError> {
        use std::sync::mpsc;

        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::CreateEventW;

        let stop_event = unsafe { CreateEventW(None, false, false, None) }
            .map_err(|error| AudioError(error.to_string()))?;
        let stop_for_thread = stop_event.0 as usize;
        // Keep several seconds of bounded headroom for encoder or GPU stalls.
        // The old, smaller queue could drop a run of microphone packets and
        // turn the next waveform edge into an audible click.
        let (chunk_sender, chunk_receiver) = crossbeam_channel::bounded(256);
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let thread_name = match endpoint {
            CaptureEndpoint::Loopback => "wreath-wasapi-loopback",
            CaptureEndpoint::Microphone { .. } => "wreath-wasapi-microphone",
        };
        let thread = match std::thread::Builder::new()
            .name(thread_name.into())
            .spawn(move || {
                let stop_for_thread =
                    windows::Win32::Foundation::HANDLE(stop_for_thread as *mut std::ffi::c_void);
                let result = capture_loop(stop_for_thread, endpoint, chunk_sender, &ready_sender);
                if let Err(error) = result {
                    let _ = ready_sender.send(Err(error));
                }
            }) {
            Ok(thread) => thread,
            Err(error) => {
                let _ = unsafe { CloseHandle(stop_event) };
                return Err(AudioError(error.to_string()));
            }
        };
        match ready_receiver.recv() {
            Ok(Ok(format)) => Ok(Self {
                format,
                receiver: chunk_receiver,
                stop_event,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                unsafe { CloseHandle(stop_event) }
                    .map_err(|close_error| AudioError(close_error.to_string()))?;
                Err(error)
            }
            Err(error) => {
                let _ = thread.join();
                let _ = unsafe { CloseHandle(stop_event) };
                Err(AudioError(error.to_string()))
            }
        }
    }
}

#[cfg(target_os = "windows")]
impl Drop for CaptureStream {
    fn drop(&mut self) {
        let _ = unsafe { windows::Win32::System::Threading::SetEvent(self.stop_event) };
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = unsafe { windows::Win32::Foundation::CloseHandle(self.stop_event) };
    }
}

#[cfg(target_os = "windows")]
fn capture_loop(
    stop_event: windows::Win32::Foundation::HANDLE,
    endpoint: CaptureEndpoint,
    sender: crossbeam_channel::Sender<PcmChunk>,
    ready: &std::sync::mpsc::SyncSender<Result<AudioFormat, AudioError>>,
) -> Result<(), AudioError> {
    use windows::Win32::Foundation::{CloseHandle, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT};
    use windows::Win32::Media::Audio::{
        AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_LOOPBACK,
        AudioCategory_Communications, IAudioCaptureClient, IMMDeviceEnumerator, MMDeviceEnumerator,
        eCapture, eCommunications, eConsole, eRender,
    };
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
    };
    use windows::Win32::System::Threading::{
        AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW, CreateEventW, INFINITE,
        WaitForMultipleObjects, WaitForSingleObject,
    };

    const MICROPHONE_BUFFER_DURATION_HNS: i64 = 2_000_000;

    // Microsoft documents the first IAudioClient activation on Windows 8+
    // from an STA. Each capture stream owns this dedicated COM apartment.
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
        .ok()
        .map_err(|error| AudioError(error.to_string()))?;
    let mut task_index = 0;
    let mmcss =
        unsafe { AvSetMmThreadCharacteristicsW(windows::core::w!("Audio"), &mut task_index).ok() };
    let result = (|| -> Result<(), AudioError> {
        let enumerator: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
                .map_err(|error| AudioError(error.to_string()))?;
        let (device, stream_flags, category, buffer_duration_hns, timer_driven) = match endpoint {
            CaptureEndpoint::Loopback => (
                unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eConsole) }
                    .map_err(|error| AudioError(error.to_string()))?,
                AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                None,
                0,
                false,
            ),
            CaptureEndpoint::Microphone {
                endpoint_id: Some(endpoint_id),
            } => {
                let wide_id = endpoint_id
                    .encode_utf16()
                    .chain(Some(0))
                    .collect::<Vec<_>>();
                (
                    unsafe { enumerator.GetDevice(windows::core::PCWSTR(wide_id.as_ptr())) }
                        .map_err(|error| {
                            AudioError(format!(
                                "configured microphone endpoint `{endpoint_id}` is unavailable: {error}"
                            ))
                        })?,
                    0,
                    Some(AudioCategory_Communications),
                    MICROPHONE_BUFFER_DURATION_HNS,
                    true,
                )
            }
            CaptureEndpoint::Microphone { endpoint_id: None } => (
                unsafe { enumerator.GetDefaultAudioEndpoint(eCapture, eCommunications) }
                    .map_err(|error| AudioError(error.to_string()))?,
                0,
                Some(AudioCategory_Communications),
                MICROPHONE_BUFFER_DURATION_HNS,
                true,
            ),
        };
        let (client, format, device_period_hns, format_mode) =
            initialize_capture_client(&device, stream_flags, buffer_duration_hns, category)?;
        let endpoint_name = if timer_driven {
            "microphone"
        } else {
            "desktop"
        };
        let endpoint_buffer_frames = unsafe { client.GetBufferSize() }.unwrap_or_default();
        eprintln!(
            "Wreath {endpoint_name} capture: {format_mode}, {} Hz, {} channel(s), {}-bit, buffer {} frames",
            format.sample_rate, format.channels, format.bits_per_sample, endpoint_buffer_frames
        );
        if timer_driven {
            log_audio_effects(&client);
        }
        let audio_event = if timer_driven {
            None
        } else {
            let event = unsafe { CreateEventW(None, false, false, None) }
                .map_err(|error| AudioError(error.to_string()))?;
            unsafe { client.SetEventHandle(event) }
                .map_err(|error| AudioError(error.to_string()))?;
            Some(event)
        };
        let capture: IAudioCaptureClient =
            unsafe { client.GetService() }.map_err(|error| AudioError(error.to_string()))?;
        unsafe { client.Start() }.map_err(|error| AudioError(error.to_string()))?;
        if ready.send(Ok(format)).is_err() {
            let _ = unsafe { client.Stop() };
            if let Some(audio_event) = audio_event {
                let _ = unsafe { CloseHandle(audio_event) };
            }
            return Ok(());
        }

        let mut clock = CapturePacketClock::default();
        let mut dropped_packet = false;
        let mut diagnostics = CaptureDiagnostics::new(endpoint_name);
        if timer_driven {
            let poll_interval_ms = capture_poll_interval_ms(device_period_hns);
            loop {
                let wait = unsafe { WaitForSingleObject(stop_event, poll_interval_ms) };
                if wait == WAIT_FAILED {
                    let _ = unsafe { client.Stop() };
                    return Err(AudioError(std::io::Error::last_os_error().to_string()));
                }
                if wait == WAIT_OBJECT_0 {
                    break;
                }
                if wait == WAIT_TIMEOUT {
                    read_available_packets(
                        &capture,
                        format,
                        &sender,
                        &mut clock,
                        &mut dropped_packet,
                        &mut diagnostics,
                    )?;
                }
            }
        } else if let Some(audio_event) = audio_event {
            let handles = [audio_event, stop_event];
            loop {
                let wait = unsafe { WaitForMultipleObjects(&handles, false, INFINITE) };
                if wait == WAIT_FAILED {
                    let _ = unsafe { client.Stop() };
                    let _ = unsafe { CloseHandle(audio_event) };
                    return Err(AudioError(std::io::Error::last_os_error().to_string()));
                }
                if wait.0 == WAIT_OBJECT_0.0 + 1 {
                    break;
                }
                read_available_packets(
                    &capture,
                    format,
                    &sender,
                    &mut clock,
                    &mut dropped_packet,
                    &mut diagnostics,
                )?;
            }
        }
        let _ = unsafe { client.Stop() };
        if let Some(audio_event) = audio_event {
            let _ = unsafe { CloseHandle(audio_event) };
        }
        Ok(())
    })();
    if let Some(mmcss) = mmcss {
        let _ = unsafe { AvRevertMmThreadCharacteristics(mmcss) };
    }
    unsafe { CoUninitialize() };
    result
}

#[cfg(target_os = "windows")]
fn initialize_capture_client(
    device: &windows::Win32::Media::Audio::IMMDevice,
    stream_flags: u32,
    buffer_duration_hns: i64,
    category: Option<windows::Win32::Media::Audio::AUDIO_STREAM_CATEGORY>,
) -> Result<
    (
        windows::Win32::Media::Audio::IAudioClient2,
        AudioFormat,
        i64,
        CaptureFormatMode,
    ),
    AudioError,
> {
    use windows::Win32::Media::Audio::{
        AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
        AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, WAVEFORMATEX,
    };
    if let Some(category) = category {
        let (client, native_format, device_period_hns) =
            prepare_capture_client(device, Some(category))?;
        let sample_rate = preferred_microphone_sample_rate(native_format.sample_rate);
        let desired = WAVEFORMATEX {
            wFormatTag: windows::Win32::Media::Audio::WAVE_FORMAT_PCM as u16,
            nChannels: 1,
            nSamplesPerSec: sample_rate,
            nAvgBytesPerSec: sample_rate.saturating_mul(2),
            nBlockAlign: 2,
            wBitsPerSample: 16,
            cbSize: 0,
        };
        let preferred_flags = stream_flags
            | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
            | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;
        match unsafe {
            client.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                preferred_flags,
                buffer_duration_hns,
                0,
                &desired,
                None,
            )
        } {
            Ok(()) => {
                return Ok((
                    client,
                    AudioFormat {
                        sample_rate,
                        channels: 1,
                        bits_per_sample: 16,
                        block_align: 2,
                        floating_point: false,
                    },
                    device_period_hns,
                    CaptureFormatMode::SystemMono,
                ));
            }
            Err(preferred_error) => {
                eprintln!(
                    "Wreath microphone: Windows mono conversion unavailable ({preferred_error}); retrying the endpoint's native format"
                );
            }
        }

        // IAudioClient cannot be reinitialized reliably after Initialize has
        // failed. Activate a fresh client for the native-format fallback.
        let (fallback, native_format, fallback_period_hns) =
            prepare_capture_client(device, Some(category))?;
        initialize_native_client(
            fallback,
            native_format,
            fallback_period_hns,
            stream_flags,
            buffer_duration_hns,
            CaptureFormatMode::NativeMicrophoneFallback,
        )
    } else {
        let (client, native_format, device_period_hns) = prepare_capture_client(device, None)?;
        initialize_native_client(
            client,
            native_format,
            device_period_hns,
            stream_flags,
            buffer_duration_hns,
            CaptureFormatMode::NativeLoopback,
        )
    }
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy)]
enum CaptureFormatMode {
    SystemMono,
    NativeMicrophoneFallback,
    NativeLoopback,
}

#[cfg(target_os = "windows")]
impl fmt::Display for CaptureFormatMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::SystemMono => "Windows-processed communications mono",
            Self::NativeMicrophoneFallback => "native microphone fallback",
            Self::NativeLoopback => "native loopback",
        })
    }
}

#[cfg(any(target_os = "windows", test))]
fn preferred_microphone_sample_rate(native_sample_rate: u32) -> u32 {
    if native_sample_rate == 44_100 {
        44_100
    } else {
        48_000
    }
}

#[cfg(target_os = "windows")]
fn prepare_capture_client(
    device: &windows::Win32::Media::Audio::IMMDevice,
    category: Option<windows::Win32::Media::Audio::AUDIO_STREAM_CATEGORY>,
) -> Result<
    (
        windows::Win32::Media::Audio::IAudioClient2,
        AudioFormat,
        i64,
    ),
    AudioError,
> {
    use windows::Win32::Media::Audio::{
        AUDCLNT_STREAMOPTIONS_NONE, AudioClientProperties, IAudioClient2,
    };
    use windows::Win32::System::Com::{CLSCTX_ALL, CoTaskMemFree};

    let client: IAudioClient2 = unsafe { device.Activate(CLSCTX_ALL, None) }
        .map_err(|error| AudioError(error.to_string()))?;
    if let Some(category) = category {
        let properties = AudioClientProperties {
            cbSize: std::mem::size_of::<AudioClientProperties>() as u32,
            bIsOffload: false.into(),
            eCategory: category,
            Options: AUDCLNT_STREAMOPTIONS_NONE,
        };
        unsafe { client.SetClientProperties(&properties) }.map_err(|error| {
            AudioError(format!("cannot enable Windows voice processing: {error}"))
        })?;
    }
    let mix_format =
        unsafe { client.GetMixFormat() }.map_err(|error| AudioError(error.to_string()))?;
    if mix_format.is_null() {
        return Err(AudioError("WASAPI returned no mix format".into()));
    }
    let format = unsafe { describe_format(mix_format) };
    unsafe { CoTaskMemFree(Some(mix_format.cast())) };
    let mut device_period_hns = 0_i64;
    unsafe { client.GetDevicePeriod(Some(&mut device_period_hns), None) }
        .map_err(|error| AudioError(error.to_string()))?;
    Ok((client, format, device_period_hns))
}

#[cfg(target_os = "windows")]
fn initialize_native_client(
    client: windows::Win32::Media::Audio::IAudioClient2,
    format: AudioFormat,
    device_period_hns: i64,
    stream_flags: u32,
    buffer_duration_hns: i64,
    mode: CaptureFormatMode,
) -> Result<
    (
        windows::Win32::Media::Audio::IAudioClient2,
        AudioFormat,
        i64,
        CaptureFormatMode,
    ),
    AudioError,
> {
    use windows::Win32::Media::Audio::AUDCLNT_SHAREMODE_SHARED;
    use windows::Win32::System::Com::CoTaskMemFree;

    let mix_format =
        unsafe { client.GetMixFormat() }.map_err(|error| AudioError(error.to_string()))?;
    if mix_format.is_null() {
        return Err(AudioError("WASAPI returned no mix format".into()));
    }
    let initialized = unsafe {
        client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            stream_flags,
            buffer_duration_hns,
            0,
            mix_format,
            None,
        )
    };
    unsafe { CoTaskMemFree(Some(mix_format.cast())) };
    initialized.map_err(|error| AudioError(error.to_string()))?;
    Ok((client, format, device_period_hns, mode))
}

#[cfg(target_os = "windows")]
fn capture_poll_interval_ms(device_period_hns: i64) -> u32 {
    let half_period_ms = device_period_hns.max(0) as u64 / 20_000;
    u32::try_from(half_period_ms.clamp(2, 10)).unwrap_or(10)
}

#[cfg(target_os = "windows")]
fn log_audio_effects(client: &windows::Win32::Media::Audio::IAudioClient2) {
    use windows::Win32::Media::Audio::{AUDIO_EFFECT_STATE_ON, IAudioEffectsManager};
    use windows::Win32::System::Com::CoTaskMemFree;

    let Ok(manager) = (unsafe { client.GetService::<IAudioEffectsManager>() }) else {
        eprintln!("Wreath microphone: Windows audio-effect enumeration is unavailable");
        return;
    };
    let mut effects = std::ptr::null_mut();
    let mut count = 0_u32;
    if let Err(error) = unsafe { manager.GetAudioEffects(&mut effects, &mut count) } {
        eprintln!("Wreath microphone: cannot enumerate Windows audio effects: {error}");
        return;
    }
    if effects.is_null() || count == 0 {
        eprintln!("Wreath microphone: no endpoint audio effects reported");
        if !effects.is_null() {
            unsafe { CoTaskMemFree(Some(effects.cast())) };
        }
        return;
    }
    let effects_slice = unsafe { std::slice::from_raw_parts(effects, count as usize) };
    let summary = effects_slice
        .iter()
        .map(|effect| {
            format!(
                "{}:{}",
                audio_effect_name(effect.id),
                if effect.state == AUDIO_EFFECT_STATE_ON {
                    "on"
                } else {
                    "off"
                }
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    eprintln!("Wreath microphone Windows audio effects: {summary}");
    unsafe { CoTaskMemFree(Some(effects.cast())) };
}

#[cfg(target_os = "windows")]
fn audio_effect_name(id: windows::core::GUID) -> String {
    use windows::Win32::Media::KernelStreaming::{
        AUDIO_EFFECT_TYPE_ACOUSTIC_ECHO_CANCELLATION, AUDIO_EFFECT_TYPE_AUTOMATIC_GAIN_CONTROL,
        AUDIO_EFFECT_TYPE_BEAMFORMING, AUDIO_EFFECT_TYPE_DEEP_NOISE_SUPPRESSION,
        AUDIO_EFFECT_TYPE_FAR_FIELD_BEAMFORMING, AUDIO_EFFECT_TYPE_NOISE_SUPPRESSION,
    };

    if id == AUDIO_EFFECT_TYPE_ACOUSTIC_ECHO_CANCELLATION {
        "echo-cancellation".into()
    } else if id == AUDIO_EFFECT_TYPE_AUTOMATIC_GAIN_CONTROL {
        "automatic-gain".into()
    } else if id == AUDIO_EFFECT_TYPE_BEAMFORMING {
        "beamforming".into()
    } else if id == AUDIO_EFFECT_TYPE_FAR_FIELD_BEAMFORMING {
        "far-field-beamforming".into()
    } else if id == AUDIO_EFFECT_TYPE_NOISE_SUPPRESSION {
        "noise-suppression".into()
    } else if id == AUDIO_EFFECT_TYPE_DEEP_NOISE_SUPPRESSION {
        "deep-noise-suppression".into()
    } else {
        format!("{id:?}")
    }
}

#[cfg(target_os = "windows")]
struct CaptureDiagnostics {
    endpoint: &'static str,
    discontinuities: u64,
    timestamp_errors: u64,
    queue_drops: u64,
}

#[cfg(target_os = "windows")]
impl CaptureDiagnostics {
    fn new(endpoint: &'static str) -> Self {
        Self {
            endpoint,
            discontinuities: 0,
            timestamp_errors: 0,
            queue_drops: 0,
        }
    }

    fn discontinuity(&mut self) {
        Self::record(
            self.endpoint,
            "WASAPI discontinuities",
            &mut self.discontinuities,
        );
    }

    fn timestamp_error(&mut self) {
        Self::record(
            self.endpoint,
            "WASAPI timestamp errors",
            &mut self.timestamp_errors,
        );
    }

    fn queue_drop(&mut self) {
        Self::record(self.endpoint, "queue drops", &mut self.queue_drops);
    }

    fn record(endpoint: &str, label: &str, counter: &mut u64) {
        *counter = counter.saturating_add(1);
        if counter.is_power_of_two() {
            eprintln!("Wreath {endpoint} capture diagnostics: {label}={counter}");
        }
    }
}

#[cfg(target_os = "windows")]
#[derive(Default)]
struct CapturePacketClock {
    next_timestamp: Option<std::time::Duration>,
}

#[cfg(target_os = "windows")]
impl CapturePacketClock {
    fn timestamp(
        &mut self,
        qpc_position: u64,
        frames: u32,
        sample_rate: u32,
        timestamp_error: bool,
    ) -> std::time::Duration {
        let reported = std::time::Duration::from_nanos(qpc_position.saturating_mul(100));
        let timestamp = if timestamp_error {
            self.next_timestamp.unwrap_or(reported)
        } else {
            reported
        };
        self.next_timestamp = Some(timestamp.saturating_add(std::time::Duration::from_nanos(
            u64::from(frames).saturating_mul(1_000_000_000) / u64::from(sample_rate.max(1)),
        )));
        timestamp
    }
}

#[cfg(target_os = "windows")]
unsafe fn describe_format(
    format: *const windows::Win32::Media::Audio::WAVEFORMATEX,
) -> AudioFormat {
    use windows::Win32::Media::KernelStreaming::WAVE_FORMAT_EXTENSIBLE;
    use windows::Win32::Media::Multimedia::{
        KSDATAFORMAT_SUBTYPE_IEEE_FLOAT, WAVE_FORMAT_IEEE_FLOAT,
    };

    let wave = unsafe { format.read_unaligned() };
    let floating_point = if u32::from(wave.wFormatTag) == WAVE_FORMAT_IEEE_FLOAT {
        true
    } else if u32::from(wave.wFormatTag) == WAVE_FORMAT_EXTENSIBLE {
        let extensible = format.cast::<windows::Win32::Media::Audio::WAVEFORMATEXTENSIBLE>();
        let subtype = unsafe { std::ptr::addr_of!((*extensible).SubFormat).read_unaligned() };
        subtype == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT
    } else {
        false
    };
    AudioFormat {
        sample_rate: wave.nSamplesPerSec,
        channels: wave.nChannels,
        bits_per_sample: wave.wBitsPerSample,
        block_align: wave.nBlockAlign,
        floating_point,
    }
}

#[cfg(target_os = "windows")]
fn read_available_packets(
    capture: &windows::Win32::Media::Audio::IAudioCaptureClient,
    format: AudioFormat,
    sender: &crossbeam_channel::Sender<PcmChunk>,
    clock: &mut CapturePacketClock,
    dropped_packet: &mut bool,
    diagnostics: &mut CaptureDiagnostics,
) -> Result<(), AudioError> {
    use windows::Win32::Media::Audio::{
        AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY, AUDCLNT_BUFFERFLAGS_SILENT,
        AUDCLNT_BUFFERFLAGS_TIMESTAMP_ERROR,
    };

    loop {
        let packet_frames = unsafe { capture.GetNextPacketSize() }
            .map_err(|error| AudioError(error.to_string()))?;
        if packet_frames == 0 {
            return Ok(());
        }
        let mut data = std::ptr::null_mut();
        let mut frames = 0_u32;
        let mut flags = 0_u32;
        let mut qpc_position = 0_u64;
        unsafe {
            capture.GetBuffer(
                &mut data,
                &mut frames,
                &mut flags,
                None,
                Some(&mut qpc_position),
            )
        }
        .map_err(|error| AudioError(error.to_string()))?;
        let length = format
            .bytes_for_frames(frames)
            .ok_or_else(|| AudioError("WASAPI packet size overflow".into()))?;
        let silent = flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0;
        let timestamp_error = flags & AUDCLNT_BUFFERFLAGS_TIMESTAMP_ERROR.0 as u32 != 0;
        let wasapi_discontinuous = flags & AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY.0 as u32 != 0;
        if wasapi_discontinuous {
            diagnostics.discontinuity();
        }
        if timestamp_error {
            diagnostics.timestamp_error();
        }
        let discontinuous = *dropped_packet || wasapi_discontinuous;
        let bytes = if silent {
            vec![0; length].into_boxed_slice()
        } else if data.is_null() {
            let _ = unsafe { capture.ReleaseBuffer(frames) };
            return Err(AudioError("WASAPI returned a null audio packet".into()));
        } else {
            unsafe { std::slice::from_raw_parts(data, length) }
                .to_vec()
                .into_boxed_slice()
        };
        unsafe { capture.ReleaseBuffer(frames) }.map_err(|error| AudioError(error.to_string()))?;
        let chunk = PcmChunk {
            timestamp: clock.timestamp(qpc_position, frames, format.sample_rate, timestamp_error),
            frames,
            discontinuous,
            data: bytes,
        };
        match sender.try_send(chunk) {
            Ok(()) => *dropped_packet = false,
            Err(crossbeam_channel::TrySendError::Full(_)) => {
                diagnostics.queue_drop();
                *dropped_packet = true;
            }
            Err(crossbeam_channel::TrySendError::Disconnected(_)) => return Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_byte_math_is_checked() {
        let format = AudioFormat {
            sample_rate: 48_000,
            channels: 2,
            bits_per_sample: 32,
            block_align: 8,
            floating_point: true,
        };

        assert_eq!(format.bytes_for_frames(480), Some(3_840));
    }

    #[test]
    fn microphone_conversion_uses_standard_communications_rates() {
        assert_eq!(preferred_microphone_sample_rate(44_100), 44_100);
        assert_eq!(preferred_microphone_sample_rate(48_000), 48_000);
        assert_eq!(preferred_microphone_sample_rate(96_000), 48_000);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn microphone_polling_tracks_half_the_device_period_with_safe_bounds() {
        assert_eq!(capture_poll_interval_ms(0), 2);
        assert_eq!(capture_poll_interval_ms(100_000), 5);
        assert_eq!(capture_poll_interval_ms(1_000_000), 10);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn uncertain_wasapi_timestamps_continue_from_the_previous_packet() {
        let mut clock = CapturePacketClock::default();
        let first = clock.timestamp(10_000, 480, 48_000, false);
        let uncertain = clock.timestamp(1, 480, 48_000, true);

        assert_eq!(first, std::time::Duration::from_millis(1));
        assert_eq!(uncertain, std::time::Duration::from_millis(11));
    }

    #[test]
    fn normalizes_float_stereo_to_packed_pcm16() {
        let format = AudioFormat {
            sample_rate: 48_000,
            channels: 2,
            bits_per_sample: 32,
            block_align: 8,
            floating_point: true,
        };
        let data = [-1.0_f32, -0.5, 0.5, 1.0]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>()
            .into_boxed_slice();

        let normalized = normalize_to_pcm16(
            format,
            PcmChunk {
                timestamp: std::time::Duration::from_secs(2),
                frames: 2,
                discontinuous: true,
                data,
            },
        )
        .unwrap();

        let samples = normalized
            .data
            .chunks_exact(2)
            .map(|sample| i16::from_le_bytes(sample.try_into().unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(samples, [i16::MIN, -16_384, 16_384, i16::MAX]);
        assert_eq!(normalized.timestamp, std::time::Duration::from_secs(2));
        assert_eq!(normalized.frames, 2);
        assert!(normalized.discontinuous);
    }

    #[test]
    fn normalizes_integer_widths_without_unbounded_buffers() {
        let formats_and_data = [
            (8, vec![0x00, 0x80, 0xff], vec![i16::MIN, 0, 32_512]),
            (
                24,
                vec![0x00, 0x00, 0x80, 0x00, 0x00, 0x00, 0xff, 0xff, 0x7f],
                vec![i16::MIN, 0, i16::MAX],
            ),
            (
                32,
                vec![0x00, 0x00, 0x00, 0x80, 0, 0, 0, 0, 0xff, 0xff, 0xff, 0x7f],
                vec![i16::MIN, 0, i16::MAX],
            ),
        ];

        for (bits, data, expected) in formats_and_data {
            let format = AudioFormat {
                sample_rate: 48_000,
                channels: 1,
                bits_per_sample: bits,
                block_align: bits / 8,
                floating_point: false,
            };
            let normalized = normalize_to_pcm16(
                format,
                PcmChunk {
                    timestamp: std::time::Duration::ZERO,
                    frames: 3,
                    discontinuous: false,
                    data: data.into_boxed_slice(),
                },
            )
            .unwrap();
            let samples = normalized
                .data
                .chunks_exact(2)
                .map(|sample| i16::from_le_bytes(sample.try_into().unwrap()))
                .collect::<Vec<_>>();
            assert_eq!(samples, expected);
            assert_eq!(normalized.data.len(), 6);
        }
    }

    #[test]
    fn rejects_truncated_and_unknown_audio_packets() {
        let base = AudioFormat {
            sample_rate: 48_000,
            channels: 2,
            bits_per_sample: 16,
            block_align: 4,
            floating_point: false,
        };
        let chunk = PcmChunk {
            timestamp: std::time::Duration::ZERO,
            frames: 2,
            discontinuous: false,
            data: vec![0; 7].into_boxed_slice(),
        };
        assert!(normalize_to_pcm16(base, chunk).is_err());

        let unsupported = AudioFormat {
            bits_per_sample: 64,
            floating_point: true,
            block_align: 16,
            ..base
        };
        let chunk = PcmChunk {
            timestamp: std::time::Duration::ZERO,
            frames: 1,
            discontinuous: false,
            data: vec![0; 16].into_boxed_slice(),
        };
        assert!(normalize_to_pcm16(unsupported, chunk).is_err());
    }
}
