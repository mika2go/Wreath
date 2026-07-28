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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collection {
    pub path: PathBuf,
    pub name: String,
    pub clip_count: usize,
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

pub fn collections(directory: &Path) -> io::Result<Vec<Collection>> {
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let mut collections = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let clip_count = scan(&path)?.len();
        collections.push(Collection {
            path,
            name,
            clip_count,
        });
    }
    collections.sort_by(|left, right| {
        left.name
            .to_ascii_lowercase()
            .cmp(&right.name.to_ascii_lowercase())
    });
    Ok(collections)
}

pub fn create_collection(directory: &Path, name: &str) -> io::Result<PathBuf> {
    let name = validate_name(name)?;
    fs::create_dir_all(directory)?;
    let path = directory.join(name);
    if path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "a collection with this name already exists",
        ));
    }
    fs::create_dir(&path)?;
    Ok(path)
}

pub fn rename(clip: &Clip, name: &str, thumbnail_directory: &Path) -> io::Result<PathBuf> {
    let name = validate_name(name)?;
    let extension = clip
        .path
        .extension()
        .and_then(|value| value.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "clip has no extension"))?;
    let destination = clip
        .path
        .with_file_name(format!("{name}.{extension}"));
    move_clip(clip, &destination, thumbnail_directory)
}

pub fn move_to_collection(
    clip: &Clip,
    directory: &Path,
    collection: &Path,
    thumbnail_directory: &Path,
) -> io::Result<PathBuf> {
    let root = directory.canonicalize()?;
    let collection = collection.canonicalize()?;
    if collection.parent() != Some(root.as_path()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "collection must be a direct child of the clip directory",
        ));
    }
    let file_name = clip
        .path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid clip path"))?;
    move_clip(clip, &collection.join(file_name), thumbnail_directory)
}

fn move_clip(clip: &Clip, destination: &Path, thumbnail_directory: &Path) -> io::Result<PathBuf> {
    if destination == clip.path {
        return Ok(destination.to_path_buf());
    }
    if destination.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "a clip with this name already exists",
        ));
    }
    let thumbnail = thumbnail_path(clip, thumbnail_directory);
    fs::rename(&clip.path, destination)?;
    match fs::remove_file(thumbnail) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    Ok(destination.to_path_buf())
}

fn validate_name(name: &str) -> io::Result<&str> {
    let name = name.trim();
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.chars().count() > 80
        || name
            .chars()
            .any(|character| character.is_control() || matches!(character, '/' | '\\'))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "use 1–80 characters without slashes",
        ));
    }
    Ok(name)
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

pub fn delete(clip: &Clip, thumbnail_directory: &Path) -> io::Result<()> {
    let thumbnail = thumbnail_path(clip, thumbnail_directory);
    fs::remove_file(&clip.path)?;
    match fs::remove_file(thumbnail) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
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

    #[test]
    fn deletes_clip_and_cached_thumbnail() {
        let root = std::env::temp_dir().join(format!(
            "trace-delete-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let thumbnails = root.join("thumbnails");
        fs::create_dir_all(&thumbnails).unwrap();
        let path = root.join("clip.mp4");
        fs::write(&path, b"clip").unwrap();
        let metadata = fs::metadata(&path).unwrap();
        let clip = Clip {
            path: path.clone(),
            title: "Clip".into(),
            size_bytes: metadata.len(),
            modified: metadata.modified().unwrap(),
        };
        let thumbnail = thumbnail_path(&clip, &thumbnails);
        fs::write(&thumbnail, b"thumbnail").unwrap();

        delete(&clip, &thumbnails).unwrap();

        assert!(!path.exists());
        assert!(!thumbnail.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn creates_lists_and_moves_collection_clips() {
        let root = test_root("collections");
        let thumbnails = root.join("thumbnails");
        let clips_directory = root.join("clips");
        fs::create_dir_all(&clips_directory).unwrap();
        let collection = create_collection(&clips_directory, "Funny").unwrap();
        let path = clips_directory.join("moment.mp4");
        fs::write(&path, b"clip").unwrap();
        let clip = scan(&clips_directory).unwrap().remove(0);

        let moved = move_to_collection(&clip, &clips_directory, &collection, &thumbnails).unwrap();

        assert_eq!(moved, collection.join("moment.mp4"));
        let listed = collections(&clips_directory).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "Funny");
        assert_eq!(listed[0].clip_count, 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn renames_clip_without_changing_extension() {
        let root = test_root("rename");
        let thumbnails = root.join("thumbnails");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("old-name.webm");
        fs::write(&path, b"clip").unwrap();
        let clip = scan(&root).unwrap().remove(0);

        let renamed = rename(&clip, "New moment", &thumbnails).unwrap();

        assert_eq!(renamed, root.join("New moment.webm"));
        assert!(renamed.exists());
        assert!(rename(&scan(&root).unwrap()[0], "../escape", &thumbnails).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    fn test_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "trace-{name}-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }
}
