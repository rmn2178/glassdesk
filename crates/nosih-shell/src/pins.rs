use std::{
    env,
    ffi::c_void,
    fs,
    path::{Path, PathBuf},
};

use windows::{
    Win32::{
        Storage::{
            EnhancedStorage::{PKEY_AppUserModel_ID, PKEY_Link_TargetParsingPath},
            FileSystem::GetDriveTypeW,
        },
        System::{
            Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree, IPersistFile, STGM_READ},
            Registry::{HKEY_CURRENT_USER, RRF_RT_REG_BINARY, RegGetValueW},
        },
        UI::{
            Shell::{
                BHID_SFUIObject, CMF_NORMAL, CMINVOKECOMMANDINFO, GCS_VERBW, IContextMenu,
                IShellItem, IShellItem2, IShellLinkW, PropertiesSystem::IPropertyStore,
                SHCreateItemFromParsingName, ShellLink,
            },
            WindowsAndMessaging::{
                CreatePopupMenu, DestroyMenu, GetMenuItemCount, GetMenuItemID, SW_SHOWNORMAL,
            },
        },
    },
    core::{HSTRING, Interface, PCSTR, PCWSTR, PSTR, PWSTR, w},
};

const TASKBAND: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Taskband");

use crate::{
    tracker::{APPS_FOLDER, is_program, normalize_id},
    watch::RegistryWatch,
};

/// The apps pinned to the Windows taskbar, in the taskbar's order: desktop
/// apps by the program their shortcut opens, Store apps (Media Player, Phone
/// Link, WhatsApp...) as `shell:AppsFolder\<AppUserModelID>`.
///
/// Desktop pins are the shortcuts in `User Pinned\TaskBar`. The order, and
/// the Store apps, which have no shortcut there, come from
/// `Taskband\Favorites`, an undocumented binary list. It is only searched
/// for those shortcut names and for AppUserModelIDs, so if its format
/// changes the desktop pins still come, in alphabetical order.
///
/// Requires COM to be initialized on the calling thread.
#[must_use]
pub fn taskbar_pins() -> Vec<String> {
    taskbar_pins_if_readable().unwrap_or_default()
}

/// Like [`taskbar_pins`], but `None` when Windows' pinned list cannot be
/// read at all, so a failed read is never mistaken for "everything was
/// unpinned".
#[must_use]
pub fn taskbar_pins_if_readable() -> Option<Vec<String>> {
    let appdata = env::var("APPDATA").ok()?;
    let folder = PathBuf::from(appdata)
        .join("Microsoft\\Internet Explorer\\Quick Launch\\User Pinned\\TaskBar");
    let links = fs::read_dir(folder).map(|entries| {
        entries
            .filter_map(|entry| Some(entry.ok()?.path()))
            .filter(|path| {
                path.extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("lnk"))
            })
            .collect::<Vec<PathBuf>>()
    });
    // Either source alone still gives pins: Windows 10 may have shortcuts
    // and no `Favorites` value yet, if the taskbar was never edited.
    let (favorites, mut links) = match (favorites(), links) {
        (None, Err(_)) => return None,
        (favorites, links) => (favorites.unwrap_or_default(), links.unwrap_or_default()),
    };
    links.sort();

    // (position in Favorites, id); shortcuts it does not mention go last.
    let mut found: Vec<(usize, String)> = Vec::new();
    for link in &links {
        let Some(id) = shortcut_id(link) else {
            continue;
        };
        let position = link
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| find_utf16(&favorites, name))
            .unwrap_or(usize::MAX);
        found.push((position, id));
    }
    found.extend(
        app_user_model_ids(&favorites)
            .into_iter()
            .map(|(position, aumid)| (position, format!("shell:AppsFolder\\{aumid}"))),
    );
    // Stable: shortcuts with the same (unknown) position keep their alphabetical order.
    found.sort_by_key(|(position, _)| *position);

    let mut pins: Vec<String> = Vec::new();
    for (_, id) in found {
        if !pins.contains(&id) {
            pins.push(id);
        }
    }
    Some(pins)
}

