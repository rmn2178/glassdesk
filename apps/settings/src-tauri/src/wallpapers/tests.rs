use std::{
    fs,
    io::Write,
    sync::atomic::{AtomicUsize, Ordering},
};

use super::*;

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        Self(env::temp_dir().join(format!(
            "nosih-wallpaper-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        // Only files created by this fixture, never user cache/backgrounds.
        if let Ok(files) = fs::read_dir(&self.0) {
            for file in files.flatten() {
                let _ = fs::remove_file(file.path());
            }
        }
        let _ = fs::remove_dir(&self.0);
    }
}
fn original(asset: &catalog::Wallpaper) -> Result<Vec<u8>, String> {
    fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../wallpapers")
            .join(&asset.name),
    )
    .map_err(|error| error.to_string())
}
fn fetch(asset: &catalog::Wallpaper, file: &mut fs::File) -> Result<(), String> {
    file.write_all(&original(asset)?)
        .map_err(|error| error.to_string())
}

#[test]
fn metadata_matches_all_originals_without_embedding_them_in_the_executable() -> Result<(), String> {
    let _apartment = Apartment::new()?;
    let root = Directory::new();
    for asset in catalog::all()? {
        let bytes = original(asset)?;
        let image = nosih_shell::imagery::decode(&bytes, 128, 40_000_000)
            .ok_or("invalid catalog image")?;
        assert!(image.width <= 128 && image.height <= 128);
        // This checks the generated digest/length against the source file.
        cache::materialize(asset, &root.0, fetch)?;
    }
    Ok(())
}

#[test]
fn invalid_ids_never_access_disk_network_or_windows() {
    let root = Directory::new();
    let invalid =
        u8::try_from(catalog::all().map_or(0, |entries| entries.len()) + 1).unwrap_or(255);
    for id in [0, invalid, 255] {
        assert!(
            apply_at(
                id,
                &root.0,
                |_, _| panic!("unexpected fetch"),
                |_| panic!("unexpected desktop write")
            )
            .is_err()
        );
    }
    assert!(!root.0.exists());
}

#[test]
fn cache_is_reused_offline_repaired_and_preserves_failed_desktop_changes() -> Result<(), String> {
    let root = Directory::new();
    let asset = entry(1)?;
    let path = cache::materialize(asset, &root.0, fetch)?;
    let modified = fs::metadata(&path)
        .and_then(|m| m.modified())
        .map_err(|e| e.to_string())?;
    cache::materialize(asset, &root.0, |_, _| {
        panic!("valid cache must work offline")
    })?;
    assert_eq!(
        fs::metadata(&path)
            .and_then(|m| m.modified())
            .map_err(|e| e.to_string())?,
        modified
    );
    fs::write(&path, b"damaged cache").map_err(|e| e.to_string())?;
    cache::materialize(asset, &root.0, fetch)?;
    assert_eq!(
        fs::read(&path).map_err(|e| e.to_string())?,
        original(asset)?
    );
    let result = apply_at(
        1,
        &root.0,
        |_, _| panic!("no download"),
        |_| Err("Windows refused the change".into()),
    );
    assert_eq!(result, Err("Windows refused the change".into()));
    assert_eq!(selected_path(&root.0, &path), Some(1));
    assert_eq!(
        selected_path(&root.0, &root.0.join("elsewhere/desktop-01.jpg")),
        None
    );
    Ok(())
}

#[test]
fn failed_downloads_and_wrong_hashes_never_publish_or_apply_a_wallpaper() -> Result<(), String> {
    let root = Directory::new();
    fs::create_dir_all(&root.0).map_err(|e| e.to_string())?;
    let path = root.0.join("desktop-01.jpg");
    let existing = b"a damaged file that must not be overwritten by an invalid response";
    fs::write(&path, existing).map_err(|e| e.to_string())?;
    for kind in 0..3 {
        let result = apply_at(
            1,
            &root.0,
            |asset, file| match kind {
                0 => Err("offline".into()),
                1 => file
                    .write_all(b"<html>GitHub error</html>")
                    .map_err(|e| e.to_string()),
                _ => file
                    .write_all(&vec![0; asset.bytes as usize])
                    .map_err(|e| e.to_string()),
            },
            |_| panic!("unverified bytes must never reach Windows"),
        );
        assert!(result.is_err());
        assert_eq!(fs::read(&path).map_err(|e| e.to_string())?, existing);
        assert_eq!(
            fs::read_dir(&root.0).map_err(|e| e.to_string())?.count(),
            1,
            "no partial files left behind"
        );
    }
    apply_at(1, &root.0, fetch, |_| Ok(()))?;
    Ok(())
}

#[test]
fn apply_guard_rejects_overlap_and_is_released_after_failure() -> Result<(), String> {
    let guard = ApplyGuard::acquire()?;
    assert!(ApplyGuard::acquire().is_err());
    drop(guard);
    drop(ApplyGuard::acquire()?);
    Ok(())
}

#[test]
fn response_streams_are_bounded_even_without_a_content_length() -> Result<(), String> {
    let root = Directory::new();
    fs::create_dir_all(&root.0).map_err(|e| e.to_string())?;
    for size in [99, 100, 101, 4096] {
        let path = root.0.join(format!("body-{size}"));
        let mut file = fs::File::create(&path).map_err(|e| e.to_string())?;
        let bytes = vec![0_u8; size];
        let result = cache::copy_body(bytes.as_slice(), &mut file, 100);
        assert_eq!(result.is_ok(), size == 100);
        assert_eq!(
            file.metadata().map_err(|e| e.to_string())?.len(),
            size.min(101) as u64
        );
    }
    Ok(())
}

#[test]
#[ignore = "Downloads one catalog original into test storage; never changes the desktop"]
fn native_github_download_verifies_and_then_reuses_the_cache_offline() -> Result<(), String> {
    let root = Directory::new();
    cache::materialize(entry(1)?, &root.0, cache::download)?;
    cache::materialize(entry(1)?, &root.0, |_, _| panic!("must use cache"))?;
    Ok(())
}
