use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use gdk_pixbuf::Pixbuf;
use serde_json::Value;

const FALLBACK_ACCENT: Rgb = Rgb::new(137, 113, 255);
const FALLBACK_DOMINANT: Rgb = Rgb::new(22, 27, 38);

#[derive(Clone, Copy)]
struct Rgb {
    red: u8,
    green: u8,
    blue: u8,
}

impl Rgb {
    const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }

    fn parse(value: &str) -> Option<Self> {
        let hex = value.trim().trim_start_matches('#');
        if hex.len() != 6 {
            return None;
        }
        Some(Self {
            red: u8::from_str_radix(&hex[0..2], 16).ok()?,
            green: u8::from_str_radix(&hex[2..4], 16).ok()?,
            blue: u8::from_str_radix(&hex[4..6], 16).ok()?,
        })
    }

    fn mix(self, other: Self, amount: f32) -> Self {
        let channel = |base: u8, overlay: u8| {
            (f32::from(base) * (1.0 - amount) + f32::from(overlay) * amount)
                .round()
                .clamp(0.0, 255.0) as u8
        };
        Self::new(
            channel(self.red, other.red),
            channel(self.green, other.green),
            channel(self.blue, other.blue),
        )
    }

    fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.red, self.green, self.blue)
    }
}

pub struct Palette {
    accent: Rgb,
    dominant: Rgb,
}

impl Palette {
    pub fn discover() -> Self {
        if let Some(wallpaper) = current_wallpaper() {
            if let Some(palette) = quickshell_palette(&wallpaper) {
                return palette;
            }
            if let Some(palette) = sample_wallpaper(&wallpaper) {
                return palette;
            }
        }
        pywal_palette().unwrap_or(Self {
            accent: FALLBACK_ACCENT,
            dominant: FALLBACK_DOMINANT,
        })
    }

    pub fn css_prefix(&self) -> String {
        let background = Rgb::new(7, 9, 13).mix(self.dominant, 0.18);
        let sidebar = Rgb::new(10, 13, 19).mix(self.dominant, 0.22);
        let surface = Rgb::new(15, 19, 27).mix(self.dominant, 0.24);
        let stage = Rgb::new(3, 4, 7).mix(self.dominant, 0.10);
        let accent_strong = self.accent.mix(Rgb::new(255, 255, 255), 0.08);
        format!(
            "@define-color trace_bg {};\n\
             @define-color trace_sidebar {};\n\
             @define-color trace_surface {};\n\
             @define-color trace_stage {};\n\
             @define-color trace_accent {};\n\
             @define-color trace_accent_strong {};\n",
            background.hex(),
            sidebar.hex(),
            surface.hex(),
            stage.hex(),
            self.accent.hex(),
            accent_strong.hex(),
        )
    }
}

fn current_wallpaper() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("TRACE_WALLPAPER").map(PathBuf::from)
        && path.is_file()
    {
        return Some(path);
    }
    let cache = cache_home().join("quickshell/wallpaper-engine-current");
    if let Ok(value) = fs::read_to_string(cache)
        && let Some(path) = value.lines().map(str::trim).find(|line| !line.is_empty())
    {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    command_wallpaper("hyprctl", &["hyprpaper", "listactive"], |line| {
        line.split_once(" = ").map(|(_, path)| path.trim())
    })
    .or_else(|| {
        command_wallpaper("swww", &["query"], |line| {
            line.split_once("image: ").map(|(_, path)| path.trim())
        })
    })
}

fn command_wallpaper(
    executable: &str,
    arguments: &[&str],
    extract: impl Fn(&str) -> Option<&str>,
) -> Option<PathBuf> {
    let output = Command::new(executable)
        .args(arguments)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(extract)
        .map(PathBuf::from)
        .filter(|path| path.is_file())
}

