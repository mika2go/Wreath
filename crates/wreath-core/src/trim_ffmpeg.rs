use std::io;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use crate::trim::{ClipTiming, Container, CutPlan, TrimBackend, TrimError};

const MAX_PACKETS: usize = 400_000;

#[derive(Debug, Clone, Copy, Default)]
pub struct FfmpegTrimmer;

impl TrimBackend for FfmpegTrimmer {
    fn timing(&self, source: &Path) -> Result<ClipTiming, TrimError> {
        Ok(ClipTiming {
            duration: probe_duration(source)?,
            keyframes: probe_keyframes(source)?,
        })
    }

    fn cut(&self, plan: &CutPlan) -> Result<(), TrimError> {
        let mut command = Command::new("ffmpeg");
        command.args(["-nostdin", "-loglevel", "error", "-y", "-ss"]);
        command.arg(seconds(plan.start));
        command.arg("-i").arg(&plan.source);
        command.arg("-t").arg(seconds(plan.length()));
        command.args(["-map", "0:v:0", "-map", "0:a:0?"]);
        if plan.reencode {
            command.args(video_encoder(
                &probe_video_codec(&plan.source),
                plan.container,
            ));
            command.args(["-pix_fmt", "yuv420p"]);
            command.args(audio_encoder(plan.container));
        } else {
            command.args(["-c:v", "copy", "-c:a", "copy"]);
        }
        command.args(["-avoid_negative_ts", "make_zero"]);
        if plan.container == Container::Mp4 {
            command.args(["-movflags", "+faststart"]);
        }
        command.args(["-f", plan.container.muxer()]);
        command.arg(&plan.destination);

        let output = run(command)?;
        if output.status.success() {
            return Ok(());
        }
        Err(TrimError::Backend(last_lines(&output.stderr)))
    }
}

fn audio_encoder(container: Container) -> [&'static str; 4] {
    match container {
        Container::WebM => ["-c:a", "libopus", "-b:a", "160k"],
        _ => ["-c:a", "aac", "-b:a", "160k"],
    }
}

fn video_encoder(codec: &str, container: Container) -> Vec<&'static str> {
    if container == Container::WebM {
        return vec![
            "-c:v",
            "libvpx-vp9",
            "-b:v",
            "0",
            "-crf",
            "31",
            "-deadline",
            "good",
            "-cpu-used",
            "4",
        ];
    }
    match codec {
        "hevc" | "h265" => vec!["-c:v", "libx265", "-preset", "veryfast", "-crf", "24"],
        "av1" => vec!["-c:v", "libsvtav1", "-preset", "8", "-crf", "32"],
        "vp9" => vec![
            "-c:v",
            "libvpx-vp9",
            "-b:v",
            "0",
            "-crf",
            "31",
            "-deadline",
            "good",
            "-cpu-used",
            "4",
        ],
        _ => vec!["-c:v", "libx264", "-preset", "veryfast", "-crf", "20"],
    }
}

fn probe_duration(source: &Path) -> Result<Duration, TrimError> {
    let mut command = Command::new("ffprobe");
    command.args([
        "-v",
        "error",
        "-show_entries",
        "format=duration",
        "-of",
        "default=noprint_wrappers=1:nokey=1",
    ]);
    command.arg(source);
    let output = run(command)?;
    if !output.status.success() {
        return Err(TrimError::Backend(last_lines(&output.stderr)));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|seconds| seconds.is_finite() && *seconds > 0.0)
        .map(Duration::from_secs_f64)
        .unwrap_or_default())
}

fn probe_video_codec(source: &Path) -> String {
    let mut command = Command::new("ffprobe");
    command.args([
        "-v",
        "error",
        "-select_streams",
        "v:0",
        "-show_entries",
        "stream=codec_name",
        "-of",
        "default=noprint_wrappers=1:nokey=1",
    ]);
    command.arg(source);
    run(command)
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_default()
}

