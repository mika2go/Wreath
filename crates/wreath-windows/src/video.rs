use std::fmt;

use wreath_core::config::Codec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HardwareCodec {
    H264,
    Hevc,
    Av1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphicsAdapterInfo {
    pub name: String,
    pub vendor_id: u32,
    pub device_id: u32,
}

impl HardwareCodec {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::H264 => "h264",
            Self::Hevc => "hevc",
            Self::Av1 => "av1",
        }
    }
}

#[cfg(target_os = "windows")]
impl HardwareCodec {
    pub(crate) fn media_subtype(self) -> windows::core::GUID {
        use windows::Win32::Media::MediaFoundation::{
            MFVideoFormat_AV1, MFVideoFormat_H264, MFVideoFormat_HEVC,
        };

        match self {
            Self::H264 => MFVideoFormat_H264,
            Self::Hevc => MFVideoFormat_HEVC,
            Self::Av1 => MFVideoFormat_AV1,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HardwareEncoderSupport {
    pub h264: bool,
    pub hevc: bool,
    pub av1: bool,
}

impl HardwareEncoderSupport {
    pub fn select(self, requested: Codec) -> Option<HardwareCodec> {
        match requested {
            Codec::Auto => self
                .h264
                .then_some(HardwareCodec::H264)
                .or_else(|| self.hevc.then_some(HardwareCodec::Hevc))
                .or_else(|| self.av1.then_some(HardwareCodec::Av1)),
            Codec::H264 => self.h264.then_some(HardwareCodec::H264),
            Codec::Hevc => self.hevc.then_some(HardwareCodec::Hevc),
            Codec::Av1 => self.av1.then_some(HardwareCodec::Av1),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VideoError {
    Initialization(String),
    NoHardwareEncoder(Codec),
}

impl fmt::Display for VideoError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Initialization(message) => {
                write!(formatter, "Windows video initialization failed: {message}")
            }
            Self::NoHardwareEncoder(codec) => write!(
                formatter,
                "no Windows hardware encoder is available for {codec:?}; CPU fallback is disabled"
            ),
        }
    }
}

impl std::error::Error for VideoError {}

#[cfg(target_os = "windows")]
pub struct VideoRuntime {
    _com: ComRuntime,
    _media_foundation: MediaFoundationRuntime,
    device: windows::Win32::Graphics::Direct3D11::ID3D11Device,
    context: windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext,
    adapter: GraphicsAdapterInfo,
    support: HardwareEncoderSupport,
}

#[cfg(target_os = "windows")]
impl VideoRuntime {
    pub fn initialize() -> Result<Self, VideoError> {
        use windows::Win32::Foundation::HMODULE;
        use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
        use windows::Win32::Graphics::Direct3D11::{
            D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION,
            D3D11CreateDevice,
        };
        use windows::Win32::Graphics::Dxgi::IDXGIAdapter;

        let com = ComRuntime::initialize()?;
        let media_foundation = MediaFoundationRuntime::initialize()?;
        let mut device = None;
        let mut context = None;
        unsafe {
            D3D11CreateDevice(
                None::<&IDXGIAdapter>,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        }
        .map_err(|error| VideoError::Initialization(error.to_string()))?;
        let device =
            device.ok_or_else(|| VideoError::Initialization("D3D11 returned no device".into()))?;
        let context = context.ok_or_else(|| {
            VideoError::Initialization("D3D11 returned no immediate context".into())
        })?;
        let adapter = graphics_adapter_info(&device)?;
        let support = query_hardware_encoder_support()?;

        Ok(Self {
            _com: com,
            _media_foundation: media_foundation,
            device,
            context,
            adapter,
            support,
        })
    }

    pub fn select_encoder(&self, requested: Codec) -> Result<HardwareCodec, VideoError> {
        self.support
            .select(requested)
            .ok_or(VideoError::NoHardwareEncoder(requested))
    }

    pub fn support(&self) -> HardwareEncoderSupport {
        self.support
    }

    pub fn adapter(&self) -> &GraphicsAdapterInfo {
        &self.adapter
    }

    pub fn device(&self) -> &windows::Win32::Graphics::Direct3D11::ID3D11Device {
        &self.device
    }

    pub fn context(&self) -> &windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext {
        &self.context
    }
}

#[cfg(target_os = "windows")]
fn graphics_adapter_info(
    device: &windows::Win32::Graphics::Direct3D11::ID3D11Device,
) -> Result<GraphicsAdapterInfo, VideoError> {
    use windows::Win32::Graphics::Dxgi::{IDXGIAdapter1, IDXGIDevice};
    use windows::core::Interface;

    let dxgi_device: IDXGIDevice = device
        .cast()
        .map_err(|error| VideoError::Initialization(error.to_string()))?;
    let adapter = unsafe { dxgi_device.GetAdapter() }
        .map_err(|error| VideoError::Initialization(error.to_string()))?;
    let adapter: IDXGIAdapter1 = adapter
        .cast()
        .map_err(|error| VideoError::Initialization(error.to_string()))?;
    let description = unsafe { adapter.GetDesc1() }
        .map_err(|error| VideoError::Initialization(error.to_string()))?;
    let name_end = description
        .Description
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(description.Description.len());
    let name = String::from_utf16_lossy(&description.Description[..name_end]);
    if name.trim().is_empty() {
        return Err(VideoError::Initialization(
            "D3D11 returned an unnamed graphics adapter".into(),
        ));
    }
    Ok(GraphicsAdapterInfo {
        name,
        vendor_id: description.VendorId,
        device_id: description.DeviceId,
    })
}

#[cfg(target_os = "windows")]
struct ComRuntime;

#[cfg(target_os = "windows")]
impl ComRuntime {
    fn initialize() -> Result<Self, VideoError> {
        use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};

        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .ok()
            .map_err(|error| VideoError::Initialization(error.to_string()))?;
        Ok(Self)
    }
}

#[cfg(target_os = "windows")]
impl Drop for ComRuntime {
    fn drop(&mut self) {
        unsafe { windows::Win32::System::Com::CoUninitialize() };
    }
}

#[cfg(target_os = "windows")]
struct MediaFoundationRuntime;

#[cfg(target_os = "windows")]
impl MediaFoundationRuntime {
    fn initialize() -> Result<Self, VideoError> {
        use windows::Win32::Media::MediaFoundation::{MF_VERSION, MFSTARTUP_FULL, MFStartup};

        unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) }
            .map_err(|error| VideoError::Initialization(error.to_string()))?;
        Ok(Self)
    }
}

#[cfg(target_os = "windows")]
impl Drop for MediaFoundationRuntime {
    fn drop(&mut self) {
        let _ = unsafe { windows::Win32::Media::MediaFoundation::MFShutdown() };
    }
}

#[cfg(target_os = "windows")]
fn query_hardware_encoder_support() -> Result<HardwareEncoderSupport, VideoError> {
    use windows::Win32::Media::MediaFoundation::{
        MFVideoFormat_AV1, MFVideoFormat_H264, MFVideoFormat_HEVC,
    };

    Ok(HardwareEncoderSupport {
        h264: has_hardware_encoder(MFVideoFormat_H264)?,
        hevc: has_hardware_encoder(MFVideoFormat_HEVC)?,
        av1: has_hardware_encoder(MFVideoFormat_AV1)?,
    })
}

#[cfg(target_os = "windows")]
fn has_hardware_encoder(output_subtype: windows::core::GUID) -> Result<bool, VideoError> {
    Ok(!hardware_encoder_activations(output_subtype)?.is_empty())
}

#[cfg(target_os = "windows")]
pub(crate) fn hardware_encoder_activations(
    output_subtype: windows::core::GUID,
) -> Result<Vec<windows::Win32::Media::MediaFoundation::IMFActivate>, VideoError> {
    use std::ptr;

    use windows::Win32::Media::MediaFoundation::{
        IMFActivate, MFMediaType_Video, MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG_HARDWARE,
        MFT_ENUM_FLAG_SORTANDFILTER, MFT_REGISTER_TYPE_INFO, MFTEnumEx, MFVideoFormat_NV12,
    };
    use windows::Win32::System::Com::CoTaskMemFree;

    let input = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_NV12,
    };
    let output = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: output_subtype,
    };
    let mut activations: *mut Option<IMFActivate> = ptr::null_mut();
    let mut count = 0_u32;
    unsafe {
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_ENCODER,
            MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER,
            Some(&input),
            Some(&output),
            &mut activations,
            &mut count,
        )
    }
    .map_err(|error| VideoError::Initialization(error.to_string()))?;

    let mut results = Vec::with_capacity(count as usize);
    if !activations.is_null() {
        let activation_slice =
            unsafe { std::slice::from_raw_parts_mut(activations, count as usize) };
        for activation in activation_slice {
            if let Some(activation) = activation.take() {
                results.push(activation);
            }
        }
        unsafe { CoTaskMemFree(Some(activations.cast())) };
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_codec_prefers_compatible_h264_hardware() {
        let support = HardwareEncoderSupport {
            h264: true,
            hevc: true,
            av1: true,
        };

        assert_eq!(support.select(Codec::Auto), Some(HardwareCodec::H264));
    }

    #[test]
    fn explicit_codec_never_falls_back_to_another_encoder() {
        let support = HardwareEncoderSupport {
            h264: true,
            hevc: false,
            av1: true,
        };

        assert_eq!(support.select(Codec::Hevc), None);
        assert_eq!(support.select(Codec::Av1), Some(HardwareCodec::Av1));
    }

    #[test]
    fn empty_support_never_selects_a_cpu_fallback() {
        assert_eq!(HardwareEncoderSupport::default().select(Codec::Auto), None);
    }

    #[test]
    fn hardware_codec_names_are_stable_evidence_values() {
        assert_eq!(HardwareCodec::H264.as_str(), "h264");
        assert_eq!(HardwareCodec::Hevc.as_str(), "hevc");
        assert_eq!(HardwareCodec::Av1.as_str(), "av1");
    }
}