/// Tells a window, by posting it `message`, when apps are pinned to or
/// unpinned from the Windows taskbar (from Start, Explorer or an app's own
/// menu): Explorer rewrites `Taskband` for every pin and unpin.
#[must_use]
pub fn watch(hwnd: isize, message: u32) -> Option<RegistryWatch> {
    RegistryWatch::new(TASKBAND, false, hwnd, message)
}

/// The taskbar's pinned list as Explorer stores it.
fn favorites() -> Option<Vec<u8>> {
    let key = TASKBAND;
    let mut size = 0u32;
    // SAFETY: First call only asks for the size; the second fills a buffer of that size.
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key,
            w!("Favorites"),
            RRF_RT_REG_BINARY,
            None,
            None,
            Some(&mut size),
        )
        .ok()
        .ok()?;
        let mut blob = vec![0u8; size as usize];
        RegGetValueW(
            HKEY_CURRENT_USER,
            key,
            w!("Favorites"),
            RRF_RT_REG_BINARY,
            None,
            Some(blob.as_mut_ptr().cast()),
            Some(&mut size),
        )
        .ok()
        .ok()?;
        blob.truncate(size as usize);
        Some(blob)
    }
}

/// Byte offset of `text`, as UTF-16, inside `blob`, ignoring ASCII case.
fn find_utf16(blob: &[u8], text: &str) -> Option<usize> {
    let needle: Vec<u8> = text
        .to_ascii_lowercase()
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    if needle.is_empty() {
        return None;
    }
    let lowered: Vec<u8> = blob.iter().map(u8::to_ascii_lowercase).collect();
    lowered
        .windows(needle.len())
        .position(|window| window == needle.as_slice())
}

/// AppUserModelIDs of Store apps (`Name_publisherid!App`) found in `blob`,
/// with their byte offsets, in order. Strings may start at any byte, so
/// both alignments are read.
fn app_user_model_ids(blob: &[u8]) -> Vec<(usize, String)> {
    let mut found: Vec<(usize, String)> = Vec::new();
    for start in 0..2 {
        let units: Vec<u16> = blob[start.min(blob.len())..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&pair| u16::from_le_bytes(pair))
            .collect();
        let allowed = |unit: u16| {
            char::from_u32(u32::from(unit))
                .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '!'))
        };
        let mut index = 0;
        while index < units.len() {
            if !allowed(units[index]) {
                index += 1;
                continue;
            }
            let begin = index;
            while index < units.len() && allowed(units[index]) {
                index += 1;
            }
            let word = String::from_utf16_lossy(&units[begin..index]);
            if is_app_user_model_id(&word) {
                found.push((start + begin * 2, word));
            }
        }
    }
    found.sort_by_key(|(position, _)| *position);
    found
}