fn probe_keyframes(source: &Path) -> Result<Vec<Duration>, TrimError> {
    let mut command = Command::new("ffprobe");
    command.args([
        "-v",
        "error",
        "-select_streams",
        "v:0",
        "-show_entries",
        "packet=pts_time,flags",
        "-of",
        "csv=p=0",
    ]);
    command.arg(source);
    let output = run(command)?;
    if !output.status.success() {
        return Err(TrimError::Backend(last_lines(&output.stderr)));
    }
    Ok(parse_keyframes(&String::from_utf8_lossy(&output.stdout)))
}

fn parse_keyframes(listing: &str) -> Vec<Duration> {
    let mut keyframes = Vec::new();
    for line in listing.lines().take(MAX_PACKETS) {
        let mut fields = line.split(',');
        let Some(timestamp) = fields.next() else {
            continue;
        };
        let Some(flags) = fields.next() else {
            continue;
        };
        if !flags.contains('K') {
            continue;
        }
        let Ok(seconds) = timestamp.trim().parse::<f64>() else {
            continue;
        };
        if seconds.is_finite() && seconds >= 0.0 {
            keyframes.push(Duration::from_secs_f64(seconds));
        }
    }
    keyframes.sort_unstable();
    keyframes.dedup();
    keyframes
}

fn run(mut command: Command) -> Result<Output, TrimError> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => TrimError::Unsupported(
                "cutting needs ffmpeg; on Arch or CachyOS run `sudo pacman -S ffmpeg`".into(),
            ),
            _ => TrimError::Io(error),
        })
}

fn seconds(value: Duration) -> String {
    format!("{:.6}", value.as_secs_f64())
}

fn last_lines(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let message = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .rev()
        .take(3)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("; ");
    if message.is_empty() {
        "ffmpeg reported no reason".into()
    } else {
        message
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyframe_packets_are_read_and_others_ignored() {
        let listing = "0.000000,K__\n0.016000,__\n2.000004,K__\n2.033000,\nN/A,K__\n";

        let keyframes = parse_keyframes(listing);

        assert_eq!(
            keyframes,
            vec![Duration::ZERO, Duration::from_secs_f64(2.000004)]
        );
    }

    #[test]
    fn keyframes_are_sorted_and_deduplicated() {
        let keyframes = parse_keyframes("4.0,K\n0.0,K\n4.0,K\n2.0,K\n");

        assert_eq!(
            keyframes,
            vec![
                Duration::ZERO,
                Duration::from_secs(2),
                Duration::from_secs(4)
            ]
        );
    }

    #[test]
    fn the_encoder_follows_the_source_codec() {
        assert!(video_encoder("hevc", Container::Mp4).contains(&"libx265"));
        assert!(video_encoder("av1", Container::Mp4).contains(&"libsvtav1"));
        assert!(video_encoder("h264", Container::Mp4).contains(&"libx264"));
        assert!(video_encoder("", Container::Mp4).contains(&"libx264"));
    }

    #[test]
    fn a_webm_cut_never_asks_for_a_codec_the_container_rejects() {
        assert!(video_encoder("h264", Container::WebM).contains(&"libvpx-vp9"));
    }

    #[test]
    fn timestamps_keep_the_resolution_the_keyframe_was_read_at() {
        assert_eq!(seconds(Duration::from_millis(6_040)), "6.040000");
        assert_eq!(seconds(Duration::from_secs_f64(6.000001)), "6.000001");
        assert_eq!(seconds(Duration::ZERO), "0.000000");
    }

    #[test]
    fn a_reencode_reencodes_the_sound_as_well() {
        assert!(audio_encoder(Container::Mp4).contains(&"aac"));
        assert!(audio_encoder(Container::WebM).contains(&"libopus"));
    }

    #[test]
    fn a_failure_reports_the_tail_of_the_ffmpeg_output() {
        let stderr = b"first\n\nsecond\nthird\nfourth\n";

        assert_eq!(last_lines(stderr), "second; third; fourth");
        assert_eq!(last_lines(b""), "ffmpeg reported no reason");
    }
}
