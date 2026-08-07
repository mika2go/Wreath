pub const CLIP_SAVED_MESSAGE: u32 = 0x8000 + 31;

#[cfg(target_os = "windows")]
pub fn broadcast_clip_saved() {
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowExW, FindWindowW, HWND_MESSAGE, PostMessageW,
    };
    use windows::core::{PCWSTR, w};

    let app = unsafe { FindWindowW(w!("WreathApplicationWindow"), PCWSTR::null()) };
    if let Ok(app) = app
        && !app.is_invalid()
    {
        let _ = unsafe { PostMessageW(Some(app), CLIP_SAVED_MESSAGE, WPARAM(0), LPARAM(0)) };
    }
    let tray = unsafe {
        FindWindowExW(
            Some(HWND_MESSAGE),
            Some(HWND::default()),
            w!("WreathTrayWindow"),
            PCWSTR::null(),
        )
    };
    if let Ok(tray) = tray
        && !tray.is_invalid()
    {
        let _ = unsafe { PostMessageW(Some(tray), CLIP_SAVED_MESSAGE, WPARAM(0), LPARAM(0)) };
    }
}

#[cfg(target_os = "windows")]
pub fn notify_app_clip_saved() {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW};
    use windows::core::{PCWSTR, w};

    let app = unsafe { FindWindowW(w!("WreathApplicationWindow"), PCWSTR::null()) };
    if let Ok(app) = app
        && !app.is_invalid()
    {
        let _ = unsafe { PostMessageW(Some(app), CLIP_SAVED_MESSAGE, WPARAM(0), LPARAM(0)) };
    }
}

#[cfg(target_os = "windows")]
pub fn play_clip_saved_sound() {
    use windows::Win32::Media::Audio::{PlaySoundW, SND_MEMORY, SND_NODEFAULT, SND_SYNC};
    use windows::core::PCWSTR;

    let wave = render_clip_saved_wave();
    let _ = unsafe {
        PlaySoundW(
            PCWSTR(wave.as_ptr().cast()),
            None,
            SND_MEMORY | SND_NODEFAULT | SND_SYNC,
        )
    };
}

#[cfg(any(target_os = "windows", test))]
fn render_clip_saved_wave() -> Vec<u8> {
    const SAMPLE_RATE: u32 = 48_000;
    const DURATION_SECONDS: f32 = 0.48;
    let sample_count = (SAMPLE_RATE as f32 * DURATION_SECONDS) as usize;
    let data_bytes = u32::try_from(sample_count * size_of::<i16>()).unwrap_or(u32::MAX);
    let mut wave = Vec::with_capacity(44 + data_bytes as usize);
    wave.extend_from_slice(b"RIFF");
    wave.extend_from_slice(&(36_u32.saturating_add(data_bytes)).to_le_bytes());
    wave.extend_from_slice(b"WAVEfmt ");
    wave.extend_from_slice(&16_u32.to_le_bytes());
    wave.extend_from_slice(&1_u16.to_le_bytes());
    wave.extend_from_slice(&1_u16.to_le_bytes());
    wave.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    wave.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    wave.extend_from_slice(&2_u16.to_le_bytes());
    wave.extend_from_slice(&16_u16.to_le_bytes());
    wave.extend_from_slice(b"data");
    wave.extend_from_slice(&data_bytes.to_le_bytes());
    for index in 0..sample_count {
        let time = index as f32 / SAMPLE_RATE as f32;
        let first = chime_tone(time, 0.0, 739.99, 0.30, 0.066);
        let second = chime_tone(time, 0.12, 1108.73, 0.36, 0.058);
        let sample = ((first + second).clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
        wave.extend_from_slice(&sample.to_le_bytes());
    }
    wave
}

#[cfg(any(target_os = "windows", test))]
fn chime_tone(time: f32, start: f32, frequency: f32, duration: f32, level: f32) -> f32 {
    let local_time = time - start;
    if !(0.0..duration).contains(&local_time) {
        return 0.0;
    }
    let attack = (local_time / 0.012).min(1.0);
    let release = ((duration - local_time) / 0.09).min(1.0);
    let decay = (-5.2 * local_time).exp();
    let phase = std::f32::consts::TAU * frequency * local_time;
    let timbre = phase.sin() + 0.16 * (phase * 2.0 + 0.35).sin();
    level * attack * release * decay * timbre
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_feedback_is_a_short_pcm_wave() {
        let wave = render_clip_saved_wave();
        assert_eq!(&wave[..4], b"RIFF");
        assert_eq!(&wave[8..12], b"WAVE");
        assert_eq!(&wave[36..40], b"data");
        assert!(wave.len() > 44);
        assert!(wave.len() < 48_000 * 2);
    }
}
