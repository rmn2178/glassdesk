//! On-disk cache for app icons and names.
//!
//! Getting these from the Windows shell takes 10–250 ms per app (loading shell
//! extensions or opening Store packages); reading them from disk takes under
//! 1 ms. Each entry has a validity stamp:
//! - files use their modification time, which changes when an app is updated;
//! - folders (whose timestamps change whenever a file is added) and shell items
//!   without files (Store apps) use the current day, so they are refreshed at
//!   most once per day.

use std::{
    env, fs,
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    icons::{IconError, IconImage, load_icon, load_icon_at},
    names::display_name,
    recycle_bin,
};

/// Changed when icons are read differently, so entries from before are read
/// again (2: programs' icons come from their own resources; 3: Store apps'
/// logos without their tile).
const MAGIC: &[u8; 4] = b"WDI3";
const HEADER_LEN: usize = 4 + 8 + 4 + 4 + 4;
const SECONDS_PER_DAY: u64 = 86_400;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppVisual {
    pub name: String,
    pub icon: IconImage,
}

#[derive(Debug)]
pub struct IconCache {
    folder: PathBuf,
}

impl Default for IconCache {
    fn default() -> Self {
        Self::new(
            PathBuf::from(env::var_os("LOCALAPPDATA").unwrap_or_default())
                .join("Nosih")
                .join("icons"),
        )
    }
}

impl IconCache {
    #[must_use]
    pub fn new(folder: PathBuf) -> Self {
        Self { folder }
    }

    /// Gets the icon (at `size` pixels) and name for `id` from the cache if
    /// valid, or from the shell otherwise, saving the result for next time.
    ///
    /// Requires COM to be initialized on the calling thread.
    pub fn get(&self, id: &str, size: u32) -> Result<AppVisual, IconError> {
        self.get_variant(id, "", size)
    }

    /// Like [`get`](Self::get), but stores each `variant` separately for items
    /// whose icon depends on their state, such as an empty or full recycle bin.
    pub fn get_variant(&self, id: &str, variant: &str, size: u32) -> Result<AppVisual, IconError> {
        // A program pinned from a shortcut Nosih keeps shows that shortcut's
        // name and icon ("Launch Hidden", not "Windows Based Script Host"),
        // as the taskbar does. Part of the key, so replacing or removing the
        // shortcut never shows a stale label.
        let kept = crate::pins::kept_shortcut_for(id);
        let source = kept.as_deref().unwrap_or(id);
        // A Store app's logo has a form for light taskbars (see
        // `icons::load_icon`): each theme keeps its own entry.
        let theme = if source.starts_with(crate::tracker::APPS_FOLDER)
            && crate::system::uses_light_theme()
        {
            "|light"
        } else {
            ""
        };
        let path = self.entry_path(&format!("{source}|{variant}{theme}"), size);
        // The recycle bin's two icons come from where Windows names them, not
        // from the shell item, which shows whichever state Explorer has drawn
        // last (issue #15).
        let bin_icon = (source == recycle_bin::PARSING_NAME)
            .then(|| recycle_bin::icon_location(variant == recycle_bin::FULL))
            .flatten();
        let stamp = bin_icon.as_ref().map_or_else(
            || stamp_of(source),
            |(file, index)| location_stamp(file, *index),
        );
        if let Some(cached) = fs::read(&path).ok().and_then(|bytes| decode(&bytes, stamp)) {
            return Ok(cached);
        }

        let icon = match &bin_icon {
            Some((file, index)) => load_icon_at(file, *index, size)?,
            None => load_icon(source, size)?,
        };
        let visual = AppVisual {
            name: display_name(source),
            icon,
        };
        // An unwritable cache only makes the next startup slower.
        let _ = fs::create_dir_all(&self.folder);
        let _ = fs::write(&path, encode(&visual, stamp));
        Ok(visual)
    }

    fn entry_path(&self, id: &str, size: u32) -> PathBuf {
        let mut hasher = DefaultHasher::new();
        id.hash(&mut hasher);
        self.folder
            .join(format!("{:016x}-{size}.bin", hasher.finish()))
    }
}

