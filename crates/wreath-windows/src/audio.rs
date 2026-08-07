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
pub struct AudioError(pub String);

impl fmt::Display for AudioError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Windows audio capture failed: {}", self.0)
    }
}

impl std::error::Error for AudioError {}

/// Event-driven WASAPI loopback capture. Its queue is intentionally bounded;
/// lagging consumers lose old capture callbacks instead of growing memory.
#[cfg(target_os = "windows")]
pub struct LoopbackCapture {
    format: AudioFormat,
    receiver: crossbeam_channel::Receiver<PcmChunk>,
    stop_event: windows::Win32::Foundation::HANDLE,
    thread: Option<std::thread::JoinHandle<()>>,
}

#[cfg(target_os = "windows")]
impl LoopbackCapture {
    pub fn spawn() -> Result<Self, AudioError> {
        use std::sync::mpsc;

        use windows::Win32::System::Threading::CreateEventW;

        let stop_event = unsafe { CreateEventW(None, false, false, None) }
            .map_err(|error| AudioError(error.to_string()))?;
        let stop_for_thread = stop_event.0 as usize;
        let (chunk_sender, chunk_receiver) = crossbeam_channel::bounded(8);
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("wreath-wasapi".into())
            .spawn(move || {
                let stop_for_thread =
                    windows::Win32::Foundation::HANDLE(stop_for_thread as *mut std::ffi::c_void);
                let result = capture_loop(stop_for_thread, chunk_sender, &ready_sender);
                if let Err(error) = result {
                    let _ = ready_sender.send(Err(error));
                }
            })
            .map_err(|error| AudioError(error.to_string()))?;
        match ready_receiver.recv() {
            Ok(Ok(format)) => Ok(Self {
                format,
                receiver: chunk_receiver,
                stop_event,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                unsafe { windows::Win32::Foundation::CloseHandle(stop_event) }
                    .map_err(|close_error| AudioError(close_error.to_string()))?;
                Err(error)
            }
            Err(error) => {
                let _ = thread.join();
                let _ = unsafe { windows::Win32::Foundation::CloseHandle(stop_event) };
                Err(AudioError(error.to_string()))
            }
        }
    }

    pub fn format(&self) -> AudioFormat {
        self.format
    }

    pub fn receiver(&self) -> &crossbeam_channel::Receiver<PcmChunk> {
        &self.receiver
    }
}

#[cfg(target_os = "windows")]
impl Drop for LoopbackCapture {
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
    sender: crossbeam_channel::Sender<PcmChunk>,
    ready: &std::sync::mpsc::SyncSender<Result<AudioFormat, AudioError>>,
) -> Result<(), AudioError> {
    use windows::Win32::Foundation::{CloseHandle, WAIT_FAILED, WAIT_OBJECT_0};
    use windows::Win32::Media::Audio::{
        AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_LOOPBACK,
        IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator, eConsole,
        eRender,
    };
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
        CoUninitialize,
    };
    use windows::Win32::System::Threading::{CreateEventW, INFINITE, WaitForMultipleObjects};

    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
        .ok()
        .map_err(|error| AudioError(error.to_string()))?;
    let result = (|| -> Result<(), AudioError> {
        let enumerator: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
                .map_err(|error| AudioError(error.to_string()))?;
        let device = unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eConsole) }
            .map_err(|error| AudioError(error.to_string()))?;
        let client: IAudioClient = unsafe { device.Activate(CLSCTX_ALL, None) }
            .map_err(|error| AudioError(error.to_string()))?;
        let mix_format =
            unsafe { client.GetMixFormat() }.map_err(|error| AudioError(error.to_string()))?;
        if mix_format.is_null() {
            return Err(AudioError("WASAPI returned no mix format".into()));
        }
        let format = unsafe { describe_format(mix_format) };
        let audio_event = unsafe { CreateEventW(None, false, false, None) }
            .map_err(|error| AudioError(error.to_string()))?;
        let initialize_result = unsafe {
            client.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                0,
                0,
                mix_format,
                None,
            )
        };
        unsafe { CoTaskMemFree(Some(mix_format.cast())) };
        initialize_result.map_err(|error| AudioError(error.to_string()))?;
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
    unsafe { CoUninitialize() };
    result
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
}
