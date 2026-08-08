use std::path::Path;
#[cfg(any(target_os = "windows", test))]
use std::time::Duration;

use wreath_core::trim::{ClipTiming, CutPlan, TrimBackend, TrimError};

#[cfg(target_os = "windows")]
use windows::Win32::Media::MediaFoundation::{
    IMFMediaType, IMFSample, IMFSinkWriter, IMFSourceReader,
};

#[cfg(target_os = "windows")]
const AAC_BYTES_PER_SECOND: u32 = 20_000;
#[cfg(target_os = "windows")]
const MAX_STREAMS: u32 = 16;

pub struct MediaFoundationTrimmer {
    #[cfg(target_os = "windows")]
    _runtime: Runtime,
}

impl MediaFoundationTrimmer {
    #[cfg(target_os = "windows")]
    pub fn new() -> Result<Self, TrimError> {
        Ok(Self {
            _runtime: Runtime::start()?,
        })
    }

    #[cfg(not(target_os = "windows"))]
    pub fn new() -> Result<Self, TrimError> {
        Err(TrimError::Unsupported(
            "Media Foundation is only available on Windows".into(),
        ))
    }
}

impl TrimBackend for MediaFoundationTrimmer {
    #[cfg(target_os = "windows")]
    fn timing(&self, source: &Path) -> Result<ClipTiming, TrimError> {
        scan(source)
    }

    #[cfg(not(target_os = "windows"))]
    fn timing(&self, _source: &Path) -> Result<ClipTiming, TrimError> {
        Err(TrimError::Unsupported(
            "Media Foundation is only available on Windows".into(),
        ))
    }

    #[cfg(target_os = "windows")]
    fn cut(&self, plan: &CutPlan) -> Result<(), TrimError> {
        if plan.container != wreath_core::trim::Container::Mp4 {
            return Err(TrimError::Unsupported(
                "Windows can only cut mp4 clips".into(),
            ));
        }
        if plan.reencode {
            reencode(plan)
        } else {
            copy(plan)
        }
    }

    #[cfg(not(target_os = "windows"))]
    fn cut(&self, _plan: &CutPlan) -> Result<(), TrimError> {
        Err(TrimError::Unsupported(
            "Media Foundation is only available on Windows".into(),
        ))
    }
}

#[cfg(target_os = "windows")]
struct Runtime {
    owns_com: bool,
}

#[cfg(target_os = "windows")]
static MEDIA_FOUNDATION: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();

#[cfg(target_os = "windows")]
fn start_media_foundation() -> Result<(), TrimError> {
    MEDIA_FOUNDATION
        .get_or_init(|| {
            use windows::Win32::Media::MediaFoundation::{MF_VERSION, MFSTARTUP_FULL, MFStartup};

            unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) }.map_err(|error| error.to_string())
        })
        .clone()
        .map_err(TrimError::Backend)
}

#[cfg(target_os = "windows")]
impl Runtime {
    fn start() -> Result<Self, TrimError> {
        use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
        use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};

        let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if result.is_err() && result != RPC_E_CHANGED_MODE {
            return Err(TrimError::Backend(format!(
                "COM initialization failed: {result:?}"
            )));
        }
        let owns_com = result.is_ok();
        if let Err(error) = start_media_foundation() {
            if owns_com {
                unsafe { CoUninitialize() };
            }
            return Err(error);
        }
        Ok(Self { owns_com })
    }
}

#[cfg(target_os = "windows")]
impl Drop for Runtime {
    fn drop(&mut self) {
        if self.owns_com {
            unsafe { windows::Win32::System::Com::CoUninitialize() };
        }
    }
}

#[cfg(target_os = "windows")]
struct Streams {
    video: u32,
    audio: Option<u32>,
}

