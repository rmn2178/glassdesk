//! Is Nosih visible to Windows.Graphics.Capture, the API that the Snipping
//! Tool and most screen recorders use? Captures one frame of the primary
//! monitor and compares a row of pixels with what GDI sees there.
//! Usage: `spike_wgc [y]` (default: row 10, inside the menu bar).

use std::time::{Duration, Instant};

use nosih_shell::screen;
use windows::{
    Foundation::TypedEventHandler,
    Graphics::{
        Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem},
        DirectX::DirectXPixelFormat,
    },
    Win32::{
        Foundation::{HMODULE, POINT, RECT},
        Graphics::{
            Direct3D::D3D_DRIVER_TYPE_HARDWARE,
            Direct3D11::{
                D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
                D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
                D3D11_USAGE_STAGING, D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext,
                ID3D11Texture2D,
            },
            Dxgi::IDXGIDevice,
            Gdi::{MONITOR_DEFAULTTOPRIMARY, MonitorFromPoint},
        },
        System::WinRT::{
            Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess},
            Graphics::Capture::IGraphicsCaptureItemInterop,
            RO_INIT_MULTITHREADED, RoInitialize,
        },
        UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext},
    },
    core::{Interface, Result, factory},
};

fn main() -> Result<()> {
    let y: i32 = std::env::args()
        .nth(1)
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        RoInitialize(RO_INIT_MULTITHREADED)?;
        let mut d3d: Option<ID3D11Device> = None;
        let mut context: Option<ID3D11DeviceContext> = None;
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut d3d),
            None,
            Some(&mut context),
        )?;
        let (Some(d3d), Some(context)) = (d3d, context) else {
            return Ok(());
        };
        let device: windows::Graphics::DirectX::Direct3D11::IDirect3DDevice =
            CreateDirect3D11DeviceFromDXGIDevice(&d3d.cast::<IDXGIDevice>()?)?.cast()?;

        let monitor = MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTOPRIMARY);
        let interop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        let item: GraphicsCaptureItem = interop.CreateForMonitor(monitor)?;
        let size = item.Size()?;
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &device,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            1,
            size,
        )?;
        let (sender, receiver) = std::sync::mpsc::channel();
        pool.FrameArrived(&TypedEventHandler::new(
            move |pool: windows::core::Ref<Direct3D11CaptureFramePool>, _| {
                if let Some(pool) = pool.as_ref()
                    && let Ok(frame) = pool.TryGetNextFrame()
                {
                    let _ = sender.send(frame);
                }
                Ok(())
            },
        ))?;
        let session = pool.CreateCaptureSession(&item)?;
        session.StartCapture()?;
        // Skip the first frames, which can predate the capture border.
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut frame = None;
        let mut count = 0;
        while Instant::now() < deadline && count < 4 {
            if let Ok(f) = receiver.recv_timeout(Duration::from_millis(500)) {
                frame = Some(f);
                count += 1;
            }
        }
        session.Close()?;
        let Some(frame) = frame else {
            println!("no frame");
            return Ok(());
        };
        let texture: ID3D11Texture2D = frame
            .Surface()?
            .cast::<IDirect3DDxgiInterfaceAccess>()?
            .GetInterface()?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        texture.GetDesc(&mut desc);
        desc.Usage = D3D11_USAGE_STAGING;
        desc.BindFlags = 0;
        desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        desc.MiscFlags = 0;
        let mut staging: Option<ID3D11Texture2D> = None;
        d3d.CreateTexture2D(&desc, None, Some(&mut staging))?;
        let Some(staging) = staging else {
            return Ok(());
        };
        context.CopyResource(&staging, &texture);
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
        let row = std::slice::from_raw_parts(
            (mapped.pData as *const u8).add(mapped.RowPitch as usize * y as usize),
            desc.Width as usize * 4,
        )
        .to_vec();
        context.Unmap(&staging, 0);

        let Some(gdi) = screen::capture(RECT {
            left: 0,
            top: y,
            right: desc.Width as i32,
            bottom: y + 1,
        }) else {
            return Ok(());
        };
        let mut differ = 0;
        let samples = [100usize, 400, 960, 1500, 1700, 1850];
        for x in samples {
            let w = &row[x * 4..x * 4 + 3];
            let g = &gdi.bgra[x * 4..x * 4 + 3];
            println!("x={x:4}  capture api bgr={w:?}  gdi bgr={g:?}");
        }
        for x in 0..desc.Width as usize {
            let d: i32 = (0..3)
                .map(|c| (row[x * 4 + c] as i32 - gdi.bgra[x * 4 + c] as i32).abs())
                .sum();
            if d > 40 {
                differ += 1;
            }
        }
        println!(
            "row {y}: {differ} of {} pixels differ between the two captures",
            desc.Width
        );
    }
    Ok(())
}
