//! Copying a rectangle of the composed desktop.

use std::{ffi::c_void, mem::size_of};

use windows::Win32::{
    Foundation::RECT,
    Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleDC,
        CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, ReleaseDC, SRCCOPY,
        SelectObject,
    },
};

use crate::icons::IconImage;

/// Captures `rect` (screen coordinates) as opaque BGRA pixels, top row first.
#[must_use]
pub fn capture(rect: RECT) -> Option<IconImage> {
    let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
    if width <= 0 || height <= 0 {
        return None;
    }
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            // Negative height: top-down rows.
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    // SAFETY: Every GDI object created here is released before returning, and
    // `bits` points to `width * height * 4` bytes owned by the DIB section
    // while it is alive.
    unsafe {
        let screen = GetDC(None);
        let memory = CreateCompatibleDC(Some(screen));
        let mut bits: *mut c_void = std::ptr::null_mut();
        let bitmap = CreateDIBSection(Some(screen), &info, DIB_RGB_COLORS, &mut bits, None, 0);
        let image = match bitmap {
            Ok(bitmap) if !bits.is_null() => {
                let previous = SelectObject(memory, bitmap.into());
                let copied = BitBlt(
                    memory,
                    0,
                    0,
                    width,
                    height,
                    Some(screen),
                    rect.left,
                    rect.top,
                    SRCCOPY | CAPTUREBLT,
                );
                let image = copied.is_ok().then(|| {
                    let len = (width * height * 4) as usize;
                    let mut bgra = std::slice::from_raw_parts(bits.cast::<u8>(), len).to_vec();
                    // GDI leaves the alpha byte undefined.
                    bgra.as_chunks_mut::<4>()
                        .0
                        .iter_mut()
                        .for_each(|pixel| pixel[3] = 255);
                    IconImage {
                        width: width as u32,
                        height: height as u32,
                        bgra,
                    }
                });
                SelectObject(memory, previous);
                let _ = DeleteObject(bitmap.into());
                image
            }
            _ => None,
        };
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
        image
    }
}
