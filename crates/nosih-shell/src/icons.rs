use std::{fs, mem::size_of, path::Path};

use thiserror::Error;
use windows::{
    Win32::{
        Foundation::{PROPERTYKEY, S_OK, SIZE},
        Graphics::Gdi::{
            BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject, GetDC,
            GetDIBits, GetObjectW, HBITMAP, ReleaseDC,
        },
        UI::{
            Shell::{
                IShellItem, IShellItem2, IShellItemImageFactory, SHCreateItemFromParsingName,
                SHDefExtractIconW, SIIGBF_ICONONLY,
            },
            WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO},
        },
    },
    core::{GUID, HSTRING, Interface},
};

#[derive(Debug, Error)]
pub enum IconError {
    #[error("el shell no pudo generar el icono de `{name}`")]
    Shell {
        name: String,
        #[source]
        source: windows::core::Error,
    },
    #[error("no se pudieron leer los píxeles del icono de `{0}`")]
    Pixels(String),
}

/// Square BGRA icon with premultiplied alpha and top-to-bottom rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IconImage {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

struct OwnedBitmap(HBITMAP);

impl Drop for OwnedBitmap {
    fn drop(&mut self) {
        // SAFETY: This HBITMAP is owned by us (returned by GetImage or
        // GetIconInfo) and is not used afterward.
        unsafe {
            let _ = DeleteObject(self.0.into());
        }
    }
}

struct OwnedIcon(HICON);

impl Drop for OwnedIcon {
    fn drop(&mut self) {
        // SAFETY: This HICON is owned by us (extracted from a file) and is not used afterward.
        unsafe {
            let _ = DestroyIcon(self.0);
        }
    }
}

pub fn load_icon(parsing_name: &str, size: u32) -> Result<IconImage, IconError> {
    // A program's icon comes from its own resources, as on the taskbar. The
    // shell's image factory gives a blank page instead for some valid icons:
    // one whose small sizes are PNG images, as some Electron tools write.
    if crate::tracker::is_program(parsing_name)
        && let Some(icon) = program_icon(parsing_name, size)
    {
        return Ok(icon);
    }
    // A Store app's logo as its taskbar button shows it, without its tile.
    if let Some(icon) = packaged_icon(parsing_name, size, crate::system::uses_light_theme()) {
        return Ok(icon);
    }

    let shell_error = |source| IconError::Shell {
        name: parsing_name.to_owned(),
        source,
    };
    let side = size as i32;

    // SAFETY: The strings and structures passed to each call remain alive for its duration.
    let bitmap = unsafe {
        let factory: IShellItemImageFactory =
            SHCreateItemFromParsingName(&HSTRING::from(parsing_name), None).map_err(shell_error)?;
        OwnedBitmap(
            factory
                .GetImage(SIZE { cx: side, cy: side }, SIIGBF_ICONONLY)
                .map_err(shell_error)?,
        )
    };

    read_pixels(&bitmap).ok_or_else(|| IconError::Pixels(parsing_name.to_owned()))
}

/// `System.Tile.SmallLogoPath`: a Store app's small logo, relative to its
/// package folder (`Assets\\AppList.png`).
const SMALL_LOGO: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x86d4_0b4d_9069_443c_819a_2a54_090d_ccec),
    pid: 2,
};
/// `System.AppUserModel.PackageInstallPath`: a Store app's package folder.
const PACKAGE_FOLDER: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x9f4c_2855_9f79_4b39_a8d0_e1d4_2de1_d5f3),
    pid: 15,
};

/// A Store app's logo without the tile behind it, as its taskbar button
/// shows it (issue #73): the package's `<logo>.targetsize-N_altform-unplated
/// .png`, its `lightunplated` form on a light taskbar. Windows 10's shell
/// gives the Start tile instead, the logo on a square of the app's colour.
/// `None` when the package has no such logo; the shell's image is used then.
fn packaged_icon(parsing_name: &str, size: u32, light: bool) -> Option<IconImage> {
    parsing_name.strip_prefix(crate::tracker::APPS_FOLDER)?;
    // SAFETY: Shell item and property reads; the strings are freed by the
    // helper below.
    let (folder, logo) = unsafe {
        let item: IShellItem2 =
            SHCreateItemFromParsingName(&HSTRING::from(parsing_name), None).ok()?;
        (
            string_property(&item, &PACKAGE_FOLDER)?,
            string_property(&item, &SMALL_LOGO)?,
        )
    };
    let logo = Path::new(&folder).join(logo);
    let names: Vec<String> = fs::read_dir(logo.parent()?)
        .ok()?
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .collect();
    let stem = logo.file_stem()?.to_str()?;
    let chosen = light
        .then(|| unplated_logo(&names, stem, "lightunplated", size))
        .flatten()
        .or_else(|| unplated_logo(&names, stem, "unplated", size))?;
    let bytes = fs::read(logo.with_file_name(chosen)).ok()?;
    crate::imagery::decode(&bytes, size, 4096 * 4096)
}