#[cfg(target_os = "windows")]
impl Streams {
    fn discover(reader: &IMFSourceReader) -> Result<Self, TrimError> {
        use windows::Win32::Media::MediaFoundation::{
            MF_MT_MAJOR_TYPE, MFMediaType_Audio, MFMediaType_Video,
        };

        let mut video = None;
        let mut audio = None;
        for index in 0..MAX_STREAMS {
            let Ok(media_type) = (unsafe { reader.GetNativeMediaType(index, 0) }) else {
                break;
            };
            let Ok(major) = (unsafe { media_type.GetGUID(&MF_MT_MAJOR_TYPE) }) else {
                continue;
            };
            if major == MFMediaType_Video && video.is_none() {
                video = Some(index);
            } else if major == MFMediaType_Audio && audio.is_none() {
                audio = Some(index);
            }
        }
        let video = video.ok_or_else(|| TrimError::Unsupported("the clip has no video".into()))?;
        Ok(Self { video, audio })
    }

    fn select(&self, reader: &IMFSourceReader) -> Result<(), TrimError> {
        use windows::Win32::Media::MediaFoundation::MF_SOURCE_READER_ALL_STREAMS;

        unsafe { reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false) }
            .map_err(backend_error)?;
        unsafe { reader.SetStreamSelection(self.video, true) }.map_err(backend_error)?;
        if let Some(audio) = self.audio {
            unsafe { reader.SetStreamSelection(audio, true) }.map_err(backend_error)?;
        }
        Ok(())
    }
}

#[cfg(target_os = "windows")]
fn scan(source: &Path) -> Result<ClipTiming, TrimError> {
    use windows::Win32::Media::MediaFoundation::MFSampleExtension_CleanPoint;

    let reader = open(source, None)?;
    let streams = Streams::discover(&reader)?;
    unsafe {
        reader.SetStreamSelection(
            windows::Win32::Media::MediaFoundation::MF_SOURCE_READER_ALL_STREAMS.0 as u32,
            false,
        )
    }
    .map_err(backend_error)?;
    unsafe { reader.SetStreamSelection(streams.video, true) }.map_err(backend_error)?;

    let mut keyframes = Vec::new();
    let mut duration = Duration::ZERO;
    loop {
        let Some((_, sample, timestamp)) = read(&reader, streams.video)? else {
            break;
        };
        let keyframe = unsafe { sample.GetUINT32(&MFSampleExtension_CleanPoint) }.unwrap_or(0) != 0;
        if keyframe {
            keyframes.push(from_hns(timestamp));
        }
        let length = unsafe { sample.GetSampleDuration() }.unwrap_or(0);
        duration = duration.max(from_hns(timestamp.saturating_add(length.max(0))));
    }
    keyframes.sort_unstable();
    keyframes.dedup();
    Ok(ClipTiming {
        duration,
        keyframes,
    })
}

#[cfg(target_os = "windows")]
fn copy(plan: &CutPlan) -> Result<(), TrimError> {
    let reader = open(&plan.source, None)?;
    let streams = Streams::discover(&reader)?;
    streams.select(&reader)?;
    let video_type = unsafe { reader.GetCurrentMediaType(streams.video) }.map_err(backend_error)?;
    let audio_type = match streams.audio {
        Some(audio) => Some(unsafe { reader.GetCurrentMediaType(audio) }.map_err(backend_error)?),
        None => None,
    };
    seek(&reader, plan.start)?;
    transfer(
        &reader,
        &streams,
        plan,
        &video_type,
        audio_type.as_ref(),
        None,
    )
}

