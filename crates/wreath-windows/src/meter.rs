#[cfg(target_os = "windows")]
use windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation;

#[cfg(target_os = "windows")]
pub struct MicrophoneMeter {
    endpoint_id: Option<String>,
    meter: Option<IAudioMeterInformation>,
}

#[cfg(target_os = "windows")]
impl MicrophoneMeter {
    pub fn closed() -> Self {
        Self {
            endpoint_id: None,
            meter: None,
        }
    }

    pub fn is_open(&self) -> bool {
        self.meter.is_some()
    }

    pub fn matches(&self, endpoint_id: Option<&str>) -> bool {
        self.endpoint_id.as_deref() == endpoint_id
    }

    pub fn open(endpoint_id: Option<&str>) -> Self {
        Self {
            endpoint_id: endpoint_id.map(str::to_owned),
            meter: activate(endpoint_id),
        }
    }

    pub fn peak_percent(&mut self) -> Option<u8> {
        let meter = self.meter.as_ref()?;
        match unsafe { meter.GetPeakValue() } {
            Ok(peak) => Some((peak.clamp(0.0, 1.0) * 100.0).round() as u8),
            Err(_) => {
                self.meter = None;
                None
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn activate(endpoint_id: Option<&str>) -> Option<IAudioMeterInformation> {
    use windows::Win32::Media::Audio::{
        IMMDeviceEnumerator, MMDeviceEnumerator, eCapture, eConsole,
    };
    use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
    use windows::core::HSTRING;

    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.ok()?;
    let device = match endpoint_id {
        Some(id) => unsafe { enumerator.GetDevice(&HSTRING::from(id)) }.ok()?,
        None => unsafe { enumerator.GetDefaultAudioEndpoint(eCapture, eConsole) }.ok()?,
    };
    unsafe { device.Activate::<IAudioMeterInformation>(CLSCTX_ALL, None) }.ok()
}

/// Opens its own capture stream so the level moves even when nothing else is
/// recording, which is what a microphone test has to show.
#[cfg(target_os = "windows")]
pub struct MicrophoneProbe {
    endpoint_id: Option<String>,
    capture: crate::audio::MicrophoneCapture,
}

#[cfg(target_os = "windows")]
impl MicrophoneProbe {
    pub fn open(endpoint_id: Option<&str>) -> Result<Self, crate::audio::AudioError> {
        Ok(Self {
            endpoint_id: endpoint_id.map(str::to_owned),
            capture: crate::audio::MicrophoneCapture::spawn(endpoint_id)?,
        })
    }

    /// Opens the requested endpoint and falls back to the Windows default when
    /// it is gone. The probe keeps identifying itself by the requested id so a
    /// fallback is not mistaken for a device change.
    pub fn open_with_fallback(
        endpoint_id: Option<&str>,
    ) -> Result<(Self, bool), crate::audio::AudioError> {
        match crate::audio::MicrophoneCapture::spawn(endpoint_id) {
            Ok(capture) => Ok((
                Self {
                    endpoint_id: endpoint_id.map(str::to_owned),
                    capture,
                },
                false,
            )),
            Err(error) => {
                if endpoint_id.is_none() {
                    return Err(error);
                }
                let capture = crate::audio::MicrophoneCapture::spawn(None)?;
                Ok((
                    Self {
                        endpoint_id: endpoint_id.map(str::to_owned),
                        capture,
                    },
                    true,
                ))
            }
        }
    }

    pub fn matches(&self, endpoint_id: Option<&str>) -> bool {
        self.endpoint_id.as_deref() == endpoint_id
    }

    /// Loudest sample since the last call, as a percentage of full scale, or
    /// `None` while the device has not delivered audio yet.
    pub fn peak_percent(&self) -> Option<u8> {
        let format = self.capture.format();
        let mut loudest = 0;
        let mut delivered = false;
        while let Ok(chunk) = self.capture.receiver().try_recv() {
            delivered = true;
            if let Ok(pcm) = crate::audio::normalize_to_pcm16(format, chunk) {
                loudest = loudest.max(peak_percent_of_pcm16(&pcm.data));
            }
        }
        delivered.then_some(loudest)
    }
}

#[cfg(not(target_os = "windows"))]
pub struct MicrophoneProbe;

#[cfg(not(target_os = "windows"))]
impl MicrophoneProbe {
    pub fn open(_endpoint_id: Option<&str>) -> Result<Self, crate::audio::AudioError> {
        Err(crate::audio::AudioError(
            "microphone capture is available only on Windows".into(),
        ))
    }

    pub fn matches(&self, _endpoint_id: Option<&str>) -> bool {
        true
    }

    pub fn peak_percent(&self) -> Option<u8> {
        None
    }
}

pub fn peak_percent_of_pcm16(data: &[u8]) -> u8 {
    let loudest = data
        .chunks_exact(2)
        .map(|sample| i32::from(i16::from_le_bytes([sample[0], sample[1]])).abs())
        .max()
        .unwrap_or(0);
    ((loudest * 100) / 32_767).clamp(0, 100) as u8
}

#[cfg(test)]
mod tests {
    use super::peak_percent_of_pcm16;

    #[test]
    fn silence_and_full_scale_map_to_the_ends_of_the_meter() {
        assert_eq!(peak_percent_of_pcm16(&[]), 0);
        assert_eq!(peak_percent_of_pcm16(&[0, 0, 0, 0]), 0);
        assert_eq!(peak_percent_of_pcm16(&i16::MAX.to_le_bytes()), 100);
        assert_eq!(peak_percent_of_pcm16(&i16::MIN.to_le_bytes()), 100);
    }

    #[test]
    fn the_loudest_sample_of_a_chunk_wins() {
        let mut data = Vec::new();
        for sample in [120_i16, -16_384, 900] {
            data.extend_from_slice(&sample.to_le_bytes());
        }

        assert_eq!(peak_percent_of_pcm16(&data), 50);
    }
}

#[cfg(not(target_os = "windows"))]
pub struct MicrophoneMeter;

#[cfg(not(target_os = "windows"))]
impl MicrophoneMeter {
    pub fn closed() -> Self {
        Self
    }

    pub fn is_open(&self) -> bool {
        false
    }

    pub fn matches(&self, _endpoint_id: Option<&str>) -> bool {
        true
    }

    pub fn open(_endpoint_id: Option<&str>) -> Self {
        Self
    }

    pub fn peak_percent(&mut self) -> Option<u8> {
        None
    }
}
