//! Integrity metadata only. Full-resolution JPEGs are never embedded.
use std::sync::OnceLock;

use serde::Deserialize;

pub(super) const MAX_BYTES: u64 = 32 * 1024 * 1024;
pub(super) const ORIGIN: &str =
    "https://raw.githubusercontent.com/nosih-app/nosih/main/apps/settings/wallpapers";

#[derive(Deserialize)]
pub(super) struct Wallpaper {
    pub id: u8,
    pub name: String,
    pub bytes: u64,
    pub sha256: String,
    width: u32,
    height: u32,
}

pub(super) fn all() -> Result<&'static [Wallpaper], String> {
    static CATALOG: OnceLock<Result<Vec<Wallpaper>, String>> = OnceLock::new();
    CATALOG
        .get_or_init(|| {
            let catalog: Vec<Wallpaper> =
                serde_json::from_str(include_str!("../../../wallpapers/catalog.json"))
                    .map_err(|error| error.to_string())?;
            if catalog.is_empty()
                || catalog.iter().enumerate().any(|(index, asset)| {
                    usize::from(asset.id) != index + 1
                        || asset.name != format!("desktop-{:02}.jpg", asset.id)
                        || !(1..=MAX_BYTES).contains(&asset.bytes)
                        || asset.width == 0
                        || asset.height == 0
                        || u64::from(asset.width) * u64::from(asset.height) > 40_000_000
                        || asset.sha256.len() != 64
                        || !asset
                            .sha256
                            .bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                })
            {
                return Err("Invalid wallpaper catalog".into());
            }
            Ok(catalog)
        })
        .as_ref()
        .map(Vec::as_slice)
        .map_err(Clone::clone)
}

pub(super) fn entry(id: u8) -> Result<&'static Wallpaper, String> {
    let index = id.checked_sub(1).ok_or("Unknown wallpaper")?;
    all()?
        .get(usize::from(index))
        .ok_or_else(|| "Unknown wallpaper".into())
}
