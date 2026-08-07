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
    pub data: Box<[u8]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pcm16Chunk {
    pub timestamp: std::time::Duration,
    pub frames: u32,
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

/// Event-driven WASAPI microphone capture using the endpoint's native shared
/// mix format. Conversion happens continuously downstream so driver-specific
/// format conversion cannot introduce packet-edge artifacts.
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
    use windows::Win32::Foundation::{CloseHandle, WAIT_FAILED, WAIT_OBJECT_0};
    use windows::Win32::Media::Audio::{
        AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_LOOPBACK,
        AudioCategory_Communications, IAudioCaptureClient, IMMDeviceEnumerator, MMDeviceEnumerator,
        eCapture, eCommunications, eConsole, eRender,
    };
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
    };
    use windows::Win32::System::Threading::{
        AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW, CreateEventW, INFINITE,
        WaitForMultipleObjects,
    };

    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
        .ok()
        .map_err(|error| AudioError(error.to_string()))?;
    let mut task_index = 0;
    let mmcss =
        unsafe { AvSetMmThreadCharacteristicsW(windows::core::w!("Audio"), &mut task_index).ok() };
    let result = (|| -> Result<(), AudioError> {
        let enumerator: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
                .map_err(|error| AudioError(error.to_string()))?;
        let (device, stream_flags, category) = match endpoint {
            CaptureEndpoint::Loopback => (
                unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eConsole) }
                    .map_err(|error| AudioError(error.to_string()))?,
                AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                None,
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
                    AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                    Some(AudioCategory_Communications),
                )
            }
            CaptureEndpoint::Microphone { endpoint_id: None } => (
                unsafe { enumerator.GetDefaultAudioEndpoint(eCapture, eCommunications) }
                    .map_err(|error| AudioError(error.to_string()))?,
                AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                Some(AudioCategory_Communications),
            ),
        };
        let (client, format) = initialize_capture_client(&device, stream_flags, category)?;
        let audio_event = unsafe { CreateEventW(None, false, false, None) }
            .map_err(|error| AudioError(error.to_string()))?;
        unsafe { client.SetEventHandle(audio_event) }
            .map_err(|error| AudioError(error.to_string()))?;
        let capture: IAudioCaptureClient =
            unsafe { client.GetService() }.map_err(|error| AudioError(error.to_string()))?;
        unsafe { client.Start() }.map_err(|error| AudioError(error.to_string()))?;
        if ready.send(Ok(format)).is_err() {
            let _ = unsafe { client.Stop() };
            let _ = unsafe { CloseHandle(audio_event) };
            return Ok(());
        }

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
            read_available_packets(&capture, format, &sender)?;
        }
        let _ = unsafe { client.Stop() };
        let _ = unsafe { CloseHandle(audio_event) };
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
    category: Option<windows::Win32::Media::Audio::AUDIO_STREAM_CATEGORY>,
) -> Result<(windows::Win32::Media::Audio::IAudioClient2, AudioFormat), AudioError> {
    use windows::Win32::Media::Audio::{
        AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMOPTIONS_NONE, AudioClientProperties, IAudioClient2,
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
    let initialized = unsafe {
        client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            stream_flags,
            0,
            0,
            mix_format,
            None,
        )
    };
    unsafe { CoTaskMemFree(Some(mix_format.cast())) };
    initialized.map_err(|error| AudioError(error.to_string()))?;
    Ok((client, format))
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
) -> Result<(), AudioError> {
    use windows::Win32::Media::Audio::AUDCLNT_BUFFERFLAGS_SILENT;

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
            timestamp: std::time::Duration::from_nanos(qpc_position.saturating_mul(100)),
            frames,
            data: bytes,
        };
        match sender.try_send(chunk) {
            Ok(())
            | Err(crossbeam_channel::TrySendError::Full(_))
            | Err(crossbeam_channel::TrySendError::Disconnected(_)) => {}
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
            data: vec![0; 16].into_boxed_slice(),
        };
        assert!(normalize_to_pcm16(unsupported, chunk).is_err());
    }
}
