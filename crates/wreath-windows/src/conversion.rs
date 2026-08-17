#[cfg(target_os = "windows")]
use crate::video::VideoError;

#[cfg(target_os = "windows")]
pub struct Nv12Surface {
    pub texture: windows::Win32::Graphics::Direct3D11::ID3D11Texture2D,
    output_view: windows::Win32::Graphics::Direct3D11::ID3D11VideoProcessorOutputView,
}

#[cfg(target_os = "windows")]
pub struct GpuColorConverter {
    device: windows::Win32::Graphics::Direct3D11::ID3D11Device,
    video_device: windows::Win32::Graphics::Direct3D11::ID3D11VideoDevice,
    video_context: windows::Win32::Graphics::Direct3D11::ID3D11VideoContext,
    enumerator: windows::Win32::Graphics::Direct3D11::ID3D11VideoProcessorEnumerator,
    processor: windows::Win32::Graphics::Direct3D11::ID3D11VideoProcessor,
    width: u32,
    height: u32,
}

#[cfg(target_os = "windows")]
impl GpuColorConverter {
    pub fn initialize(
        device: &windows::Win32::Graphics::Direct3D11::ID3D11Device,
        context: &windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext,
        width: u32,
        height: u32,
        frames_per_second: u16,
    ) -> Result<Self, VideoError> {
        use windows::Win32::Graphics::Direct3D11::{
            D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE, D3D11_VIDEO_PROCESSOR_CONTENT_DESC,
            D3D11_VIDEO_USAGE_PLAYBACK_NORMAL, ID3D11VideoContext, ID3D11VideoDevice,
        };
        use windows::Win32::Graphics::Dxgi::Common::DXGI_RATIONAL;
        use windows::core::Interface;

        if width == 0 || height == 0 || width % 2 != 0 || height % 2 != 0 {
            return Err(VideoError::Initialization(
                "GPU conversion requires non-zero even dimensions".into(),
            ));
        }
        let rate = DXGI_RATIONAL {
            Numerator: u32::from(frames_per_second.max(1)),
            Denominator: 1,
        };
        let description = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
            InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
            InputFrameRate: rate,
            InputWidth: width,
            InputHeight: height,
            OutputFrameRate: rate,
            OutputWidth: width,
            OutputHeight: height,
            Usage: D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
        };
        let video_device: ID3D11VideoDevice = device.cast().map_err(initialization_error)?;
        let video_context: ID3D11VideoContext = context.cast().map_err(initialization_error)?;
        let enumerator = unsafe { video_device.CreateVideoProcessorEnumerator(&description) }
            .map_err(initialization_error)?;
        let processor = unsafe { video_device.CreateVideoProcessor(&enumerator, 0) }
            .map_err(initialization_error)?;

        Ok(Self {
            device: device.clone(),
            video_device,
            video_context,
            enumerator,
            processor,
            width,
            height,
        })
    }

    pub fn create_output_surface(&self) -> Result<Nv12Surface, VideoError> {
        use windows::Win32::Graphics::Direct3D11::{
            D3D11_BIND_RENDER_TARGET, D3D11_TEX2D_VPOV, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
            D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0,
            D3D11_VPOV_DIMENSION_TEXTURE2D,
        };
        use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_SAMPLE_DESC};

        let description = D3D11_TEXTURE2D_DESC {
            Width: self.width,
            Height: self.height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_NV12,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut texture = None;
        unsafe {
            self.device
                .CreateTexture2D(&description, None, Some(&mut texture))
        }
        .map_err(initialization_error)?;
        let texture = texture.ok_or_else(|| {
            VideoError::Initialization("D3D11 returned no NV12 output texture".into())
        })?;
        let view_description = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
            ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 },
            },
        };
        let mut output_view = None;
        unsafe {
            self.video_device.CreateVideoProcessorOutputView(
                &texture,
                &self.enumerator,
                &view_description,
                Some(&mut output_view),
            )
        }
        .map_err(initialization_error)?;

        Ok(Nv12Surface {
            texture,
            output_view: output_view.ok_or_else(|| {
                VideoError::Initialization("D3D11 returned no NV12 output view".into())
            })?,
        })
    }

    pub fn convert(
        &self,
        input: &windows::Win32::Graphics::Direct3D11::ID3D11Texture2D,
        output: &Nv12Surface,
    ) -> Result<(), VideoError> {
        use std::mem::ManuallyDrop;

        use windows::Win32::Graphics::Direct3D11::{
            D3D11_TEX2D_VPIV, D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC,
            D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0, D3D11_VIDEO_PROCESSOR_STREAM,
            D3D11_VPIV_DIMENSION_TEXTURE2D,
        };

        let input_description = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
            FourCC: 0,
            ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPIV {
                    MipSlice: 0,
                    ArraySlice: 0,
                },
            },
        };
        let mut input_view = None;
        unsafe {
            self.video_device.CreateVideoProcessorInputView(
                input,
                &self.enumerator,
                &input_description,
                Some(&mut input_view),
            )
        }
        .map_err(initialization_error)?;
        let input_view = input_view.ok_or_else(|| {
            VideoError::Initialization("D3D11 returned no BGRA input view".into())
        })?;
        let mut stream = D3D11_VIDEO_PROCESSOR_STREAM {
            Enable: true.into(),
            pInputSurface: ManuallyDrop::new(Some(input_view)),
            ..Default::default()
        };
        let result = unsafe {
            self.video_context.VideoProcessorBlt(
                &self.processor,
                &output.output_view,
                0,
                std::slice::from_ref(&stream),
            )
        };
        unsafe { ManuallyDrop::drop(&mut stream.pInputSurface) };
        result.map_err(initialization_error)
    }
}

#[cfg(target_os = "windows")]
fn initialization_error(error: windows::core::Error) -> VideoError {
    VideoError::Initialization(error.to_string())
}