#[cfg(target_os = "windows")]
fn reencode(plan: &CutPlan) -> Result<(), TrimError> {
    use windows::Win32::Media::MediaFoundation::{
        MF_MT_AVG_BITRATE, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE,
        MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING,
        MFAudioFormat_PCM, MFVideoFormat_NV12,
    };

    let reader = open(
        &plan.source,
        Some(&[
            MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING,
            MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS,
        ]),
    )?;
    let streams = Streams::discover(&reader)?;
    streams.select(&reader)?;

    let native = unsafe { reader.GetNativeMediaType(streams.video, 0) }.map_err(backend_error)?;
    let frame_size = unsafe { native.GetUINT64(&MF_MT_FRAME_SIZE) }.map_err(backend_error)?;
    let frame_rate = unsafe { native.GetUINT64(&MF_MT_FRAME_RATE) }.unwrap_or(pack(60, 1));
    let bitrate = unsafe { native.GetUINT32(&MF_MT_AVG_BITRATE) }
        .ok()
        .filter(|value| *value > 0)
        .unwrap_or_else(|| default_bitrate(frame_size, frame_rate));

    unsafe { reader.SetCurrentMediaType(streams.video, None, &video_format(MFVideoFormat_NV12)?) }
        .map_err(backend_error)?;
    let decoded_video =
        unsafe { reader.GetCurrentMediaType(streams.video) }.map_err(backend_error)?;
    let encoded_video = encoded_video_type(&native, frame_size, frame_rate, bitrate)?;

    let audio = match streams.audio {
        Some(index) => {
            unsafe { reader.SetCurrentMediaType(index, None, &audio_format(MFAudioFormat_PCM)?) }
                .map_err(backend_error)?;
            let decoded = unsafe { reader.GetCurrentMediaType(index) }.map_err(backend_error)?;
            let encoded = encoded_audio_type(&decoded)?;
            Some((decoded, encoded))
        }
        None => None,
    };

    seek(&reader, plan.start)?;
    let (decoded_audio, encoded_audio) = match audio {
        Some((decoded, encoded)) => (Some(decoded), Some(encoded)),
        None => (None, None),
    };
    transfer(
        &reader,
        &streams,
        plan,
        &encoded_video,
        encoded_audio.as_ref(),
        Some(Inputs {
            video: &decoded_video,
            audio: decoded_audio.as_ref(),
        }),
    )
}

#[cfg(target_os = "windows")]
struct Inputs<'a> {
    video: &'a IMFMediaType,
    audio: Option<&'a IMFMediaType>,
}

#[cfg(target_os = "windows")]
fn transfer(
    reader: &IMFSourceReader,
    streams: &Streams,
    plan: &CutPlan,
    video_type: &IMFMediaType,
    audio_type: Option<&IMFMediaType>,
    inputs: Option<Inputs<'_>>,
) -> Result<(), TrimError> {
    use windows::Win32::Media::MediaFoundation::{
        IMFAttributes, MF_SOURCE_READER_ALL_STREAMS, MF_SOURCE_READERF_ENDOFSTREAM,
    };

    let writer = sink(&plan.destination)?;
    let video_stream = unsafe { writer.AddStream(video_type) }.map_err(backend_error)?;
    let video_input = inputs
        .as_ref()
        .map(|inputs| inputs.video)
        .unwrap_or(video_type);
    unsafe { writer.SetInputMediaType(video_stream, video_input, None::<&IMFAttributes>) }
        .map_err(backend_error)?;
    let audio_stream = match audio_type {
        Some(audio_type) => {
            let stream = unsafe { writer.AddStream(audio_type) }.map_err(backend_error)?;
            let input = inputs
                .as_ref()
                .and_then(|inputs| inputs.audio)
                .unwrap_or(audio_type);
            unsafe { writer.SetInputMediaType(stream, input, None::<&IMFAttributes>) }
                .map_err(backend_error)?;
            Some(stream)
        }
        None => None,
    };
    unsafe { writer.BeginWriting() }.map_err(backend_error)?;

    let start = to_hns(plan.start);
    let end = to_hns(plan.end);
    let skip_before = inputs.is_some().then_some(start);
    let mut base: Option<i64> = skip_before;
    let mut video_done = false;
    let mut audio_done = audio_stream.is_none();
    let mut written = 0_u64;

    while !(video_done && audio_done) {
        let mut index = 0_u32;
        let mut flags = 0_u32;
        let mut timestamp = 0_i64;
        let mut sample: Option<IMFSample> = None;
        unsafe {
            reader.ReadSample(
                MF_SOURCE_READER_ALL_STREAMS.0 as u32,
                0,
                Some(&mut index),
                Some(&mut flags),
                Some(&mut timestamp),
                Some(&mut sample),
            )
        }
        .map_err(backend_error)?;
        if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
            break;
        }
        let Some(sample) = sample else {
            continue;
        };
        let stream = if index == streams.video {
            video_stream
        } else if Some(index) == streams.audio {
            let Some(stream) = audio_stream else {
                continue;
            };
            stream
        } else {
            continue;
        };
        if timestamp >= end {
            if stream == video_stream {
                video_done = true;
            } else {
                audio_done = true;
            }
            continue;
        }
        if skip_before.is_some_and(|start| timestamp < start) {
            continue;
        }
        let base = *base.get_or_insert(timestamp);
        unsafe { sample.SetSampleTime(timestamp.saturating_sub(base).max(0)) }
            .map_err(backend_error)?;
        unsafe { writer.WriteSample(stream, &sample) }.map_err(backend_error)?;
        written += 1;
    }

    if written == 0 {
        let _ = unsafe { writer.Finalize() };
        return Err(TrimError::Backend(
            "the selected span holds no samples".into(),
        ));
    }
    unsafe { writer.Finalize() }.map_err(backend_error)
}