/// `Package.Name_publisherid!AppId`, where the publisher id is 13 lowercase
/// letters and digits.
fn is_app_user_model_id(word: &str) -> bool {
    let Some((family, app)) = word.split_once('!') else {
        return false;
    };
    let Some((name, publisher)) = family.rsplit_once('_') else {
        return false;
    };
    !app.is_empty()
        && !app.contains('!')
        && !name.is_empty()
        && publisher.len() == 13
        && publisher
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

/// Unpins `id` from the Windows taskbar with the shell's own "Unpin from
/// taskbar" command, so unpinning in the dock unpins there too. Windows lets
/// programs unpin but not pin, so the reverse is not possible. `false` if it
/// is not pinned there or the command is missing.
///
/// Requires COM on the calling thread; it may load shell extensions, so call
/// it off the UI thread.
#[must_use]
pub fn unpin_from_taskbar(id: &str) -> bool {
    let Some(item) = pinned_item(id) else {
        return false;
    };
    invoke_verb(&item, "taskbarunpin")
}

/// What Explorer pinned for `id`: the Store app itself, or the shortcut in
/// `User Pinned\TaskBar` that opens the program.
fn pinned_item(id: &str) -> Option<String> {
    if id.starts_with("shell:AppsFolder\\") || crate::tracker::is_shortcut(id) {
        return Some(id.to_owned());
    }
    let folder = PathBuf::from(env::var("APPDATA").ok()?)
        .join("Microsoft\\Internet Explorer\\Quick Launch\\User Pinned\\TaskBar");
    fs::read_dir(folder)
        .ok()?
        .filter_map(|entry| Some(entry.ok()?.path()))
        .find(|link| link_target(link).is_some_and(|target| normalize_id(&target) == id))
        .map(|link| link.to_string_lossy().into_owned())
}

/// Runs the context menu command `verb` (canonical name) on the shell item
/// at `path`, as clicking it in Explorer would.
fn invoke_verb(path: &str, verb: &str) -> bool {
    // SAFETY: COM objects and the popup menu live for the block; the menu is
    // destroyed before returning.
    unsafe {
        let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(path), None)
        else {
            return false;
        };
        let Ok(menu) = item.BindToHandler::<_, IContextMenu>(None, &BHID_SFUIObject) else {
            return false;
        };
        let Ok(popup) = CreatePopupMenu() else {
            return false;
        };
        let mut done = false;
        if menu
            .QueryContextMenu(popup, 0, 1, 0x7FFF, CMF_NORMAL)
            .is_ok()
        {
            for position in 0..GetMenuItemCount(Some(popup)).max(0) {
                let command = GetMenuItemID(popup, position);
                if command == u32::MAX || command == 0 {
                    continue;
                }
                let offset = (command - 1) as usize;
                let mut name = [0u16; 64];
                let named = menu
                    .GetCommandString(
                        offset,
                        GCS_VERBW,
                        None,
                        PSTR(name.as_mut_ptr().cast()),
                        name.len() as u32,
                    )
                    .is_ok();
                let len = name.iter().position(|&unit| unit == 0).unwrap_or(0);
                if !named || !String::from_utf16_lossy(&name[..len]).eq_ignore_ascii_case(verb) {
                    continue;
                }
                let info = CMINVOKECOMMANDINFO {
                    cbSize: size_of::<CMINVOKECOMMANDINFO>() as u32,
                    // A command offset passed where a verb name would go.
                    lpVerb: PCSTR(offset as *const u8),
                    nShow: SW_SHOWNORMAL.0,
                    ..Default::default()
                };
                done = menu.InvokeCommand(&info).is_ok();
                break;
            }
        }
        let _ = DestroyMenu(popup);
        done
    }
}

/// `false` only when `id` certainly no longer exists: a program removed from
/// a fixed drive, or a Store app that was uninstalled. Programs on removable
/// or network drives count as present; they may just be unplugged.
///
/// Requires COM on the calling thread.
#[must_use]
pub fn app_exists(id: &str) -> bool {
    if id.starts_with("shell:AppsFolder\\") {
        // SAFETY: Only asks the shell whether it can resolve the item.
        return unsafe {
            SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(id), None)
        }
        .is_ok();
    }
    let path = Path::new(id);
    let Some(root) = id.get(..3).filter(|root| root.ends_with(":\\")) else {
        return true;
    };
    const DRIVE_FIXED: u32 = 3;
    // SAFETY: Read-only query on a drive root.
    let fixed = unsafe { GetDriveTypeW(&HSTRING::from(root)) } == DRIVE_FIXED;
    !fixed || path.exists()
}

pub fn link_target(link: &Path) -> Option<String> {
    read_link(link)?.0
}

