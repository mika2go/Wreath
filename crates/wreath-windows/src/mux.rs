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
    let has_desktop_audio = audio_media_type.is_some()
        && packets
            .iter()
            .any(|packet| packet.track == TrackKind::DesktopAudio);
    let has_microphone_audio = audio_media_type.is_some()
        && packets
            .iter()
            .any(|packet| packet.track == TrackKind::MicrophoneAudio);
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
    let desktop_audio_stream = if has_desktop_audio {
        let media_type = audio_media_type.expect("audio media type checked");
        let stream = unsafe { writer.AddStream(media_type) }.map_err(initialization_error)?;
        unsafe { writer.SetInputMediaType(stream, media_type, None::<&IMFAttributes>) }
            .map_err(initialization_error)?;
        Some(stream)
    } else {
        None
    };
    let microphone_audio_stream = if has_microphone_audio {
        let media_type = audio_media_type.expect("audio media type checked");
        let stream = unsafe { writer.AddStream(media_type) }.map_err(initialization_error)?;
        unsafe { writer.SetInputMediaType(stream, media_type, None::<&IMFAttributes>) }
            .map_err(initialization_error)?;
        Some(stream)
    } else {
        None
    };
    unsafe { writer.BeginWriting() }.map_err(initialization_error)?;

    let durations = presented_durations(&packets);
    for (packet, duration) in packets.iter().zip(durations) {
        let stream = match packet.track {
            TrackKind::Video => video_stream,
            TrackKind::Audio => {
                let Some(stream) = audio_stream else {
                    continue;
                };
                stream
            }
            TrackKind::DesktopAudio => {
                let Some(stream) = desktop_audio_stream else {
                    continue;
                };
                stream
            }
            TrackKind::MicrophoneAudio => {
                let Some(stream) = microphone_audio_stream else {
                    continue;
                };
                stream
            }
        };
        let sample = packet_to_sample(packet, first_timestamp, duration)?;
        unsafe { writer.WriteSample(stream, &sample) }.map_err(initialization_error)?;
    }
    let span = packets
        .last()
        .map(|packet| packet.end_timestamp().saturating_sub(first_timestamp))
        .unwrap_or_default();
    let video_frames = packets
        .iter()
        .filter(|packet| packet.track == TrackKind::Video)
        .count();
    wreath_core::diagnostic!(
        "Wreath clip: {} s written from {} packets, {video_frames} video frames",
        span.as_secs_f32(),
        packets.len()
    );
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

/// How long each sample is actually on screen, taken from the gap to the next
/// sample on its own track.
///
/// Every encoded video packet claims the nominal frame duration, but Windows
/// Graphics Capture only delivers a frame when the picture changes and the
/// pipeline skips frames the encoder is not ready for. The muxer builds the
/// track from these durations, so a clip whose frames were sparse came out
/// proportionally shorter than the span it covered - a 30 second replay saved
/// as 24. Stretching a sample until its successor arrives holds the picture,
/// which is what actually happened, and keeps the track as long as the capture.
#[cfg(any(target_os = "windows", test))]
fn presented_durations(packets: &[&EncodedPacket]) -> Vec<std::time::Duration> {
    let mut durations = vec![std::time::Duration::ZERO; packets.len()];
    let mut next_video: Option<std::time::Duration> = None;
    let mut next_audio: Option<std::time::Duration> = None;
    let mut next_desktop_audio: Option<std::time::Duration> = None;
    let mut next_microphone_audio: Option<std::time::Duration> = None;
    for (index, packet) in packets.iter().enumerate().rev() {
        let next = match packet.track {
            TrackKind::Video => next_video,
            TrackKind::Audio => next_audio,
            TrackKind::DesktopAudio => next_desktop_audio,
            TrackKind::MicrophoneAudio => next_microphone_audio,
        };
        durations[index] = next
            .and_then(|next| next.checked_sub(packet.timestamp))
            .filter(|gap| !gap.is_zero())
            .unwrap_or(packet.duration);
        match packet.track {
            TrackKind::Video => next_video = Some(packet.timestamp),
            TrackKind::Audio => next_audio = Some(packet.timestamp),
            TrackKind::DesktopAudio => next_desktop_audio = Some(packet.timestamp),
            TrackKind::MicrophoneAudio => next_microphone_audio = Some(packet.timestamp),
        }
    }
    durations
}

