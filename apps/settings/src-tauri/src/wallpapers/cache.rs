//! Downloads only an explicit, uncached selection. No startup or idle work.
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use sha2::{Digest, Sha256};

use super::catalog::{ORIGIN, Wallpaper};

static TEMPORARY: AtomicU64 = AtomicU64::new(0);

fn matches(path: &Path, asset: &Wallpaper) -> bool {
    let Ok(file) = fs::File::open(path) else {
        return false;
    };
    if !file
        .metadata()
        .is_ok_and(|metadata| metadata.len() == asset.bytes)
    {
        return false;
    }
    let mut reader = file.take(asset.bytes + 1);
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 8192];
    let mut size = 0;
    loop {
        let Ok(read) = reader.read(&mut buffer) else {
            return false;
        };
        if read == 0 {
            break;
        }
        size += read as u64;
        digest.update(&buffer[..read]);
    }
    size == asset.bytes && format!("{:x}", digest.finalize()) == asset.sha256
}

pub(super) fn copy_body(
    reader: impl Read,
    file: &mut fs::File,
    expected: u64,
) -> Result<(), String> {
    // Content-Length can be absent or dishonest; bound the actual stream too.
    let written =
        std::io::copy(&mut reader.take(expected + 1), file).map_err(|error| error.to_string())?;
    if written != expected {
        return Err("Incomplete or oversized wallpaper download".into());
    }
    Ok(())
}

pub(super) fn download(asset: &Wallpaper, file: &mut fs::File) -> Result<(), String> {
    // Drop the client/runtime at the end of the request; cached/offline use
    // does not keep HTTP threads or connection pools alive in Settings.
    let client = reqwest::blocking::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(30))
        .user_agent(concat!(
            "Nosih/",
            env!("CARGO_PKG_VERSION"),
            " (wallpapers)"
        ))
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .get(format!("{ORIGIN}/{}", asset.name))
        .send()
        .map_err(|error| error.to_string())?;
    if response.status() != reqwest::StatusCode::OK {
        return Err(format!("Wallpaper download failed ({})", response.status()));
    }
    if response
        .content_length()
        .is_some_and(|size| size != asset.bytes)
    {
        return Err("Unexpected wallpaper download size".into());
    }
    copy_body(response, file, asset.bytes)
}

pub(super) fn materialize(
    asset: &Wallpaper,
    root: &Path,
    fetch: impl FnOnce(&Wallpaper, &mut fs::File) -> Result<(), String>,
) -> Result<PathBuf, String> {
    let destination = root.join(&asset.name);
    // Also reuses images cached by older, fully offline versions of Nosih.
    if matches(&destination, asset) {
        return Ok(destination);
    }
    fs::create_dir_all(root).map_err(|error| error.to_string())?;
    let temporary = root.join(format!(
        ".{}-{}-{}.tmp",
        asset.name,
        std::process::id(),
        TEMPORARY.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| -> Result<(), String> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| error.to_string())?;
        fetch(asset, &mut file)?;
        file.flush()
            .and_then(|()| file.sync_all())
            .map_err(|error| error.to_string())?;
        drop(file);
        // Exact length + SHA-256 verifies the known JPEG, without decoding
        // a full-resolution image or trusting a server's Content-Type.
        if !matches(&temporary, asset) {
            return Err("Wallpaper integrity check failed".into());
        }
        fs::rename(&temporary, &destination).map_err(|error| error.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result?;
    Ok(destination)
}