/// What the dock pins for the shortcut `link`, as the Windows taskbar does:
/// the program it opens, whatever its arguments, so the program's windows
/// gather under one icon (the arguments still apply when it is opened, see
/// [`launch_target`]); a web app's own identity, under which its windows
/// gather instead (see [`is_separate_app`]); or the shortcut itself when it
/// opens something that is not a program (a script, a document). `None` for
/// shortcuts to no file (File Explorer, Control Panel items) or to a file
/// that no longer exists.
///
/// Requires COM to be initialized on the calling thread.
#[must_use]
pub fn shortcut_id(link: &Path) -> Option<String> {
    let (target, arguments) = read_link(link)?;
    let target = target.filter(|target| Path::new(target).exists())?;
    // A web app's shortcut opens a launcher (`msedge_proxy.exe`), but the
    // app's windows carry its own identity: pinned by it, they gather under
    // the pin, as on the taskbar.
    if let Some(app) = link_app_id(link)
        .filter(|app| arguments.contains("--app-id=") || is_separate_app(app, &target))
        .filter(|app| app_exists(&format!("{APPS_FOLDER}{app}")))
    {
        return Some(format!("{APPS_FOLDER}{app}"));
    }
    Some(if is_program(&target) {
        normalize_id(&target)
    } else {
        normalize_id(&link.to_string_lossy())
    })
}

/// `true` when Windows lists `app_id` as an app of its own rather than as a
/// shortcut to `program`, the program that runs it: a web app inside a
/// browser. The browser's own identity (`MSEdge`, `Chrome`) is a shortcut to
/// the browser and stays with it.
///
/// Requires COM to be initialized on the calling thread.
#[must_use]
pub fn is_separate_app(app_id: &str, program: &str) -> bool {
    // SAFETY: Only resolves the app's shell item and reads one property.
    let target = unsafe {
        let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem2>(
            &HSTRING::from(format!("{APPS_FOLDER}{app_id}")),
            None,
        ) else {
            return false;
        };
        item.GetString(&PKEY_Link_TargetParsingPath)
            .ok()
            .and_then(|target| take_string(target))
    };
    target.is_none_or(|target| normalize_id(&target) != normalize_id(program))
}

/// The identity a shortcut gives the app it opens (`AppUserModelID`), when
/// it sets one.
fn link_app_id(link: &Path) -> Option<String> {
    // SAFETY: The shortcut is loaded into an object that lives through the read.
    let value = unsafe {
        let shell_link: IShellLinkW =
            CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        shell_link
            .cast::<IPersistFile>()
            .ok()?
            .Load(&HSTRING::from(link.as_os_str()), STGM_READ)
            .ok()?;
        shell_link
            .cast::<IPropertyStore>()
            .ok()?
            .GetValue(&PKEY_AppUserModel_ID)
            .ok()?
    };
    Some(value.to_string()).filter(|app_id| !app_id.is_empty())
}

/// `true` when `link` opens the dock item `id`: the program it targets, or,
/// for a web app pinned by its identity, the app it names.
fn opens(link: &Path, id: &str) -> bool {
    match id.strip_prefix(APPS_FOLDER) {
        Some(app_id) => link_app_id(link).is_some_and(|linked| linked == app_id),
        None => read_link(link)
            .and_then(|(target, _)| target)
            .is_some_and(|target| normalize_id(&target) == id),
    }
}

/// Reads and frees a string the shell allocated.
///
/// # Safety
/// `text` must be null or a string allocated with `CoTaskMemAlloc`.
unsafe fn take_string(text: PWSTR) -> Option<String> {
    if text.is_null() {
        return None;
    }
    // SAFETY: Guaranteed by the caller.
    unsafe {
        let value = text.to_string().ok();
        CoTaskMemFree(Some(text.0 as *const c_void));
        value.filter(|value| !value.is_empty())
    }
}

/// The shortcut to open `id` with, when one carries more than the program:
/// `wscript.exe "script.vbs"`, a browser profile, a web app. Searched among
/// the shortcuts Nosih keeps and those pinned to the Windows taskbar, only
/// when the app is opened, never on the dock's thread.
///
/// Requires COM to be initialized on the calling thread.
#[must_use]
pub fn launch_target(id: &str) -> Option<String> {
    if crate::tracker::is_shortcut(id) || id.starts_with("shell:") {
        return None;
    }
    [kept_folder(), taskbar_folder()]
        .into_iter()
        .flatten()
        .filter_map(|folder| fs::read_dir(folder).ok())
        .flatten()
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|link| crate::tracker::is_shortcut(&link.to_string_lossy()))
        .find(|link| {
            read_link(link).is_some_and(|(target, arguments)| {
                !arguments.is_empty() && target.is_some_and(|target| normalize_id(&target) == id)
            })
        })
        .map(|link| link.to_string_lossy().into_owned())
}