#[cfg(any(target_os = "windows", test))]
fn track_order(track: TrackKind) -> u8 {
    match track {
        TrackKind::Video => 0,
        TrackKind::Audio => 1,
        TrackKind::DesktopAudio => 2,
        TrackKind::MicrophoneAudio => 3,
    }
}

#[cfg(target_os = "windows")]
fn packet_to_sample(
    packet: &EncodedPacket,
    first_timestamp: std::time::Duration,
    duration: std::time::Duration,
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
            sample.SetSampleDuration(duration_to_hns(duration))?;
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

    /// A 30 second replay saved as 24 because sparse frames each claimed the
    /// nominal frame duration, so the track was as long as the frames it had
    /// rather than as long as the capture it covered.
    #[test]
    fn a_held_frame_stretches_until_its_successor_instead_of_shortening_the_clip() {
        let packets = [
            packet(TrackKind::Video, 0),
            packet(TrackKind::Video, 250),
            packet(TrackKind::Video, 500),
        ];
        let ordered = ordered_packets(packets.iter());

        let durations = presented_durations(&ordered);

        // Each frame claims 10 ms, but a quarter of a second passed between
        // them; the covered span has to survive into the track.
        assert_eq!(durations[0], std::time::Duration::from_millis(250));
        assert_eq!(durations[1], std::time::Duration::from_millis(250));
        // The last sample has no successor and keeps its own duration.
        assert_eq!(durations[2], std::time::Duration::from_millis(10));
        assert_eq!(
            durations.iter().sum::<std::time::Duration>(),
            std::time::Duration::from_millis(510)
        );
    }

    #[test]
    fn each_track_is_stretched_against_its_own_successor() {
        let packets = [
            packet(TrackKind::Video, 0),
            packet(TrackKind::Audio, 20),
            packet(TrackKind::Video, 100),
            packet(TrackKind::Audio, 120),
        ];
        let ordered = ordered_packets(packets.iter());
        let durations = presented_durations(&ordered);

        for (packet, duration) in ordered.iter().zip(&durations) {
            if packet.timestamp < std::time::Duration::from_millis(100) {
                assert_eq!(*duration, std::time::Duration::from_millis(100));
            } else {
                assert_eq!(*duration, std::time::Duration::from_millis(10));
            }
        }
    }

    #[test]
    fn separate_audio_tracks_keep_independent_timelines_and_stable_order() {
        let packets = [
            packet(TrackKind::MicrophoneAudio, 100),
            packet(TrackKind::DesktopAudio, 0),
            packet(TrackKind::Audio, 100),
            packet(TrackKind::Video, 100),
            packet(TrackKind::MicrophoneAudio, 0),
            packet(TrackKind::DesktopAudio, 100),
            packet(TrackKind::Audio, 0),
            packet(TrackKind::Video, 0),
        ];
        let ordered = ordered_packets(packets.iter());
        let durations = presented_durations(&ordered);

        assert_eq!(
            ordered
                .iter()
                .take(4)
                .map(|packet| packet.track)
                .collect::<Vec<_>>(),
            vec![
                TrackKind::Video,
                TrackKind::Audio,
                TrackKind::DesktopAudio,
                TrackKind::MicrophoneAudio,
            ]
        );
        assert!(
            durations
                .iter()
                .take(4)
                .all(|duration| *duration == std::time::Duration::from_millis(100))
        );
        assert!(
            durations
                .iter()
                .skip(4)
                .all(|duration| *duration == std::time::Duration::from_millis(10))
        );
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