fn stamp_of(id: &str) -> u64 {
    let since_epoch = |time: SystemTime| {
        time.duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs())
    };
    match fs::metadata(Path::new(id)) {
        Ok(meta) if meta.is_file() => meta.modified().map_or(0, since_epoch),
        _ => since_epoch(SystemTime::now()) / SECONDS_PER_DAY,
    }
}

/// Validity of an icon read from `file,index`: changes when another icon is
/// chosen or the file is updated. It never equals a day number, so entries
/// cached the old way (issue #15) are read again.
fn location_stamp(file: &str, index: i32) -> u64 {
    let mut hasher = DefaultHasher::new();
    (file.to_lowercase(), index, stamp_of(file)).hash(&mut hasher);
    hasher.finish() | (1 << 63)
}

fn encode(visual: &AppVisual, stamp: u64) -> Vec<u8> {
    let name = visual.name.as_bytes();
    let mut bytes = Vec::with_capacity(HEADER_LEN + name.len() + visual.icon.bgra.len());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&stamp.to_le_bytes());
    bytes.extend_from_slice(&visual.icon.width.to_le_bytes());
    bytes.extend_from_slice(&visual.icon.height.to_le_bytes());
    bytes.extend_from_slice(&(name.len() as u32).to_le_bytes());
    bytes.extend_from_slice(name);
    bytes.extend_from_slice(&visual.icon.bgra);
    bytes
}

fn decode(bytes: &[u8], expected_stamp: u64) -> Option<AppVisual> {
    let (header, rest) = bytes.split_at_checked(HEADER_LEN)?;
    let (magic, header) = header.split_first_chunk::<4>()?;
    let (stamp, header) = header.split_first_chunk::<8>()?;
    let (width, header) = header.split_first_chunk::<4>()?;
    let (height, header) = header.split_first_chunk::<4>()?;
    let (name_len, _) = header.split_first_chunk::<4>()?;
    if magic != MAGIC || u64::from_le_bytes(*stamp) != expected_stamp {
        return None;
    }

    let (width, height) = (u32::from_le_bytes(*width), u32::from_le_bytes(*height));
    let (name, bgra) = rest.split_at_checked(u32::from_le_bytes(*name_len) as usize)?;
    let pixel_len = (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)?;
    if bgra.len() != pixel_len {
        return None;
    }
    Some(AppVisual {
        name: String::from_utf8(name.to_vec()).ok()?,
        icon: IconImage {
            width,
            height,
            bgra: bgra.to_vec(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> AppVisual {
        AppVisual {
            name: "Explorador de Windows".into(),
            icon: IconImage {
                width: 2,
                height: 2,
                bgra: (0..16).collect(),
            },
        }
    }

    #[test]
    fn decode_should_return_what_encode_stored() {
        assert_eq!(decode(&encode(&sample(), 7), 7), Some(sample()));
    }

    #[test]
    fn decode_should_reject_a_stale_entry() {
        assert_eq!(decode(&encode(&sample(), 7), 8), None);
    }

    #[test]
    fn decode_should_reject_a_truncated_entry() {
        let bytes = encode(&sample(), 7);
        assert_eq!(decode(&bytes[..bytes.len() - 1], 7), None);
    }

    #[test]
    fn decode_should_reject_another_format() {
        let mut bytes = encode(&sample(), 7);
        bytes[0] = b'X';
        assert_eq!(decode(&bytes, 7), None);
    }

    #[test]
    fn decode_should_reject_dimensions_that_overflow() {
        let mut bytes = encode(&sample(), 7);
        bytes[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        bytes[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(decode(&bytes, 7), None);
    }

    #[test]
    fn stamp_of_should_follow_file_modification_time() {
        let file = env::temp_dir().join(format!("nosih-stamp-{}", std::process::id()));
        let _ = fs::write(&file, b"x");
        let expected = fs::metadata(&file)
            .and_then(|meta| meta.modified())
            .map_or(0, |time| {
                time.duration_since(UNIX_EPOCH)
                    .map_or(0, |elapsed| elapsed.as_secs())
            });
        let stamp = stamp_of(&file.to_string_lossy());
        let _ = fs::remove_file(&file);
        assert_eq!(stamp, expected);
    }
}