/// Any shortcut, kept by Nosih or pinned to the Windows taskbar, that opens
/// the program `id`, with or without arguments. It carries the identity
/// Windows gives the program's jump list (`AppUserModelID`).
///
/// Requires COM to be initialized on the calling thread.
#[must_use]
pub fn shortcut_for_program(id: &str) -> Option<String> {
    if crate::tracker::is_shortcut(id) || id.starts_with("shell:") {
        return None;
    }
    [kept_folder(), taskbar_folder()]
        .into_iter()
        .flatten()
        .filter_map(|folder| fs::read_dir(folder).ok())
        .flatten()
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|link| crate::tracker::is_shortcut(&link.to_string_lossy()))
        .find(|link| {
            read_link(link)
                .and_then(|(target, _)| target)
                .is_some_and(|target| normalize_id(&target) == id)
        })
        .map(|link| link.to_string_lossy().into_owned())
}

/// The shortcut Nosih keeps for the program `id` (see [`keep_shortcut`]),
/// which gives its pin its name and icon. Only Nosih's own folder, which
/// is usually empty: cheap enough for every icon lookup. A web app pinned by
/// its identity also takes its shortcut on the taskbar, which holds the name
/// and icon the user gave it.
///
/// Requires COM to be initialized on the calling thread.
#[must_use]
pub fn kept_shortcut_for(id: &str) -> Option<String> {
    if crate::tracker::is_shortcut(id) {
        return None;
    }
    let folders = if id.starts_with(APPS_FOLDER) {
        [kept_folder(), taskbar_folder()]
    } else if id.starts_with("shell:") {
        return None;
    } else {
        [kept_folder(), None]
    };
    folders
        .into_iter()
        .flatten()
        .filter_map(|folder| fs::read_dir(folder).ok())
        .flatten()
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|link| crate::tracker::is_shortcut(&link.to_string_lossy()))
        .find(|link| opens(link, id))
        .map(|link| link.to_string_lossy().into_owned())
}

/// The folder where Nosih keeps its own copy of shortcuts pinned from
/// elsewhere (the desktop, Downloads), as the taskbar keeps `User Pinned`:
/// the pin keeps working if the original is moved or deleted.
fn kept_folder() -> Option<PathBuf> {
    Some(PathBuf::from(env::var_os("APPDATA")?).join("Nosih\\Pins"))
}

fn taskbar_folder() -> Option<PathBuf> {
    Some(
        PathBuf::from(env::var_os("APPDATA")?)
            .join("Microsoft\\Internet Explorer\\Quick Launch\\User Pinned\\TaskBar"),
    )
}

/// Pins the shortcut `link`, dropped on the dock, and returns the pin id
/// ([`shortcut_id`]). When the shortcut is needed to open it (it is the pin
/// itself, or it carries arguments), Nosih keeps a copy: the pin keeps
/// working if the original is moved or deleted. One kept shortcut per
/// program, as on the taskbar: the newest replaces the older one.
#[must_use]
pub fn keep_shortcut(link: &Path) -> Option<String> {
    let id = shortcut_id(link)?;
    let (_, arguments) = read_link(link)?;
    let is_link = crate::tracker::is_shortcut(&id);
    if !is_link && arguments.is_empty() {
        return Some(id);
    }
    let folder = kept_folder()?;
    let parent = link.parent()?;
    if [Some(&folder), taskbar_folder().as_ref()]
        .into_iter()
        .flatten()
        .any(|managed| same_path(parent, managed))
    {
        return Some(id);
    }
    let contents = fs::read(link).ok()?;
    fs::create_dir_all(&folder).ok()?;
    if !is_link {
        forget_shortcut(&id);
    }
    let stem = link.file_stem()?.to_string_lossy().into_owned();
    for attempt in 1..100 {
        let name = if attempt == 1 {
            format!("{stem}.lnk")
        } else {
            format!("{stem} ({attempt}).lnk")
        };
        let kept = folder.join(name);
        match fs::read(&kept) {
            Ok(existing) if existing == contents => {}
            Ok(_) => continue,
            Err(_) => fs::write(&kept, &contents).ok()?,
        }
        return Some(if is_link {
            normalize_id(&kept.to_string_lossy())
        } else {
            id
        });
    }
    None
}