/// Of `names` (files beside the logo), the `form` of `stem` closest to `size`:
/// the smallest at least as large, or else the largest.
fn unplated_logo<'a>(names: &'a [String], stem: &str, form: &str, size: u32) -> Option<&'a String> {
    let prefix = format!("{}.targetsize-", stem.to_lowercase());
    let suffix = format!("_altform-{form}.png");
    let sized = names.iter().filter_map(|name| {
        let lower = name.to_lowercase();
        let target: u32 = lower
            .strip_prefix(&prefix)?
            .strip_suffix(&suffix)?
            .parse()
            .ok()?;
        Some((target, name))
    });
    let (larger, smaller): (Vec<_>, Vec<_>) = sized.partition(|(target, _)| *target >= size);
    larger
        .into_iter()
        .min_by_key(|(target, _)| *target)
        .or_else(|| smaller.into_iter().max_by_key(|(target, _)| *target))
        .map(|(_, name)| name)
}

/// A string property of a shell item.
///
/// # Safety
/// `item` must be a live shell item.
unsafe fn string_property(item: &IShellItem2, key: &PROPERTYKEY) -> Option<String> {
    // SAFETY: The returned string is allocated by COM and freed here.
    unsafe {
        let value = item.GetString(key).ok()?;
        let text = value.to_string().ok();
        windows::Win32::System::Com::CoTaskMemFree(Some(value.0.cast()));
        text.filter(|text| !text.is_empty())
    }
}

/// The icon of a shell item (a file, a folder, a known folder) at `size`
/// pixels, as Explorer shows it.
#[must_use]
pub fn item_icon(item: &IShellItem, size: u32) -> Option<IconImage> {
    let side = size as i32;
    // SAFETY: The factory is the item's own; the bitmap is owned from here.
    let bitmap = unsafe {
        let factory: IShellItemImageFactory = item.cast().ok()?;
        OwnedBitmap(
            factory
                .GetImage(SIZE { cx: side, cy: side }, SIIGBF_ICONONLY)
                .ok()?,
        )
    };
    read_pixels(&bitmap)
}

/// The first icon in a program's resources at `size` pixels; `None` when the
/// program has none, and the shell gives Windows' generic program icon.
fn program_icon(path: &str, size: u32) -> Option<IconImage> {
    icon_at(path, 0, size)
}

/// The icon at an icon location (`file,index`, as the registry writes them)
/// at `size` pixels. A negative `index` is a resource id, as in `DefaultIcon`.
pub fn load_icon_at(path: &str, index: i32, size: u32) -> Result<IconImage, IconError> {
    icon_at(path, index, size).ok_or_else(|| IconError::Pixels(format!("{path},{index}")))
}

fn icon_at(path: &str, index: i32, size: u32) -> Option<IconImage> {
    let mut icon = HICON::default();
    // SAFETY: The path lives for the call, which writes one icon handle we own.
    let result =
        unsafe { SHDefExtractIconW(&HSTRING::from(path), index, 0, Some(&mut icon), None, size) };
    // S_FALSE: the file has no icon.
    if result != S_OK || icon.is_invalid() {
        return None;
    }
    let icon = OwnedIcon(icon);
    icon_image(icon.0)
}

/// The pixels of `icon`, premultiplied; `None` for a monochrome icon. The
/// caller keeps the icon and destroys it.
#[must_use]
pub fn icon_image(icon: HICON) -> Option<IconImage> {
    let mut info = ICONINFO::default();
    // SAFETY: The caller passes a valid icon; GetIconInfo creates two
    // bitmaps, owned below.
    unsafe { GetIconInfo(icon, &mut info) }.ok()?;
    let (color, mask) = (OwnedBitmap(info.hbmColor), OwnedBitmap(info.hbmMask));
    // A monochrome icon has no color bitmap: left to the shell.
    if color.0.is_invalid() {
        return None;
    }
    let (width, height, mut bgra) = read_bits(&color)?;
    if bgra.as_chunks::<4>().0.iter().any(|px| px[3] != 0) {
        premultiply(&mut bgra);
    } else {
        // An icon without alpha: its mask tells which pixels are transparent.
        let (_, _, mask) = read_bits(&mask)?;
        if mask.len() != bgra.len() {
            return None;
        }
        for (px, bit) in bgra
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(mask.as_chunks::<4>().0)
        {
            if bit[0] == 0 {
                px[3] = 255;
            } else {
                *px = [0; 4];
            }
        }
    }
    Some(IconImage {
        width,
        height,
        bgra,
    })
}

