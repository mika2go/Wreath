use std::path::{Path, PathBuf};

#[cfg(any(target_os = "windows", test))]
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
pub fn write_mp4<'a>(
    path: &Path,
    video_media_type: &windows::Win32::Media::MediaFoundation::IMFMediaType,
    audio_media_type: Option<&windows::Win32::Media::MediaFoundation::IMFMediaType>,
    packets: impl Iterator<Item = &'a EncodedPacket>,
) -> Result<(), VideoError> {
    use std::os::windows::ffi::OsStrExt;

    use windows::Win32::Media::MediaFoundation::{
        IMFAttributes, IMFByteStream, MF_TRANSCODE_CONTAINERTYPE, MFCreateAttributes,
        MFCreateSinkWriterFromURL, MFTranscodeContainerType_MPEG4,
    };
    use windows::core::PCWSTR;

    let packets = ordered_packets(packets);
    if !packets
        .iter()
        .any(|packet| packet.track == TrackKind::Video)
    {
        return Err(VideoError::Initialization(
            "replay buffer has no video packets".into(),
        ));
    }
    // Rebase on the earliest packet of either track. Using the first video
    // packet instead collapsed every audio packet ahead of it onto timestamp
    // zero, because the subtraction saturates.
    let first_timestamp = packets
        .first()
        .map(|packet| packet.timestamp)
        .unwrap_or_default();
    let has_audio = audio_media_type.is_some()
        && packets
            .iter()
            .any(|packet| packet.track == TrackKind::Audio);
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
    let video_stream =
        unsafe { writer.AddStream(video_media_type) }.map_err(initialization_error)?;
    unsafe { writer.SetInputMediaType(video_stream, video_media_type, None::<&IMFAttributes>) }
        .map_err(initialization_error)?;
    let audio_stream = if has_audio {
        let media_type = audio_media_type.expect("audio media type checked");
        let stream = unsafe { writer.AddStream(media_type) }.map_err(initialization_error)?;
        unsafe { writer.SetInputMediaType(stream, media_type, None::<&IMFAttributes>) }
            .map_err(initialization_error)?;
        Some(stream)
    } else {
        None
    };
    unsafe { writer.BeginWriting() }.map_err(initialization_error)?;

    for packet in packets {
        let stream = match packet.track {
            TrackKind::Video => video_stream,
            TrackKind::Audio => {
                let Some(stream) = audio_stream else {
                    continue;
                };
                stream
            }
        };
        let sample = packet_to_sample(packet, first_timestamp)?;
        unsafe { writer.WriteSample(stream, &sample) }.map_err(initialization_error)?;
    }
    unsafe { writer.Finalize() }.map_err(initialization_error)
}

#[cfg(any(target_os = "windows", test))]
fn ordered_packets<'a>(packets: impl Iterator<Item = &'a EncodedPacket>) -> Vec<&'a EncodedPacket> {
    let mut packets = packets.collect::<Vec<_>>();
    packets.sort_by(|left, right| {
        left.timestamp
            .cmp(&right.timestamp)
            .then_with(|| track_order(left.track).cmp(&track_order(right.track)))
    });
    packets
}

#[cfg(any(target_os = "windows", test))]
fn track_order(track: TrackKind) -> u8 {
    match track {
        TrackKind::Video => 0,
        TrackKind::Audio => 1,
    }
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

    fn packet(track: TrackKind, milliseconds: u64) -> EncodedPacket {
        EncodedPacket {
            track,
            timestamp: std::time::Duration::from_millis(milliseconds),
            duration: std::time::Duration::from_millis(10),
            keyframe: track == TrackKind::Video,
            payload: vec![1].into(),
        }
    }

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

    #[test]
    fn mux_order_is_chronological_and_video_wins_ties() {
        let audio_late = packet(TrackKind::Audio, 20);
        let audio_tied = packet(TrackKind::Audio, 10);
        let video_tied = packet(TrackKind::Video, 10);
        let ordered = ordered_packets([&audio_late, &audio_tied, &video_tied].into_iter());

        assert_eq!(ordered[0].track, TrackKind::Video);
        assert_eq!(ordered[1].track, TrackKind::Audio);
        assert_eq!(ordered[2].timestamp, std::time::Duration::from_millis(20));
    }
}