#[cfg(target_os = "windows")]
fn read(reader: &IMFSourceReader, stream: u32) -> Result<Option<(u32, IMFSample, i64)>, TrimError> {
    use windows::Win32::Media::MediaFoundation::MF_SOURCE_READERF_ENDOFSTREAM;

    loop {
        let mut index = 0_u32;
        let mut flags = 0_u32;
        let mut timestamp = 0_i64;
        let mut sample: Option<IMFSample> = None;
        unsafe {
            reader.ReadSample(
                stream,
                0,
                Some(&mut index),
                Some(&mut flags),
                Some(&mut timestamp),
                Some(&mut sample),
            )
        }
        .map_err(backend_error)?;
        if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
            return Ok(None);
        }
        if let Some(sample) = sample {
            return Ok(Some((index, sample, timestamp)));
        }
    }
}

#[cfg(target_os = "windows")]
fn open(
    source: &Path,
    attributes: Option<&[windows::core::GUID]>,
) -> Result<IMFSourceReader, TrimError> {
    use windows::Win32::Media::MediaFoundation::{
        IMFAttributes, MFCreateAttributes, MFCreateSourceReaderFromURL,
    };
    use windows::core::PCWSTR;

    let url = wide(source);
    let attributes = match attributes {
        Some(flags) => {
            let mut created = None;
            unsafe { MFCreateAttributes(&mut created, flags.len() as u32) }
                .map_err(backend_error)?;
            let created = created.ok_or_else(|| {
                TrimError::Backend("Media Foundation returned no reader attributes".into())
            })?;
            for flag in flags {
                unsafe { created.SetUINT32(flag, 1) }.map_err(backend_error)?;
            }
            Some(created)
        }
        None => None,
    };
    unsafe {
        MFCreateSourceReaderFromURL(
            PCWSTR(url.as_ptr()),
            attributes.as_ref().map(|attributes| {
                let attributes: &IMFAttributes = attributes;
                attributes
            }),
        )
    }
    .map_err(|error| {
        TrimError::Backend(format!(
            "{} cannot be opened for cutting: {error}",
            source.display()
        ))
    })
}

#[cfg(target_os = "windows")]
fn sink(destination: &Path) -> Result<IMFSinkWriter, TrimError> {
    use windows::Win32::Media::MediaFoundation::{
        IMFByteStream, MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, MF_TRANSCODE_CONTAINERTYPE,
        MFCreateAttributes, MFCreateSinkWriterFromURL, MFTranscodeContainerType_MPEG4,
    };
    use windows::core::PCWSTR;

    let mut attributes = None;
    unsafe { MFCreateAttributes(&mut attributes, 2) }.map_err(backend_error)?;
    let attributes = attributes
        .ok_or_else(|| TrimError::Backend("Media Foundation returned no sink attributes".into()))?;
    unsafe { attributes.SetGUID(&MF_TRANSCODE_CONTAINERTYPE, &MFTranscodeContainerType_MPEG4) }
        .map_err(backend_error)?;
    unsafe { attributes.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1) }
        .map_err(backend_error)?;
    let url = wide(destination);
    unsafe {
        MFCreateSinkWriterFromURL(
            PCWSTR(url.as_ptr()),
            None::<&IMFByteStream>,
            Some(&attributes),
        )
    }
    .map_err(backend_error)
}

