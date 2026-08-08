use std::path::PathBuf;
use std::time::Duration;

use wreath_core::paths::AppPaths;
use wreath_core::trim::{self, TrimMode, TrimOutput, TrimRequest};

pub fn run(arguments: &[String]) -> Result<(), String> {
    let request = parse(arguments)?;
    let paths = AppPaths::discover();
    let backend = backend()?;
    let report =
        trim::trim(&backend, &request, &paths.thumbnail_dir).map_err(|error| error.to_string())?;

    println!(
        "cut {} – {} into {}",
        format_timecode(report.start),
        format_timecode(report.end),
        report.path.display()
    );
    println!(
        "{}",
        if report.reencoded {
            "re-encoded to hit the requested start exactly"
        } else {
            "copied without re-encoding"
        }
    );
    Ok(())
}

#[cfg(target_os = "linux")]
fn backend() -> Result<wreath_core::trim_ffmpeg::FfmpegTrimmer, String> {
    Ok(wreath_core::trim_ffmpeg::FfmpegTrimmer)
}

#[cfg(target_os = "windows")]
fn backend() -> Result<wreath_windows::trim::MediaFoundationTrimmer, String> {
    wreath_windows::trim::MediaFoundationTrimmer::new().map_err(|error| error.to_string())
}

fn parse(arguments: &[String]) -> Result<TrimRequest, String> {
    let mut positional = Vec::new();
    let mut mode = TrimMode::Auto;
    let mut output = None;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--replace" => output = Some(TrimOutput::Replace),
            "--lossless" => mode = TrimMode::Lossless,
            "--precise" => mode = TrimMode::Precise,
            "--name" => {
                index += 1;
                let name = arguments
                    .get(index)
                    .ok_or("`--name` needs a clip name".to_owned())?;
                output = Some(TrimOutput::NewClip(Some(name.clone())));
            }
            flag if flag.starts_with("--") => {
                return Err(format!("unknown option `{flag}` for `cut`"));
            }
            value => positional.push(value),
        }
        index += 1;
    }
    let [source, start, end] = positional.as_slice() else {
        return Err(
            "usage: wreathctl cut <clip> <start> <end> [--replace|--name <name>] \
                    [--lossless|--precise]"
                .into(),
        );
    };
    Ok(TrimRequest {
        source: PathBuf::from(source),
        start: parse_timecode(start)
            .ok_or_else(|| format!("`{start}` is not a position like 1:07.500"))?,
        end: parse_timecode(end)
            .ok_or_else(|| format!("`{end}` is not a position like 1:07.500"))?,
        mode,
        output: output.unwrap_or(TrimOutput::NewClip(None)),
    })
}

fn parse_timecode(text: &str) -> Option<Duration> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let mut seconds = 0.0_f64;
    let parts = text.split(':').collect::<Vec<_>>();
    if parts.len() > 3 {
        return None;
    }
    for (position, part) in parts.iter().enumerate() {
        let value = part.trim().parse::<f64>().ok()?;
        if value < 0.0 || !value.is_finite() {
            return None;
        }
        if position > 0 && value >= 60.0 {
            return None;
        }
        if position + 1 < parts.len() && part.contains('.') {
            return None;
        }
        seconds = seconds * 60.0 + value;
    }
    Some(Duration::from_secs_f64(seconds))
}

fn format_timecode(value: Duration) -> String {
    let total = value.as_secs_f64();
    let minutes = (total / 60.0).floor();
    format!("{minutes:.0}:{:06.3}", total - minutes * 60.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_are_read_as_seconds_minutes_and_hours() {
        assert_eq!(parse_timecode("7"), Some(Duration::from_secs(7)));
        assert_eq!(parse_timecode("7.25"), Some(Duration::from_millis(7_250)));
        assert_eq!(
            parse_timecode("1:07.5"),
            Some(Duration::from_millis(67_500))
        );
        assert_eq!(parse_timecode("1:00:00"), Some(Duration::from_secs(3_600)));
    }

    #[test]
    fn malformed_positions_are_refused() {
        assert!(parse_timecode("").is_none());
        assert!(parse_timecode("abc").is_none());
        assert!(parse_timecode("-4").is_none());
        assert!(parse_timecode("1:2:3:4").is_none());
        assert!(parse_timecode("1:75").is_none());
    }

    #[test]
    fn a_cut_defaults_to_a_new_clip_and_automatic_quality() {
        let arguments = ["clip.mp4", "2", "8"].map(String::from);

        let request = parse(&arguments).unwrap();

        assert_eq!(request.source, PathBuf::from("clip.mp4"));
        assert_eq!(request.start, Duration::from_secs(2));
        assert_eq!(request.end, Duration::from_secs(8));
        assert_eq!(request.mode, TrimMode::Auto);
        assert_eq!(request.output, TrimOutput::NewClip(None));
    }

    #[test]
    fn options_select_the_destination_and_the_quality() {
        let replace = ["clip.mp4", "2", "8", "--replace", "--lossless"].map(String::from);
        let named = ["clip.mp4", "2", "8", "--name", "Best bit", "--precise"].map(String::from);

        let replace = parse(&replace).unwrap();
        let named = parse(&named).unwrap();

        assert_eq!(replace.output, TrimOutput::Replace);
        assert_eq!(replace.mode, TrimMode::Lossless);
        assert_eq!(named.output, TrimOutput::NewClip(Some("Best bit".into())));
        assert_eq!(named.mode, TrimMode::Precise);
    }

    #[test]
    fn missing_and_unknown_arguments_are_reported() {
        assert!(parse(&["clip.mp4".to_owned(), "2".to_owned()]).is_err());
        assert!(parse(&["clip.mp4", "2", "8", "--fast"].map(String::from)).is_err());
        assert!(parse(&["clip.mp4", "2", "8", "--name"].map(String::from)).is_err());
    }

    #[test]
    fn positions_are_printed_with_millisecond_resolution() {
        assert_eq!(format_timecode(Duration::from_millis(67_500)), "1:07.500");
        assert_eq!(format_timecode(Duration::from_millis(500)), "0:00.500");
    }
}
