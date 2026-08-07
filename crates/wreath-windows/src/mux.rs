use std::path::{Path, PathBuf};

#[cfg(target_os = "windows")]
use wreath_core::replay_buffer::{EncodedPacket, TrackKind};

#[cfg(target_os = "windows")]
use crate::video::VideoError;

pub fn unique_clip_path(directory: &Path, unix_milliseconds: u128) -> PathBuf {
    let first = directory.join(format!("wreath-{unix_milliseconds}.mp4"));
    if !first.exists() {
        return first;
    }
    for suffix in 2_u32.. {
        let candidate = directory.join(format!("wreath-{unix_milliseconds}-{suffix}.mp4"));
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!("the clip suffix space is inexhaustible")
}

#[cfg(target_os = "windows")]
pub fn write_video_mp4<'a>(
    path: &Path,
    media_type: &windows::Win32::Media::MediaFoundation::IMFMediaType,
    packets: impl Iterator<Item = &'a EncodedPacket>,
) -> Result<(), VideoError> {
    use std::os::windows::ffi::OsStrExt;

    use windows::Win32::Media::MediaFoundation::{
        IMFAttributes, IMFByteStream, MF_TRANSCODE_CONTAINERTYPE, MFCreateAttributes,
        MFCreateSinkWriterFromURL, MFTranscodeContainerType_MPEG4,
    };
    use windows::core::PCWSTR;

    let mut packets = packets
        .filter(|packet| packet.track == TrackKind::Video)
        .peekable();
    let first_timestamp = packets
        .peek()
        .ok_or_else(|| VideoError::Initialization("replay buffer has no video packets".into()))?
        .timestamp;
    let mut attributes = None;
    unsafe { MFCreateAttributes(&mut attributes, 1) }.map_err(initialization_error)?;
    let attributes = attributes.ok_or_else(|| {
        VideoError::Initialization("Media Foundation returned no sink attributes".into())
    })?;
    unsafe { attributes.SetGUID(&MF_TRANSCODE_CONTAINERTYPE, &MFTranscodeContainerType_MPEG4) }
        .map_err(initialization_error)?;
    let wide_path = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let writer = unsafe {
        MFCreateSinkWriterFromURL(
            PCWSTR(wide_path.as_ptr()),
            None::<&IMFByteStream>,
            Some(&attributes),
        )
    }
    .map_err(initialization_error)?;
    let stream = unsafe { writer.AddStream(media_type) }.map_err(initialization_error)?;
    unsafe { writer.SetInputMediaType(stream, media_type, None::<&IMFAttributes>) }
        .map_err(initialization_error)?;
    unsafe { writer.BeginWriting() }.map_err(initialization_error)?;

    for packet in packets {
        let sample = packet_to_sample(packet, first_timestamp)?;
        unsafe { writer.WriteSample(stream, &sample) }.map_err(initialization_error)?;
    }
    unsafe { writer.Finalize() }.map_err(initialization_error)
}

#[cfg(target_os = "windows")]
fn packet_to_sample(
    packet: &EncodedPacket,
    first_timestamp: std::time::Duration,
) -> Result<windows::Win32::Media::MediaFoundation::IMFSample, VideoError> {
    use windows::Win32::Media::MediaFoundation::{
        MFCreateMemoryBuffer, MFCreateSample, MFSampleExtension_CleanPoint,
    };

    let payload_length = u32::try_from(packet.payload.len()).map_err(|_| {
        VideoError::Initialization("encoded packet is too large for Media Foundation".into())
    })?;
    if payload_length == 0 {
        return Err(VideoError::Initialization(
            "hardware encoder returned an empty packet".into(),
        ));
    }
    let buffer = unsafe { MFCreateMemoryBuffer(payload_length) }.map_err(initialization_error)?;
    let mut destination = std::ptr::null_mut();
    unsafe { buffer.Lock(&mut destination, None, None) }.map_err(initialization_error)?;
    if destination.is_null() {
        let _ = unsafe { buffer.Unlock() };
        return Err(VideoError::Initialization(
            "Media Foundation returned a null sample buffer".into(),
        ));
    }
    unsafe {
        std::ptr::copy_nonoverlapping(packet.payload.as_ptr(), destination, packet.payload.len())
    };
    unsafe { buffer.Unlock() }.map_err(initialization_error)?;
    unsafe { buffer.SetCurrentLength(payload_length) }.map_err(initialization_error)?;

    let sample = unsafe { MFCreateSample() }.map_err(initialization_error)?;
    let configure = || -> windows::core::Result<()> {
        unsafe {
            sample.AddBuffer(&buffer)?;
            sample.SetSampleTime(duration_to_hns(
                packet.timestamp.saturating_sub(first_timestamp),
            ))?;
            sample.SetSampleDuration(duration_to_hns(packet.duration))?;
            if packet.keyframe {
                sample.SetUINT32(&MFSampleExtension_CleanPoint, 1)?;
            }
        }
        Ok(())
    };
    configure().map_err(initialization_error)?;
    Ok(sample)
}

#[cfg(target_os = "windows")]
fn duration_to_hns(duration: std::time::Duration) -> i64 {
    i64::try_from(duration.as_nanos() / 100).unwrap_or(i64::MAX)
}

#[cfg(target_os = "windows")]
fn initialization_error(error: windows::core::Error) -> VideoError {
    VideoError::Initialization(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn clip_paths_do_not_overwrite_same_millisecond_saves() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("wreath-mux-{unique}"));
        fs::create_dir_all(&directory).unwrap();

        let first = unique_clip_path(&directory, 1234);
        fs::write(&first, b"existing").unwrap();
        let second = unique_clip_path(&directory, 1234);

        assert_eq!(first.file_name().unwrap(), "wreath-1234.mp4");
        assert_eq!(second.file_name().unwrap(), "wreath-1234-2.mp4");
        fs::remove_dir_all(directory).unwrap();
    }
}