#[cfg(target_os = "windows")]
fn seek(reader: &IMFSourceReader, position: Duration) -> Result<(), TrimError> {
    use std::mem::ManuallyDrop;

    use windows::Win32::System::Com::StructuredStorage::{
        PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
    };
    use windows::Win32::System::Variant::VT_I8;

    let position = PROPVARIANT {
        Anonymous: PROPVARIANT_0 {
            Anonymous: ManuallyDrop::new(PROPVARIANT_0_0 {
                vt: VT_I8,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: PROPVARIANT_0_0_0 {
                    hVal: to_hns(position),
                },
            }),
        },
    };
    unsafe { reader.SetCurrentPosition(&windows::core::GUID::from_u128(0), &position) }
        .map_err(backend_error)
}

#[cfg(target_os = "windows")]
fn video_format(subtype: windows::core::GUID) -> Result<IMFMediaType, TrimError> {
    use windows::Win32::Media::MediaFoundation::{
        MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MFCreateMediaType, MFMediaType_Video,
    };

    let media_type = unsafe { MFCreateMediaType() }.map_err(backend_error)?;
    unsafe { media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video) }.map_err(backend_error)?;
    unsafe { media_type.SetGUID(&MF_MT_SUBTYPE, &subtype) }.map_err(backend_error)?;
    Ok(media_type)
}

#[cfg(target_os = "windows")]
fn audio_format(subtype: windows::core::GUID) -> Result<IMFMediaType, TrimError> {
    use windows::Win32::Media::MediaFoundation::{
        MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MFCreateMediaType, MFMediaType_Audio,
    };

    let media_type = unsafe { MFCreateMediaType() }.map_err(backend_error)?;
    unsafe { media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio) }.map_err(backend_error)?;
    unsafe { media_type.SetGUID(&MF_MT_SUBTYPE, &subtype) }.map_err(backend_error)?;
    Ok(media_type)
}

#[cfg(target_os = "windows")]
fn encoded_video_type(
    native: &IMFMediaType,
    frame_size: u64,
    frame_rate: u64,
    bitrate: u32,
) -> Result<IMFMediaType, TrimError> {
    use windows::Win32::Media::MediaFoundation::{
        MF_MT_AVG_BITRATE, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE,
        MF_MT_MAX_KEYFRAME_SPACING, MF_MT_PIXEL_ASPECT_RATIO, MF_MT_SUBTYPE, MFVideoFormat_H264,
        MFVideoInterlace_Progressive,
    };

    let subtype = unsafe { native.GetGUID(&MF_MT_SUBTYPE) }.unwrap_or(MFVideoFormat_H264);
    let media_type = video_format(subtype)?;
    let configure = || -> windows::core::Result<()> {
        unsafe {
            media_type.SetUINT64(&MF_MT_FRAME_SIZE, frame_size)?;
            media_type.SetUINT64(&MF_MT_FRAME_RATE, frame_rate)?;
            media_type.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1))?;
            media_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            media_type.SetUINT32(&MF_MT_AVG_BITRATE, bitrate)?;
            media_type.SetUINT32(&MF_MT_MAX_KEYFRAME_SPACING, keyframe_spacing(frame_rate))?;
        }
        Ok(())
    };
    configure().map_err(backend_error)?;
    Ok(media_type)
}

