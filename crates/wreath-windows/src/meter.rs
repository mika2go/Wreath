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
