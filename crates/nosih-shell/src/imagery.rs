//! Bounded Windows image decoding for small UI previews, off the UI thread.
use crate::icons::IconImage;
use windows::Win32::{
    Graphics::Imaging::{
        CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory,
        WICBitmapDitherTypeNone, WICBitmapInterpolationModeFant, WICBitmapPaletteTypeCustom,
        WICDecodeMetadataCacheOnDemand,
    },
    System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance},
    UI::Shell::SHCreateMemStream,
};

/// Caller initializes COM. Limits are checked before decoding pixels; only a
/// downscaled BGRA buffer is retained. No URI fetching or shell extensions.
#[must_use]
pub fn decode(bytes: &[u8], side: u32, max_pixels: u64) -> Option<IconImage> {
    if bytes.is_empty() || bytes.len() > 32 * 1024 * 1024 || !(1..=512).contains(&side) {
        return None;
    }
    // SAFETY: Input bytes and all WIC objects remain alive until CopyPixels
    // completes. The destination is checked and sized for its exact stride.
    unsafe {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let stream = SHCreateMemStream(Some(bytes))?;
        let decoder = factory
            .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
            .ok()?;
        let frame = decoder.GetFrame(0).ok()?;
        let (mut width, mut height) = (0, 0);
        frame.GetSize(&mut width, &mut height).ok()?;
        if width == 0 || height == 0 || u64::from(width) * u64::from(height) > max_pixels {
            return None;
        }
        let divisor = width.max(height).max(side);
        width = ((u64::from(width) * u64::from(side)) / u64::from(divisor)).max(1) as u32;
        height = ((u64::from(height) * u64::from(side)) / u64::from(divisor)).max(1) as u32;
        let scaler = factory.CreateBitmapScaler().ok()?;
        scaler
            .Initialize(&frame, width, height, WICBitmapInterpolationModeFant)
            .ok()?;
        let converter = factory.CreateFormatConverter().ok()?;
        converter
            .Initialize(
                &scaler,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .ok()?;
        let mut bgra = vec![0; (width * height * 4) as usize];
        converter
            .CopyPixels(std::ptr::null(), width * 4, &mut bgra)
            .ok()?;
        Some(IconImage {
            width,
            height,
            bgra,
        })
    }
}

/// Center crop and antialias the edge once, before sharing the image with UI.
#[must_use]
pub fn circular(image: IconImage) -> IconImage {
    let side = image.width.min(image.height);
    let (left, top) = ((image.width - side) / 2, (image.height - side) / 2);
    let mut bgra = Vec::with_capacity((side * side * 4) as usize);
    let radius = side as f32 / 2.0;
    for y in 0..side {
        for x in 0..side {
            let offset = (((y + top) * image.width + x + left) * 4) as usize;
            let distance = (x as f32 + 0.5 - radius).hypot(y as f32 + 0.5 - radius);
            let coverage = (radius - distance + 0.5).clamp(0.0, 1.0);
            for channel in &image.bgra[offset..offset + 4] {
                bgra.push((f32::from(*channel) * coverage).round() as u8);
            }
        }
    }
    IconImage {
        width: side,
        height: side,
        bgra,
    }
}

/// Read the current monitor's wallpaper once on a blocking worker. Only the
/// small decoded preview leaves this function; its local path is not exposed.
#[must_use]
pub fn wallpaper_preview(window: isize) -> Option<IconImage> {
    use std::{fs::File, io::Read};
    use windows::{
        Win32::{
            Foundation::{HWND, RPC_E_CHANGED_MODE},
            Graphics::Gdi::{
                GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
            },
            System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize},
            UI::HiDpi::{
                DPI_AWARENESS_CONTEXT, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
                SetThreadDpiAwarenessContext,
            },
            UI::Shell::{DesktopWallpaper, IDesktopWallpaper},
        },
        core::{PCWSTR, PWSTR},
    };
    struct Apartment(bool);
    impl Drop for Apartment {
        fn drop(&mut self) {
            if self.0 {
                // SAFETY: Balances this thread's successful initialization.
                unsafe { CoUninitialize() };
            }
        }
    }
    struct Text(PWSTR);
    struct DpiContext(DPI_AWARENESS_CONTEXT);
    impl Drop for DpiContext {
        fn drop(&mut self) {
            if !self.0.0.is_null() {
                // SAFETY: Restore this worker thread's previous DPI context.
                unsafe {
                    SetThreadDpiAwarenessContext(self.0);
                }
            }
        }
    }
    impl Drop for Text {
        fn drop(&mut self) {
            // SAFETY: GetWallpaper/GetMonitorDevicePathAt allocate with COM.
            unsafe { CoTaskMemFree(Some(self.0.0.cast())) };
        }
    }
    // SAFETY: Read-only monitor/desktop queries; allocated strings and COM
    // objects are scoped inside the apartment. No shell thumbnail handlers.
    unsafe {
        let initialized = CoInitializeEx(None, COINIT_MULTITHREADED);
        if initialized.is_err() && initialized != RPC_E_CHANGED_MODE {
            return None;
        }
        let _apartment = Apartment(initialized.is_ok());
        // DesktopWallpaper always returns physical rectangles; GetMonitorInfo
        // otherwise virtualizes them on pool/test threads at e.g. 125% DPI.
        let _dpi = DpiContext(SetThreadDpiAwarenessContext(
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        ));
        let desktop: IDesktopWallpaper = CoCreateInstance(
            &DesktopWallpaper,
            None,
            windows::Win32::System::Com::CLSCTX_ALL,
        )
        .ok()?;
        let monitor = MonitorFromWindow(HWND(window as *mut _), MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return None;
        }
        let mut path = None;
        for index in 0..desktop.GetMonitorDevicePathCount().ok()?.min(64) {
            let id = Text(desktop.GetMonitorDevicePathAt(index).ok()?);
            if id.0.is_null() {
                continue;
            }
            if desktop.GetMonitorRECT(PCWSTR(id.0.0)).ok()? == info.rcMonitor {
                let wallpaper = Text(desktop.GetWallpaper(PCWSTR(id.0.0)).ok()?);
                if !wallpaper.0.is_null() {
                    path = wallpaper.0.to_string().ok();
                }
                break;
            }
        }
        let path = path.filter(|path| !path.is_empty() && !path.starts_with(r"\\"))?;
        let file = File::open(path).ok()?;
        if file.metadata().ok()?.len() > 32 * 1024 * 1024 {
            return None;
        }
        let mut bytes = Vec::new();
        file.take(32 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        decode(&bytes, 384, 40_000_000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circular_crop_preserves_the_center_and_premultiplied_alpha() {
        let image = IconImage {
            width: 12,
            height: 8,
            bgra: (0..96)
                .flat_map(|i| [(i % 12) as u8, 20, 40, 128])
                .collect(),
        };
        let circle = circular(image);
        assert_eq!((circle.width, circle.height), (8, 8));
        assert_eq!(
            &circle.bgra[(4 * 8 + 4) * 4..(4 * 8 + 4) * 4 + 4],
            &[6, 20, 40, 128]
        );
        assert_eq!(&circle.bgra[..4], &[0, 0, 0, 0]);
        assert!(
            circle
                .bgra
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| pixel[..3].iter().all(|channel| *channel <= pixel[3]))
        );
    }

    #[test]
    fn native_decoder_limits_images_and_rejects_corrupt_inputs() -> windows::core::Result<()> {
        use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
        // SAFETY: This test owns and balances this thread's COM initialization.
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        }
        let bytes = include_bytes!("../../../assets/media-fallback.png");
        let image = decode(bytes, 32, 1_000_000);
        let too_many_pixels = decode(bytes, 32, 1);
        let corrupt = decode(b"not an encoded image", 32, 1_000_000);
        unsafe {
            CoUninitialize();
        }
        let image = image.ok_or_else(|| {
            windows::core::Error::from_hresult(windows::Win32::Foundation::E_FAIL)
        })?;
        assert!(image.width <= 32 && image.height <= 32);
        assert_eq!(image.bgra.len(), (image.width * image.height * 4) as usize);
        assert!(too_many_pixels.is_none() && corrupt.is_none());
        assert!(decode(bytes, 0, 1_000_000).is_none());
        assert!(decode(bytes, 513, 1_000_000).is_none());
        Ok(())
    }

    #[test]
    #[ignore = "Read-only probe of the connected Windows desktop wallpaper"]
    fn native_wallpaper_preview_is_bounded() {
        let started = std::time::Instant::now();
        let desktop = unsafe { windows::Win32::UI::WindowsAndMessaging::GetDesktopWindow() };
        if let Some(image) = wallpaper_preview(desktop.0 as isize) {
            assert!(image.width <= 384 && image.height <= 384);
            assert_eq!(image.bgra.len(), (image.width * image.height * 4) as usize);
            eprintln!(
                "wallpaper preview: {}x{}, {:?}",
                image.width,
                image.height,
                started.elapsed()
            );
        } else {
            eprintln!(
                "No readable image wallpaper (a supported fallback), {:?}",
                started.elapsed()
            );
        }
    }
}