#[cfg(target_os = "windows")]
fn encoded_audio_type(decoded: &IMFMediaType) -> Result<IMFMediaType, TrimError> {
    use windows::Win32::Media::MediaFoundation::{
        MF_MT_AUDIO_AVG_BYTES_PER_SECOND, MF_MT_AUDIO_BITS_PER_SAMPLE, MF_MT_AUDIO_NUM_CHANNELS,
        MF_MT_AUDIO_SAMPLES_PER_SECOND, MFAudioFormat_AAC,
    };

    let sample_rate =
        unsafe { decoded.GetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND) }.unwrap_or(48_000);
    let channels = unsafe { decoded.GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS) }.unwrap_or(2);
    let media_type = audio_format(MFAudioFormat_AAC)?;
    let configure = || -> windows::core::Result<()> {
        unsafe {
            media_type.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
            media_type.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, sample_rate)?;
            media_type.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, channels.min(2))?;
            media_type.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, AAC_BYTES_PER_SECOND)?;
        }
        Ok(())
    };
    configure().map_err(backend_error)?;
    Ok(media_type)
}

#[cfg(target_os = "windows")]
fn wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;

    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

#[cfg(target_os = "windows")]
fn backend_error(error: windows::core::Error) -> TrimError {
    TrimError::Backend(error.to_string())
}

#[cfg(any(target_os = "windows", test))]
fn to_hns(value: Duration) -> i64 {
    i64::try_from(value.as_nanos() / 100).unwrap_or(i64::MAX)
}

#[cfg(any(target_os = "windows", test))]
fn from_hns(value: i64) -> Duration {
    Duration::from_nanos(u64::try_from(value).unwrap_or(0).saturating_mul(100))
}

#[cfg(any(target_os = "windows", test))]
fn pack(high: u32, low: u32) -> u64 {
    (u64::from(high) << 32) | u64::from(low)
}

#[cfg(any(target_os = "windows", test))]
fn unpack(value: u64) -> (u32, u32) {
    ((value >> 32) as u32, value as u32)
}

#[cfg(any(target_os = "windows", test))]
fn frames_per_second(frame_rate: u64) -> u32 {
    let (numerator, denominator) = unpack(frame_rate);
    numerator
        .checked_div(denominator)
        .map_or(60, |value| value.clamp(1, 240))
}

#[cfg(any(target_os = "windows", test))]
fn keyframe_spacing(frame_rate: u64) -> u32 {
    frames_per_second(frame_rate).saturating_mul(2)
}

#[cfg(any(target_os = "windows", test))]
fn default_bitrate(frame_size: u64, frame_rate: u64) -> u32 {
    let (width, height) = unpack(frame_size);
    let pixels_per_second =
        u64::from(width) * u64::from(height) * u64::from(frames_per_second(frame_rate));
    u32::try_from(pixels_per_second.saturating_mul(120) / 1_000_000)
        .unwrap_or(80_000_000)
        .clamp(2_500_000, 80_000_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hundred_nanosecond_units_round_trip() {
        assert_eq!(to_hns(Duration::from_millis(1_500)), 15_000_000);
        assert_eq!(from_hns(15_000_000), Duration::from_millis(1_500));
        assert_eq!(from_hns(-1), Duration::ZERO);
    }

    #[test]
    fn packed_pairs_split_into_their_halves() {
        assert_eq!(unpack(pack(1_920, 1_080)), (1_920, 1_080));
        assert_eq!(unpack(pack(60, 1)), (60, 1));
    }

    #[test]
    fn keyframes_stay_two_seconds_apart() {
        assert_eq!(keyframe_spacing(pack(60, 1)), 120);
        assert_eq!(keyframe_spacing(pack(30_000, 1_001)), 58);
        assert_eq!(keyframe_spacing(pack(60, 0)), 120);
    }

    #[test]
    fn a_clip_without_a_stated_bitrate_still_gets_a_sane_one() {
        let bitrate = default_bitrate(pack(1_920, 1_080), pack(60, 1));

        assert!(bitrate >= 2_500_000);
        assert!(bitrate <= 80_000_000);
        assert!(default_bitrate(pack(640, 360), pack(30, 1)) >= 2_500_000);
    }
}
