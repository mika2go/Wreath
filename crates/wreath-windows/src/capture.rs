#[cfg(target_os = "windows")]
use std::time::Duration;

#[cfg(target_os = "windows")]
use crossbeam_channel::{Receiver, TrySendError};

#[cfg(target_os = "windows")]
use windows::Foundation::{TimeSpan, TypedEventHandler};
#[cfg(target_os = "windows")]
use windows::Graphics::Capture::{
    Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11Texture2D};

#[cfg(target_os = "windows")]
use crate::video::VideoError;

#[cfg(target_os = "windows")]
pub struct CapturedFrame {
    pub texture: ID3D11Texture2D,
    pub timestamp: Duration,
    pub width: u32,
    pub height: u32,
}

#[cfg(target_os = "windows")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureInfo {
    pub monitor: String,
    pub width: u32,
    pub height: u32,
}

/// A Windows Graphics Capture session with a two-frame handoff queue.
///
/// When the encoder is busy, new frames are dropped instead of growing a queue
/// and consuming progressively more GPU memory.
#[cfg(target_os = "windows")]
pub struct MonitorCapture {
    frame_pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    frame_arrived_token: i64,
}

#[cfg(target_os = "windows")]
impl MonitorCapture {
    pub fn start_primary(
        device: &ID3D11Device,
        frames_per_second: u16,
        capture_cursor: bool,
    ) -> Result<(Self, CaptureInfo, Receiver<CapturedFrame>), VideoError> {
        use windows::Graphics::Capture::GraphicsCaptureSession;
        use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
        use windows::Graphics::DirectX::DirectXPixelFormat;
        use windows::Win32::Foundation::POINT;
        use windows::Win32::Graphics::Dxgi::IDXGIDevice;
        use windows::Win32::Graphics::Gdi::{MONITOR_DEFAULTTOPRIMARY, MonitorFromPoint};
        use windows::Win32::System::WinRT::Direct3D11::CreateDirect3D11DeviceFromDXGIDevice;
        use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
        use windows::core::Interface;

        if !GraphicsCaptureSession::IsSupported().map_err(initialization_error)? {
            return Err(VideoError::Initialization(
                "Windows Graphics Capture is not supported".into(),
            ));
        }

        let dxgi_device: IDXGIDevice = device.cast().map_err(initialization_error)?;
        let inspectable = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi_device) }
            .map_err(initialization_error)?;
        let direct3d_device: IDirect3DDevice = inspectable.cast().map_err(initialization_error)?;

        let monitor = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
        let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
            .map_err(initialization_error)?;
        let item: GraphicsCaptureItem =
            unsafe { interop.CreateForMonitor(monitor) }.map_err(initialization_error)?;
        let size = item.Size().map_err(initialization_error)?;
        if size.Width <= 0 || size.Height <= 0 {
            return Err(VideoError::Initialization(
                "primary monitor has invalid dimensions".into(),
            ));
        }

        let frame_pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &direct3d_device,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            2,
            size,
        )
        .map_err(initialization_error)?;
        let session = frame_pool
            .CreateCaptureSession(&item)
            .map_err(initialization_error)?;
        session
            .SetIsCursorCaptureEnabled(capture_cursor)
            .map_err(initialization_error)?;
        let frame_interval_ticks = 10_000_000_i64 / i64::from(frames_per_second.max(1));
        let _ = session.SetMinUpdateInterval(TimeSpan {
            Duration: frame_interval_ticks,
        });

        let (frame_sender, frame_receiver) = crossbeam_channel::bounded(2);
        let handler =
            TypedEventHandler::<Direct3D11CaptureFramePool, windows::core::IInspectable>::new(
                move |sender, _| {
                    let pool = sender.ok()?;
                    let frame = pool.TryGetNextFrame()?;
                    let content_size = frame.ContentSize()?;
                    let surface = frame.Surface()?;
                    let access: windows::Win32::System::WinRT::Direct3D11::IDirect3DDxgiInterfaceAccess =
                surface.cast()?;
                    let texture: ID3D11Texture2D = unsafe { access.GetInterface()? };
                    let relative_time = frame.SystemRelativeTime()?.Duration.max(0) as u64;
                    let captured = CapturedFrame {
                        texture,
                        timestamp: Duration::from_nanos(relative_time.saturating_mul(100)),
                        width: u32::try_from(content_size.Width).unwrap_or_default(),
                        height: u32::try_from(content_size.Height).unwrap_or_default(),
                    };
                    match frame_sender.try_send(captured) {
                        Ok(())
                        | Err(TrySendError::Full(_))
                        | Err(TrySendError::Disconnected(_)) => {}
                    }
                    Ok(())
                },
            );
        let frame_arrived_token = frame_pool
            .FrameArrived(&handler)
            .map_err(initialization_error)?;
        session.StartCapture().map_err(initialization_error)?;

        Ok((
            Self {
                frame_pool,
                session,
                frame_arrived_token,
            },
            CaptureInfo {
                monitor: "Primary display".into(),
                width: size.Width as u32,
                height: size.Height as u32,
            },
            frame_receiver,
        ))
    }
}

#[cfg(target_os = "windows")]
impl Drop for MonitorCapture {
    fn drop(&mut self) {
        let _ = self.frame_pool.RemoveFrameArrived(self.frame_arrived_token);
        let _ = self.session.Close();
        let _ = self.frame_pool.Close();
    }
}

#[cfg(target_os = "windows")]
fn initialization_error(error: windows::core::Error) -> VideoError {
    VideoError::Initialization(error.to_string())
}