fn quickshell_palette(wallpaper: &Path) -> Option<Palette> {
    let value: Value = serde_json::from_slice(
        &fs::read(cache_home().join("quickshell/wallpaper-colors.json")).ok()?,
    )
    .ok()?;
    let colors = value
        .get("files")?
        .get(wallpaper.to_string_lossy().as_ref())?
        .get("colors")?;
    Some(Palette {
        accent: Rgb::parse(colors.get("familyColor")?.as_str()?)?,
        dominant: Rgb::parse(colors.get("dominantColor")?.as_str()?)?,
    })
}

fn pywal_palette() -> Option<Palette> {
    let value: Value =
        serde_json::from_slice(&fs::read(cache_home().join("wal/colors.json")).ok()?).ok()?;
    Some(Palette {
        accent: Rgb::parse(value.get("colors")?.get("color4")?.as_str()?)?,
        dominant: Rgb::parse(value.get("special")?.get("background")?.as_str()?)?,
    })
}

fn sample_wallpaper(path: &Path) -> Option<Palette> {
    let pixbuf = Pixbuf::from_file_at_scale(path, 72, 72, true).ok()?;
    let bytes = pixbuf.read_pixel_bytes();
    let pixels = bytes.as_ref();
    let channels = usize::try_from(pixbuf.n_channels()).ok()?;
    let row_stride = usize::try_from(pixbuf.rowstride()).ok()?;
    let width = usize::try_from(pixbuf.width()).ok()?;
    let height = usize::try_from(pixbuf.height()).ok()?;
    if channels < 3 || width == 0 || height == 0 {
        return None;
    }

    let mut total = [0_u64; 3];
    let mut count = 0_u64;
    let mut hue_weight = [0_f64; 24];
    let mut hue_rgb = [[0_f64; 3]; 24];
    for y in 0..height {
        for x in 0..width {
            let offset = y * row_stride + x * channels;
            let (red, green, blue) = (
                f64::from(pixels[offset]),
                f64::from(pixels[offset + 1]),
                f64::from(pixels[offset + 2]),
            );
            total[0] += red as u64;
            total[1] += green as u64;
            total[2] += blue as u64;
            count += 1;

            let maximum = red.max(green).max(blue);
            let minimum = red.min(green).min(blue);
            let delta = maximum - minimum;
            let lightness = (maximum + minimum) / 510.0;
            let saturation = if delta == 0.0 {
                0.0
            } else {
                delta / (255.0 - (maximum + minimum - 255.0).abs())
            };
            if saturation < 0.22 || !(0.16..=0.86).contains(&lightness) {
                continue;
            }
            let hue = if maximum == red {
                60.0 * ((green - blue) / delta).rem_euclid(6.0)
            } else if maximum == green {
                60.0 * ((blue - red) / delta + 2.0)
            } else {
                60.0 * ((red - green) / delta + 4.0)
            };
            let bin = ((hue / 15.0).floor() as usize).min(23);
            let weight = saturation * saturation * (1.0 - (lightness - 0.52).abs());
            hue_weight[bin] += weight;
            hue_rgb[bin][0] += red * weight;
            hue_rgb[bin][1] += green * weight;
            hue_rgb[bin][2] += blue * weight;
        }
    }
    let dominant = Rgb::new(
        (total[0] / count) as u8,
        (total[1] / count) as u8,
        (total[2] / count) as u8,
    );
    let accent_bin = hue_weight
        .iter()
        .enumerate()
        .max_by(|(_, left), (_, right)| left.total_cmp(right))
        .map(|(index, _)| index)?;
    let weight = hue_weight[accent_bin];
    let accent = if weight > 0.0 {
        Rgb::new(
            (hue_rgb[accent_bin][0] / weight).round() as u8,
            (hue_rgb[accent_bin][1] / weight).round() as u8,
            (hue_rgb[accent_bin][2] / weight).round() as u8,
        )
    } else {
        FALLBACK_ACCENT
    };
    Some(Palette { accent, dominant })
}

fn cache_home() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_mixes_rgb_colors() {
        let color = Rgb::parse("#668fc2").expect("valid color");
        assert_eq!(color.hex(), "#668fc2");
        assert_eq!(
            Rgb::new(0, 0, 0).mix(Rgb::new(100, 50, 200), 0.5).hex(),
            "#321964"
        );
    }
}