/// Icons keep straight alpha; the dock draws premultiplied pixels.
fn premultiply(bgra: &mut [u8]) {
    for px in bgra.as_chunks_mut::<4>().0 {
        let alpha = u16::from(px[3]);
        for channel in &mut px[..3] {
            *channel = ((u16::from(*channel) * alpha + 127) / 255) as u8;
        }
    }
}

fn read_pixels(bitmap: &OwnedBitmap) -> Option<IconImage> {
    let (width, height, mut bgra) = read_bits(bitmap)?;
    // Legacy icons without an alpha channel have an alpha value of 0 for every pixel.
    if bgra.as_chunks::<4>().0.iter().all(|px| px[3] == 0) {
        bgra.as_chunks_mut::<4>()
            .0
            .iter_mut()
            .for_each(|px| px[3] = 255);
    }
    Some(IconImage {
        width,
        height,
        bgra,
    })
}

/// The bitmap's pixels as 32-bit BGRA, rows from top to bottom.
fn read_bits(bitmap: &OwnedBitmap) -> Option<(u32, u32, Vec<u8>)> {
    let mut desc = BITMAP::default();
    // SAFETY: `desc` has the size specified in its size field.
    let written = unsafe {
        GetObjectW(
            bitmap.0.into(),
            size_of::<BITMAP>() as i32,
            Some((&raw mut desc).cast()),
        )
    };
    if written == 0 || desc.bmWidth <= 0 || desc.bmHeight <= 0 {
        return None;
    }
    let (width, height) = (desc.bmWidth as u32, desc.bmHeight as u32);

    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: desc.bmWidth,
            // A negative height specifies top-down rows.
            biHeight: -desc.bmHeight,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bgra = vec![0u8; (width * height * 4) as usize];
    // SAFETY: The buffer has `width * height * 4` bytes, the amount GetDIBits writes at 32 bpp.
    let lines = unsafe {
        let dc = GetDC(None);
        let lines = GetDIBits(
            dc,
            bitmap.0,
            0,
            height,
            Some(bgra.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        );
        ReleaseDC(None, dc);
        lines
    };
    (lines != 0).then_some((width, height, bgra))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_unplated_logo_closest_to_the_size_is_chosen() {
        let names: Vec<String> = [
            "AppList.png",
            "AppList.scale-200.png",
            "AppList.targetsize-16_altform-unplated.png",
            "AppList.targetsize-48_altform-unplated.png",
            "AppList.targetsize-256_altform-unplated.png",
            "AppList.targetsize-256_altform-unplated_contrast-black.png",
            "AppList.targetsize-48_altform-lightunplated.png",
            "applist.targetsize-96_altform-unplated.png",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        let pick = |form: &str, size: u32| {
            unplated_logo(&names, "AppList", form, size).map(String::as_str)
        };
        // The smallest at least as large; names compare without case.
        assert_eq!(
            pick("unplated", 64),
            Some("applist.targetsize-96_altform-unplated.png")
        );
        assert_eq!(
            pick("unplated", 48),
            Some("AppList.targetsize-48_altform-unplated.png")
        );
        // None large enough: the largest; contrast variants never.
        assert_eq!(
            pick("unplated", 512),
            Some("AppList.targetsize-256_altform-unplated.png")
        );
        assert_eq!(
            pick("lightunplated", 96),
            Some("AppList.targetsize-48_altform-lightunplated.png")
        );
        // A package without that form: the shell's image instead.
        assert_eq!(unplated_logo(&names, "Other", "unplated", 48), None);
    }

    #[test]
    fn premultiply_should_scale_colors_by_alpha() {
        let mut bgra = [200, 100, 50, 128, 10, 20, 30, 255, 90, 90, 90, 0];
        premultiply(&mut bgra);
        assert_eq!(bgra, [100, 50, 25, 128, 10, 20, 30, 255, 0, 0, 0, 0]);
    }

    #[test]
    #[ignore = "Reads a real program's icon; run explicitly"]
    fn program_icon_should_read_an_exe_icon_with_alpha() {
        let notepad = format!(
            r"{}\System32\notepad.exe",
            std::env::var("WINDIR").unwrap_or_default()
        );
        let icon = program_icon(&notepad, 256);
        assert_eq!(
            icon.as_ref().map(|icon| (icon.width, icon.height)),
            Some((256, 256))
        );
        assert!(icon.is_some_and(|icon| icon.bgra.as_chunks::<4>().0.iter().any(|px| px[3] == 0)));
    }
}