/// Deletes what Nosih kept to open `id` once it is no longer pinned: the
/// kept shortcut itself, or the kept shortcuts that open that program. The
/// taskbar's and the user's own shortcuts are never touched.
///
/// Requires COM to be initialized on the calling thread.
pub fn forget_shortcut(id: &str) {
    let Some(folder) = kept_folder() else {
        return;
    };
    if crate::tracker::is_shortcut(id) {
        let path = Path::new(id);
        if path
            .parent()
            .is_some_and(|parent| same_path(parent, &folder))
        {
            let _ = fs::remove_file(path);
        }
        return;
    }
    for link in fs::read_dir(&folder)
        .into_iter()
        .flatten()
        .filter_map(|entry| Some(entry.ok()?.path()))
    {
        if opens(&link, id) {
            let _ = fs::remove_file(link);
        }
    }
}

fn same_path(a: &Path, b: &Path) -> bool {
    a.as_os_str().eq_ignore_ascii_case(b.as_os_str())
}

/// The program a shortcut opens (`None` when it points at no file, such as
/// a Control Panel item) and its arguments.
fn read_link(link: &Path) -> Option<(Option<String>, String)> {
    // SAFETY: COM calls use buffers and strings that remain alive for the duration of each call.
    unsafe {
        let shell_link: IShellLinkW =
            CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        shell_link
            .cast::<IPersistFile>()
            .ok()?
            .Load(&HSTRING::from(link.as_os_str()), STGM_READ)
            .ok()?;
        let text = |buffer: &[u16]| {
            let len = buffer.iter().position(|&unit| unit == 0).unwrap_or(0);
            String::from_utf16_lossy(&buffer[..len])
        };
        let mut path = [0u16; 1024];
        let target = shell_link
            .GetPath(&mut path, std::ptr::null_mut(), 0)
            .ok()
            .map(|()| text(&path))
            .filter(|target| !target.is_empty());
        // Up to 32 767 characters, the longest command line Windows takes.
        let mut arguments = vec![0u16; 32_768];
        let arguments = shell_link
            .GetArguments(&mut arguments)
            .map_or_else(|_| String::new(), |()| text(&arguments));
        Some((target, arguments.trim().to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    #[test]
    fn favorites_should_give_store_apps_and_shortcuts_in_taskbar_order() {
        // Shaped like the real blob: binary noise around UTF-16 names, one of
        // them at an odd offset.
        let mut blob = vec![0x14, 0x00, 0x1f, 0x80];
        blob.extend(utf16("File Explorer.lnk"));
        blob.extend([0x00, 0x00, 0x07]);
        blob.extend(utf16(
            "Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic",
        ));
        blob.extend([0x00, 0x00, 0x49]);
        blob.extend(utf16("Brave.lnk"));

        let explorer = find_utf16(&blob, "file explorer.lnk");
        let brave = find_utf16(&blob, "Brave.lnk");
        let store = app_user_model_ids(&blob);
        assert_eq!(store.len(), 1);
        assert_eq!(
            store[0].1,
            "Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic"
        );
        assert!(explorer < Some(store[0].0) && Some(store[0].0) < brave);
    }

    #[test]
    fn app_user_model_id_should_need_a_publisher_id_and_an_app() {
        assert!(is_app_user_model_id(
            "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"
        ));
        assert!(!is_app_user_model_id("Microsoft.Windows.Explorer"));
        assert!(!is_app_user_model_id(
            "Microsoft.ZuneMusic_11.2312.8.0_x64__8wekyb3d8bbwe"
        ));
        assert!(!is_app_user_model_id("Name_8wekyb3d8bbwe!"));
    }
}
