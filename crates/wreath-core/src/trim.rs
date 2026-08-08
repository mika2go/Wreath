use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::clips::{self, Clip};

pub const MINIMUM_LENGTH: Duration = Duration::from_millis(300);
pub const SNAP_TOLERANCE: Duration = Duration::from_millis(120);
const EXACT_TOLERANCE: Duration = Duration::from_millis(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TrimMode {
    #[default]
    Auto,
    Lossless,
    Precise,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrimOutput {
    Replace,
    NewClip(Option<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrimRequest {
    pub source: PathBuf,
    pub start: Duration,
    pub end: Duration,
    pub mode: TrimMode,
    pub output: TrimOutput,
}

impl TrimRequest {
    pub fn new(source: impl Into<PathBuf>, start: Duration, end: Duration) -> Self {
        Self {
            source: source.into(),
            start,
            end,
            mode: TrimMode::Auto,
            output: TrimOutput::NewClip(None),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrimReport {
    pub path: PathBuf,
    pub start: Duration,
    pub end: Duration,
    pub reencoded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipTiming {
    pub duration: Duration,
    pub keyframes: Vec<Duration>,
}

impl ClipTiming {
    pub fn nearest_keyframe(&self, position: Duration) -> Option<Duration> {
        nearest(&self.keyframes, position)
    }

    pub fn keyframe_at_or_before(&self, position: Duration) -> Duration {
        at_or_before(&self.keyframes, position)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CutPlan {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub container: Container,
    pub start: Duration,
    pub end: Duration,
    pub reencode: bool,
}

impl CutPlan {
    pub fn length(&self) -> Duration {
        self.end.saturating_sub(self.start)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Container {
    Mp4,
    Matroska,
    WebM,
}

impl Container {
    pub fn from_path(path: &Path) -> Option<Self> {
        match path
            .extension()
            .and_then(|value| value.to_str())?
            .to_ascii_lowercase()
            .as_str()
        {
            "mp4" => Some(Self::Mp4),
            "mkv" => Some(Self::Matroska),
            "webm" => Some(Self::WebM),
            _ => None,
        }
    }

    pub const fn muxer(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::Matroska => "matroska",
            Self::WebM => "webm",
        }
    }
}

pub trait TrimBackend {
    fn timing(&self, source: &Path) -> Result<ClipTiming, TrimError>;
    fn cut(&self, plan: &CutPlan) -> Result<(), TrimError>;
}

#[derive(Debug)]
pub enum TrimError {
    SourceMissing(PathBuf),
    Range(String),
    Name(String),
    Unsupported(String),
    Backend(String),
    Io(io::Error),
}

impl fmt::Display for TrimError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceMissing(path) => write!(formatter, "{} is not a clip", path.display()),
            Self::Range(message) => formatter.write_str(message),
            Self::Name(message) => formatter.write_str(message),
            Self::Unsupported(message) => formatter.write_str(message),
            Self::Backend(message) => write!(formatter, "cutting failed: {message}"),
            Self::Io(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for TrimError {}

impl From<io::Error> for TrimError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

pub fn timing(backend: &impl TrimBackend, source: &Path) -> Result<ClipTiming, TrimError> {
    if !source.is_file() {
        return Err(TrimError::SourceMissing(source.to_path_buf()));
    }
    backend.timing(source)
}

pub fn trim(
    backend: &impl TrimBackend,
    request: &TrimRequest,
    thumbnail_directory: &Path,
) -> Result<TrimReport, TrimError> {
    if !request.source.is_file() {
        return Err(TrimError::SourceMissing(request.source.clone()));
    }
    let container = Container::from_path(&request.source).ok_or_else(|| {
        TrimError::Unsupported(format!(
            "{} is not a container Wreath can cut",
            request.source.display()
        ))
    })?;
    let directory = request
        .source
        .parent()
        .ok_or_else(|| TrimError::SourceMissing(request.source.clone()))?
        .to_path_buf();

    let timing = backend.timing(&request.source)?;
    let (start, end) = resolve_range(request.start, request.end, timing.duration)?;
    let (start, reencode) = plan_start(request.mode, start, &timing.keyframes);
    if end.saturating_sub(start) < MINIMUM_LENGTH {
        return Err(range_error(end.saturating_sub(start)));
    }

    let destination = match &request.output {
        TrimOutput::Replace => request.source.clone(),
        TrimOutput::NewClip(name) => new_clip_path(&request.source, &directory, name.as_deref())?,
    };
    let scratch = scratch_path(&directory, &request.source);
    let plan = CutPlan {
        source: request.source.clone(),
        destination: scratch.clone(),
        container,
        start,
        end,
        reencode,
    };

    let cut = backend.cut(&plan);
    if let Err(error) = cut {
        let _ = fs::remove_file(&scratch);
        return Err(error);
    }
    if let Err(error) = place(&scratch, &destination, request, thumbnail_directory) {
        let _ = fs::remove_file(&scratch);
        return Err(error);
    }

    Ok(TrimReport {
        path: destination,
        start,
        end,
        reencoded: reencode,
    })
}

fn place(
    scratch: &Path,
    destination: &Path,
    request: &TrimRequest,
    thumbnail_directory: &Path,
) -> Result<(), TrimError> {
    if !scratch.is_file() {
        return Err(TrimError::Backend(
            "the cut produced no file; check that a decoder for this clip is installed".into(),
        ));
    }
    if fs::metadata(scratch)?.len() == 0 {
        return Err(TrimError::Backend("the cut produced an empty file".into()));
    }
    if matches!(request.output, TrimOutput::NewClip(_)) && destination.exists() {
        return Err(TrimError::Name(
            "a clip with this name already exists".into(),
        ));
    }
    let stale = matches!(request.output, TrimOutput::Replace)
        .then(|| thumbnail_of(&request.source, thumbnail_directory))
        .flatten();
    fs::rename(scratch, destination)?;
    if let Some(stale) = stale {
        match fs::remove_file(stale) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn thumbnail_of(source: &Path, thumbnail_directory: &Path) -> Option<PathBuf> {
    let metadata = fs::metadata(source).ok()?;
    Some(clips::thumbnail_path(
        &Clip {
            path: source.to_path_buf(),
            title: String::new(),
            size_bytes: metadata.len(),
            modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
        },
        thumbnail_directory,
    ))
}

fn new_clip_path(
    source: &Path,
    directory: &Path,
    name: Option<&str>,
) -> Result<PathBuf, TrimError> {
    let extension = source
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("mp4");
    match name {
        Some(name) => {
            let name =
                clips::validate_name(name).map_err(|error| TrimError::Name(error.to_string()))?;
            let path = directory.join(format!("{name}.{extension}"));
            if path.exists() {
                return Err(TrimError::Name(
                    "a clip with this name already exists".into(),
                ));
            }
            Ok(path)
        }
        None => {
            let stem = source
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("clip");
            Ok(clips::unique_destination(
                directory,
                std::ffi::OsStr::new(&format!("{stem} (cut).{extension}")),
            ))
        }
    }
}

fn scratch_path(directory: &Path, source: &Path) -> PathBuf {
    let stem = source
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("clip");
    let unique = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    directory.join(format!(
        ".{stem}.wreath-cut-{}-{unique}.tmp",
        std::process::id()
    ))
}

fn resolve_range(
    start: Duration,
    end: Duration,
    total: Duration,
) -> Result<(Duration, Duration), TrimError> {
    if !total.is_zero() && start >= total {
        return Err(TrimError::Range(
            "the cut starts after the clip ends".into(),
        ));
    }
    let end = if total.is_zero() { end } else { end.min(total) };
    if end <= start {
        return Err(TrimError::Range("the cut ends before it starts".into()));
    }
    if end - start < MINIMUM_LENGTH {
        return Err(range_error(end - start));
    }
    Ok((start, end))
}

fn range_error(length: Duration) -> TrimError {
    TrimError::Range(format!(
        "a cut has to keep at least {} ms, not {} ms",
        MINIMUM_LENGTH.as_millis(),
        length.as_millis()
    ))
}

fn plan_start(mode: TrimMode, start: Duration, keyframes: &[Duration]) -> (Duration, bool) {
    match mode {
        TrimMode::Lossless => (at_or_before(keyframes, start), false),
        TrimMode::Auto => match nearest(keyframes, start) {
            Some(keyframe) if distance(keyframe, start) <= SNAP_TOLERANCE => (keyframe, false),
            _ => (start, true),
        },
        TrimMode::Precise => match nearest(keyframes, start) {
            Some(keyframe) if distance(keyframe, start) <= EXACT_TOLERANCE => (keyframe, false),
            _ => (start, true),
        },
    }
}

fn nearest(keyframes: &[Duration], position: Duration) -> Option<Duration> {
    keyframes
        .iter()
        .copied()
        .min_by_key(|keyframe| distance(*keyframe, position))
}

fn at_or_before(keyframes: &[Duration], position: Duration) -> Duration {
    keyframes
        .iter()
        .copied()
        .filter(|keyframe| *keyframe <= position)
        .max()
        .unwrap_or_default()
}

fn distance(left: Duration, right: Duration) -> Duration {
    left.abs_diff(right)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keyframes() -> Vec<Duration> {
        (0..16)
            .map(|index| Duration::from_secs(index * 2))
            .collect()
    }

    #[test]
    fn lossless_never_loses_content_at_the_start() {
        let (start, reencode) = plan_start(
            TrimMode::Lossless,
            Duration::from_millis(5_400),
            &keyframes(),
        );

        assert_eq!(start, Duration::from_secs(4));
        assert!(!reencode);
    }

    #[test]
    fn auto_snaps_to_a_close_keyframe_and_stays_lossless() {
        let (start, reencode) =
            plan_start(TrimMode::Auto, Duration::from_millis(6_040), &keyframes());

        assert_eq!(start, Duration::from_secs(6));
        assert!(!reencode);
    }

    #[test]
    fn auto_reencodes_when_no_keyframe_is_close() {
        let requested = Duration::from_millis(6_900);
        let (start, reencode) = plan_start(TrimMode::Auto, requested, &keyframes());

        assert_eq!(start, requested);
        assert!(reencode);
    }

    #[test]
    fn precise_keeps_the_requested_start_even_next_to_a_keyframe() {
        let requested = Duration::from_millis(6_050);
        let (start, reencode) = plan_start(TrimMode::Precise, requested, &keyframes());

        assert_eq!(start, requested);
        assert!(reencode);
    }

    #[test]
    fn an_aligned_start_is_copied_in_every_mode() {
        for mode in [TrimMode::Auto, TrimMode::Lossless, TrimMode::Precise] {
            let (start, reencode) = plan_start(mode, Duration::from_secs(8), &keyframes());

            assert_eq!(start, Duration::from_secs(8));
            assert!(!reencode, "{mode:?} should not need a re-encode");
        }
    }

    #[test]
    fn a_clip_without_keyframe_information_is_reencoded() {
        let (start, reencode) = plan_start(TrimMode::Auto, Duration::from_secs(3), &[]);

        assert_eq!(start, Duration::from_secs(3));
        assert!(reencode);
    }

    #[test]
    fn the_end_is_clamped_to_the_clip() {
        let (start, end) = resolve_range(
            Duration::from_secs(2),
            Duration::from_secs(90),
            Duration::from_secs(30),
        )
        .unwrap();

        assert_eq!(start, Duration::from_secs(2));
        assert_eq!(end, Duration::from_secs(30));
    }

    #[test]
    fn inverted_and_tiny_ranges_are_rejected() {
        let total = Duration::from_secs(30);
        assert!(matches!(
            resolve_range(Duration::from_secs(10), Duration::from_secs(4), total),
            Err(TrimError::Range(_))
        ));
        assert!(matches!(
            resolve_range(
                Duration::from_secs(10),
                Duration::from_millis(10_100),
                total
            ),
            Err(TrimError::Range(_))
        ));
        assert!(matches!(
            resolve_range(Duration::from_secs(40), Duration::from_secs(45), total),
            Err(TrimError::Range(_))
        ));
    }

    #[test]
    fn a_named_cut_keeps_the_container_and_refuses_traversal() {
        let directory = Path::new("/clips");
        let source = directory.join("moment.mkv");

        let path = new_clip_path(&source, directory, Some("Best moment")).unwrap();

        assert_eq!(path, directory.join("Best moment.mkv"));
        assert!(matches!(
            new_clip_path(&source, directory, Some("../escape")),
            Err(TrimError::Name(_))
        ));
    }

    #[test]
    fn the_scratch_file_is_hidden_and_is_not_a_clip() {
        let scratch = scratch_path(Path::new("/clips"), Path::new("/clips/moment.mp4"));
        let name = scratch.file_name().unwrap().to_str().unwrap();

        assert!(name.starts_with('.'));
        assert!(Container::from_path(&scratch).is_none());
    }

    #[test]
    fn containers_map_to_muxers() {
        assert_eq!(
            Container::from_path(Path::new("clip.MP4")).unwrap().muxer(),
            "mp4"
        );
        assert_eq!(
            Container::from_path(Path::new("clip.mkv")).unwrap().muxer(),
            "matroska"
        );
        assert!(Container::from_path(Path::new("notes.txt")).is_none());
    }
}
