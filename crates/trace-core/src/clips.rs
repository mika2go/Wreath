use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clip {
    pub path: PathBuf,
    pub title: String,
    pub size_bytes: u64,
    pub modified: SystemTime,
}

#[derive(Debug, Clone)]
pub struct ClipPreview {
    pub thumbnail: Option<PathBuf>,
    pub duration_seconds: Option<u64>,
}

pub fn scan(directory: &Path) -> io::Result<Vec<Clip>> {
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let mut clips = Vec::new();
    collect(directory, 0, &mut clips)?;
    clips.sort_by(|left, right| {
        right
            .modified
            .cmp(&left.modified)
            .then_with(|| left.path.cmp(&right.path))
    });
    Ok(clips)
}

fn collect(directory: &Path, depth: u8, clips: &mut Vec<Clip>) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let path = entry.path();
        if file_type.is_dir() && depth < 2 {
            collect(&path, depth + 1, clips)?;
            continue;
        }
        if !file_type.is_file() || !is_video(&path) {
            continue;
        }
        let metadata = entry.metadata()?;
        let title = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("Untitled clip")
            .replace(['_', '-'], " ");
        clips.push(Clip {
            path,
            title,
            size_bytes: metadata.len(),
            modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
        });
    }
    Ok(())
}

pub fn build_preview(clip: &Clip, thumbnail_directory: &Path) -> ClipPreview {
    let duration_seconds = probe_duration(&clip.path);
    let thumbnail = thumbnail_path(clip, thumbnail_directory);
    if thumbnail.exists() {
        return ClipPreview {
            thumbnail: Some(thumbnail),
            duration_seconds,
        };
    }
    if fs::create_dir_all(thumbnail_directory).is_err() {
        return ClipPreview {
            thumbnail: None,
            duration_seconds,
        };
    }
    let status = Command::new("ffmpeg")
        .args(["-nostdin", "-loglevel", "error", "-y", "-ss", "00:00:01"])
        .arg("-i")
        .arg(&clip.path)
        .args(["-frames:v", "1", "-vf", "scale=640:-2", "-q:v", "4"])
        .arg(&thumbnail)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    ClipPreview {
        thumbnail: status
            .is_ok_and(|status| status.success())
            .then_some(thumbnail),
        duration_seconds,
    }
}

pub fn thumbnail_path(clip: &Clip, thumbnail_directory: &Path) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    clip.path.hash(&mut hasher);
    clip.size_bytes.hash(&mut hasher);
    clip.modified.hash(&mut hasher);
    thumbnail_directory.join(format!("{:016x}.jpg", hasher.finish()))
}

fn probe_duration(path: &Path) -> Option<u64> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(path)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let seconds = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<f64>()
        .ok()?;
    Some(seconds.max(0.0).round() as u64)
}

pub fn format_duration(seconds: u64) -> String {
    let hours = seconds / 3_600;
    let minutes = seconds % 3_600 / 60;
    let seconds = seconds % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

pub fn format_size(bytes: u64) -> String {
    const MIB: f64 = 1_048_576.0;
    const GIB: f64 = 1_073_741_824.0;
    if bytes as f64 >= GIB {
        format!("{:.1} GB", bytes as f64 / GIB)
    } else {
        format!("{:.0} MB", bytes as f64 / MIB)
    }
}

pub fn format_age(modified: SystemTime) -> String {
    let age = SystemTime::now()
        .duration_since(modified)
        .unwrap_or(Duration::ZERO);
    if age.as_secs() < 60 {
        "just now".into()
    } else if age.as_secs() < 3_600 {
        format!("{}m ago", age.as_secs() / 60)
    } else if age.as_secs() < 86_400 {
        format!("{}h ago", age.as_secs() / 3_600)
    } else {
        format!("{}d ago", age.as_secs() / 86_400)
    }
}

fn is_video(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "mp4" | "mkv" | "webm"
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_supported_video_extensions() {
        assert!(is_video(Path::new("clip.MP4")));
        assert!(is_video(Path::new("clip.mkv")));
        assert!(is_video(Path::new("clip.webm")));
        assert!(!is_video(Path::new("notes.txt")));
    }

    #[test]
    fn formats_duration_and_size() {
        assert_eq!(format_duration(65), "1:05");
        assert_eq!(format_duration(3_661), "1:01:01");
        assert_eq!(format_size(20 * 1_048_576), "20 MB");
    }
}
