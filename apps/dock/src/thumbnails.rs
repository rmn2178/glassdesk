//! Live thumbnails of minimized windows, composed by DWM into the dock's
//! visual window (the same mechanism as taskbar previews). They cost DWM work,
//! not dock memory.

use std::collections::HashMap;

use windows::Win32::{
    Foundation::{HWND, RECT, SIZE},
    Graphics::Dwm::{
        DWM_THUMBNAIL_PROPERTIES, DWM_TNP_OPACITY, DWM_TNP_RECTDESTINATION,
        DWM_TNP_SOURCECLIENTAREAONLY, DWM_TNP_VISIBLE, DwmQueryThumbnailSourceSize,
        DwmRegisterThumbnail, DwmUnregisterThumbnail, DwmUpdateThumbnailProperties,
    },
};

/// Fraction of the slot left empty around the thumbnail.
const INSET: f32 = 0.06;

struct Thumbnail {
    handle: isize,
    source: SIZE,
    /// Last destination applied, to skip redundant DWM calls.
    placed: Option<(RECT, u8)>,
}

impl Drop for Thumbnail {
    fn drop(&mut self) {
        // SAFETY: `handle` came from DwmRegisterThumbnail and is released once.
        unsafe {
            let _ = DwmUnregisterThumbnail(self.handle);
        }
    }
}

pub struct Thumbnails {
    destination: HWND,
    live: HashMap<isize, Thumbnail>,
}

impl Thumbnails {
    pub fn new(destination: HWND) -> Self {
        Self {
            destination,
            live: HashMap::new(),
        }
    }

    /// Keeps one thumbnail per window in `windows`, releasing the others.
    pub fn sync(&mut self, windows: &[isize]) {
        self.live.retain(|hwnd, _| windows.contains(hwnd));
        for &hwnd in windows {
            if let Some(thumbnail) = self.live.get_mut(&hwnd) {
                // Revalidate on window events, never on the animation path.
                if let Ok(source) = unsafe { DwmQueryThumbnailSourceSize(thumbnail.handle) } {
                    thumbnail.source = source;
                }
                continue;
            }
            // SAFETY: Both windows exist; a source that just closed makes the call fail.
            if let Ok(handle) =
                unsafe { DwmRegisterThumbnail(self.destination, HWND(hwnd as *mut _)) }
            {
                self.live.insert(
                    hwnd,
                    Thumbnail {
                        handle,
                        // SAFETY: The thumbnail was registered just above.
                        source: unsafe { DwmQueryThumbnailSourceSize(handle) }.unwrap_or_default(),
                        placed: None,
                    },
                );
            }
        }
    }

    /// Fits the window's thumbnail, keeping its aspect ratio, inside `slot`
    /// (client coordinates of the destination window). DWM draws thumbnails
    /// above all of the window's own content, so the fraction `corner` of the
    /// slot's bottom-right corner is left free for the app badge. `opacity` 0 hides it.
    pub fn place(&mut self, hwnd: isize, slot: (f32, f32, f32, f32), corner: f32, opacity: u8) {
        let Some(thumbnail) = self.live.get_mut(&hwnd) else {
            return;
        };
        let (x, y, width, height) = slot;
        let fit = |width, height| thumbnail.fit(width, height);
        let (inner_width, inner_height) =
            (width * (1.0 - 2.0 * INSET), height * (1.0 - 2.0 * INSET));
        // Either the band above the badge or the band to its left, whichever
        // gives the larger thumbnail.
        let above = fit(inner_width, height * (1.0 - corner - INSET));
        let beside = fit(width * (1.0 - corner - INSET), inner_height);
        let use_beside = corner > 0.0 && beside.0 * beside.1 > above.0 * above.1;
        let (fit_width, fit_height) = if corner <= 0.0 {
            fit(inner_width, inner_height)
        } else if use_beside {
            beside
        } else {
            above
        };
        // Centered horizontally unless it sits beside the badge.
        let left = if use_beside {
            x + width * INSET
        } else {
            x + width * INSET + (inner_width - fit_width) / 2.0
        };
        let top = y + height * INSET;
        let destination = RECT {
            left: left.round() as i32,
            top: top.round() as i32,
            right: (left + fit_width).round() as i32,
            bottom: (top + fit_height).round() as i32,
        };
        thumbnail.update(destination, opacity);
    }

    /// Fits the preview with its bottom edge on the caption's common baseline.
    /// Returns the actual DWM bounds so the hover outline can follow the image.
    pub fn place_preview(&mut self, hwnd: isize, slot: RECT, opacity: u8) -> Option<RECT> {
        let thumbnail = self.live.get_mut(&hwnd)?;
        let width = (slot.right - slot.left) as f32;
        let (w, h) = thumbnail.fit(width, (slot.bottom - slot.top) as f32);
        let w = w.round().max(1.0) as i32;
        let h = h.round().max(1.0) as i32;
        let left = slot.left + ((slot.right - slot.left) - w) / 2;
        let destination = RECT {
            left,
            top: slot.bottom - h,
            right: left + w,
            bottom: slot.bottom,
        };
        thumbnail
            .update(destination, opacity)
            .then_some(destination)
    }

    #[cfg(test)]
    pub(crate) fn placed_bounds(&self, hwnd: isize) -> Option<RECT> {
        self.live.get(&hwnd)?.placed.map(|(bounds, _)| bounds)
    }
}

impl Thumbnail {
    fn fit(&self, width: f32, height: f32) -> (f32, f32) {
        let source = self.source;
        let ratio = if source.cx > 0 && source.cy > 0 {
            source.cx as f32 / source.cy as f32
        } else {
            1.0
        };
        if ratio >= width / height {
            (width, width / ratio)
        } else {
            (height * ratio, height)
        }
    }

    fn update(&mut self, destination: RECT, opacity: u8) -> bool {
        if self.placed == Some((destination, opacity)) {
            return true;
        }

        let properties = DWM_THUMBNAIL_PROPERTIES {
            dwFlags: DWM_TNP_RECTDESTINATION
                | DWM_TNP_VISIBLE
                | DWM_TNP_OPACITY
                | DWM_TNP_SOURCECLIENTAREAONLY,
            rcDestination: destination,
            opacity,
            fVisible: (opacity > 0).into(),
            fSourceClientAreaOnly: false.into(),
            ..Default::default()
        };
        // SAFETY: Valid handle and properties structure.
        if unsafe { DwmUpdateThumbnailProperties(self.handle, &properties) }.is_err() {
            return false;
        }
        self.placed = Some((destination, opacity));
        true
    }
}
