//! An app's jump list, as the Windows taskbar shows it on right-click: the
//! items pinned to it, the app's own categories, its recent or frequent
//! items and its tasks.
//!
//! Only Explorer reads another app's jump list. It does so through shell
//! interfaces Windows does not document (`IApplicationResolver`,
//! `IAutomaticDestinationList` and the reading side of the destination
//! list behind `ICustomDestinationList`). The layouts below are the ones
//! Open-Shell and taskbar tools have relied on since Windows 10 (10547). If a
//! build changes them, the interfaces are not found and the list is empty:
//! the dock's menu then has its own items only.
//!
//! Requires COM to be initialized on the calling thread.

// The interfaces keep the method names Windows gives them.
#![allow(non_snake_case)]

use std::{env, ffi::c_void, iter, path::Path};

use serde::{Deserialize, Serialize};
use windows::{
    Win32::{
        Foundation::{E_FAIL, E_INVALIDARG, E_OUTOFMEMORY, HWND, PROPERTYKEY},
        Storage::EnhancedStorage::{
            PKEY_AppUserModel_ID, PKEY_AppUserModel_IsDestListSeparator, PKEY_Link_Arguments,
            PKEY_Title,
        },
        System::Com::{
            CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree, IPersistStream, IStream,
            STATFLAG_NONAME, STATSTG, STREAM_SEEK_SET,
        },
        UI::{
            Shell::{
                BHID_SFUIObject, CMF_DEFAULTONLY, CMINVOKECOMMANDINFO, CMINVOKECOMMANDINFOEX,
                Common::{IObjectArray, ITEMIDLIST},
                DestinationList, FOLDERID_AppsFolder, IContextMenu, ILGetSize, IShellItem,
                IShellItem2, IShellLinkW, KF_FLAG_DEFAULT,
                PropertiesSystem::IPropertyStore,
                SEE_MASK_FLAG_LOG_USAGE, SEE_MASK_IDLIST, SEE_MASK_NOASYNC, SHCreateItemFromIDList,
                SHCreateItemFromParsingName, SHCreateItemInKnownFolder, SHCreateMemStream,
                SHELLEXECUTEINFOW, SHGetIDListFromObject, SHGetNameFromIDList,
                SHGetPathFromIDListW, SHLoadIndirectString, SIGDN, SIGDN_DESKTOPABSOLUTEPARSING,
                SIGDN_NORMALDISPLAY, ShellExecuteExW, ShellLink,
            },
            WindowsAndMessaging::{
                CreatePopupMenu, DestroyMenu, GET_MENU_DEFAULT_ITEM_FLAGS, GetMenuDefaultItem,
                SW_SHOWNORMAL,
            },
        },
    },
    core::{
        BOOL, GUID, HRESULT, HSTRING, IUnknown, IUnknown_Vtbl, Interface, PCSTR, PCWSTR, PWSTR, w,
    },
};

use crate::{
    icons::{self, IconImage},
    launcher::allow_foreground,
    pins, tracker,
};

/// Jump list icons, in pixels: a 16-pixel menu icon at up to 200% scale.
pub const ICON_SIZE: u32 = 32;

/// Recent, frequent and the app's own items together, as the taskbar shows
/// by default (`JumpListItems_Maximum`). Pinned items and tasks are always
/// all shown.
const MAX_ITEMS: usize = 10;
/// The longest label kept, as one menu row.
const MAX_LABEL: usize = 260;
const EXPLORER_APP_ID: &str = "Microsoft.Windows.Explorer";

const CLSID_APPLICATION_RESOLVER: GUID = GUID::from_u128(0x660b90c8_73a9_4b58_8cae_355b7f55341b);
const CLSID_AUTOMATIC_DESTINATION_LIST: GUID =
    GUID::from_u128(0xf0ae1542_f497_484b_a175_a20db09144ba);
/// The destination list's reading interface changed IID within Windows 10
/// (10240, then 10547) but kept its layout.
const IID_DESTINATION_LISTS: [GUID; 2] = [
    GUID::from_u128(0x507101cd_f6ad_46c8_8e20_eeb9e6bac47f),
    GUID::from_u128(0xfebd543d_1f7b_4b38_940b_5933bd2cb21b),
];

const PKEY_APP_USER_MODEL_HOST_ENVIRONMENT: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3),
    pid: 14,
};
const PKEY_APP_USER_MODEL_ACTIVATION_CONTEXT: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3),
    pid: 20,
};
const CMIC_MASK_NOASYNC: u32 = 0x0000_0100;
const CMIC_MASK_UNICODE: u32 = 0x0000_4000;
const CMIC_MASK_FLAG_LOG_USAGE: u32 = 0x0400_0000;

#[windows_core::interface("de25675a-72de-44b4-9373-05170450c140")]
unsafe trait IApplicationResolver: IUnknown {
    fn GetAppIDForShortcut(&self, item: *mut c_void, app_id: *mut PWSTR) -> HRESULT;
    fn GetAppIDForShortcutObject(
        &self,
        link: *mut c_void,
        item: *mut c_void,
        app_id: *mut PWSTR,
    ) -> HRESULT;
    fn GetAppIDForWindow(
        &self,
        hwnd: HWND,
        app_id: *mut PWSTR,
        pinning_prevented: *mut BOOL,
        explicit: *mut BOOL,
        embedded_shortcut_valid: *mut BOOL,
    ) -> HRESULT;
}

#[windows_core::interface("e9c5ef8d-fd41-4f72-ba87-eb03bad5817c")]
unsafe trait IAutomaticDestinationList: IUnknown {
    fn Initialize(&self, app_id: PCWSTR, link: PCWSTR, reserved: PCWSTR) -> HRESULT;
    fn HasList(&self, has: *mut BOOL) -> HRESULT;
    fn GetList(
        &self,
        kind: i32,
        max: u32,
        flags: u32,
        iid: *const GUID,
        object: *mut *mut c_void,
    ) -> HRESULT;
}

/// The IID is the first of [`IID_DESTINATION_LISTS`]; the object
/// is queried for each.
#[windows_core::interface("507101cd-f6ad-46c8-8e20-eeb9e6bac47f")]
unsafe trait IDestinationList: IUnknown {
    fn SetMinItems(&self) -> HRESULT;
    fn SetApplicationID(&self, app_id: PCWSTR) -> HRESULT;
    fn GetSlotCount(&self) -> HRESULT;
    fn GetCategoryCount(&self, count: *mut u32) -> HRESULT;
    fn GetCategory(&self, index: u32, flags: i32, category: *mut Category) -> HRESULT;
    fn DeleteCategory(&self) -> HRESULT;
    fn EnumerateCategoryDestinations(
        &self,
        index: u32,
        iid: *const GUID,
        object: *mut *mut c_void,
    ) -> HRESULT;
}

#[repr(C)]
#[derive(Default)]
struct Category {
    kind: i32,
    /// A union: the name of an app's own category (`CATEGORY_CUSTOM`), or
    /// which known category it is (`CATEGORY_KNOWN`).
    name_or_known: usize,
    count: i32,
    reserved: [i32; 10],
}

const CATEGORY_CUSTOM: i32 = 0;
const CATEGORY_KNOWN: i32 = 1;
const CATEGORY_TASKS: i32 = 2;
const KNOWN_FREQUENT: i32 = 1;
const KNOWN_RECENT: i32 = 2;
const LIST_PINNED: i32 = 0;
const LIST_RECENT: i32 = 1;
const LIST_FREQUENT: i32 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Section {
    Pinned,
    Frequent,
    Recent,
    Tasks,
    /// A category the app names itself, such as a browser's "Most visited".
    Custom(String),
}

#[derive(Debug, Clone)]
pub struct Group {
    pub section: Section,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone)]
pub enum Entry {
    Item {
        label: String,
        target: Target,
        /// [`ICON_SIZE`] pixels square; `None` when Windows has none for it.
        icon: Option<IconImage>,
    },
    /// Between an app's tasks.
    Separator,
}

/// What a jump list entry opens, in a form that can be sent to the
/// process that opens it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Target {
    /// A file or folder, by its shell ID list.
    Item { id_list: Vec<u8> },
    /// A shortcut as it saves itself to a stream: the app's tasks and most of
    /// its own categories.
    Link { stream: Vec<u8> },
}

/// The jump list of dock item `id`, whose open windows are `windows`: the
/// program's own, or, when it has none, that of the first window that names
/// one at run time. A program can also host other apps' windows (Edge, its
/// web apps), whose lists are theirs, not its.
#[must_use]
pub fn for_dock_item(id: &str, windows: &[isize]) -> Vec<Group> {
    let mut tried = Vec::new();
    let candidates = iter::once(None).chain(windows.iter().take(8).copied().map(Some));
    for window in candidates {
        let Some(app) = app_id(id, window) else {
            continue;
        };
        if tried.contains(&app) {
            continue;
        }
        let groups = read(&app);
        if !groups.is_empty() {
            return groups;
        }
        tried.push(app);
    }
    Vec::new()
}

/// The identity Windows files the jump list of dock item `id` under, as the
/// taskbar works it out: from the app's open window when it has one, or
/// else from its shortcut or program.
#[must_use]
pub fn app_id(id: &str, window: Option<isize>) -> Option<String> {
    // SAFETY: The resolver and the shell item live through each call, and
    // each returned string is freed once read.
    unsafe {
        let resolver: IApplicationResolver = CoCreateInstance::<_, IUnknown>(
            &CLSID_APPLICATION_RESOLVER,
            None,
            CLSCTX_INPROC_SERVER,
        )
        .ok()?
        .cast()
        .ok()?;
        if let Some(hwnd) = window {
            let mut app_id = PWSTR::null();
            let (mut prevented, mut explicit, mut embedded) =
                (BOOL::default(), BOOL::default(), BOOL::default());
            if resolver
                .GetAppIDForWindow(
                    HWND(hwnd as *mut _),
                    &mut app_id,
                    &mut prevented,
                    &mut explicit,
                    &mut embedded,
                )
                .is_ok()
                && let Some(app_id) = take_string(app_id)
            {
                return Some(app_id);
            }
        }
        if let Some(aumid) = id.strip_prefix("shell:AppsFolder\\") {
            return Some(aumid.to_owned());
        }
        if id.starts_with("shell:") || id.starts_with("::") {
            return None;
        }
        // File Explorer files its list under its own identity, not its program's.
        let explorer = env::var("WINDIR")
            .map(|windir| format!("{windir}\\explorer.exe"))
            .is_ok_and(|explorer| explorer.eq_ignore_ascii_case(id));
        if explorer {
            return Some(EXPLORER_APP_ID.to_owned());
        }
        // A program's shortcut can give it its own identity (a browser's).
        if let Some(link) = pins::shortcut_for_program(id)
            .or_else(|| tracker::is_shortcut(id).then(|| id.to_owned()))
        {
            let item: IShellItem = SHCreateItemFromParsingName(&HSTRING::from(link), None).ok()?;
            let mut app_id = PWSTR::null();
            resolver
                .GetAppIDForShortcut(item.as_raw(), &mut app_id)
                .ok()
                .ok()?;
            return take_string(app_id);
        }
        if !tracker::is_program(id) {
            return None;
        }
        // A program without a shortcut, through one made for it in memory.
        let path = HSTRING::from(id);
        let item: IShellItem = SHCreateItemFromParsingName(&path, None).ok()?;
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        link.SetPath(&path).ok()?;
        let mut app_id = PWSTR::null();
        resolver
            .GetAppIDForShortcutObject(link.as_raw(), item.as_raw(), &mut app_id)
            .ok()
            .ok()?;
        take_string(app_id)
    }
}

/// The jump list of the app filed as `app_id`, in the taskbar's order:
/// pinned items, the app's categories (its own, recent, frequent), then its
/// tasks. Empty groups are left out.
#[must_use]
pub fn read(app_id: &str) -> Vec<Group> {
    let app_id = HSTRING::from(app_id);
    let automatic = automatic_list(&app_id);
    let custom = custom_list(&app_id);
    let mut groups = Vec::new();
    let mut seen = Vec::new();

    let pinned = automatic
        .as_ref()
        .and_then(|list| automatic_entries(list, LIST_PINNED))
        .unwrap_or_default();
    for (key, _) in &pinned {
        seen.push(key.clone());
    }
    push_group(
        &mut groups,
        Section::Pinned,
        pinned,
        usize::MAX,
        &mut Vec::new(),
    );

    let mut room = MAX_ITEMS;
    let mut tasks = None;
    let count = custom.as_ref().map_or(0, |list| {
        let mut count = 0;
        // SAFETY: `count` outlives the call.
        unsafe { list.GetCategoryCount(&mut count) }
            .ok()
            .map_or(0, |()| count)
    });
    for index in 0..count {
        let Some(list) = &custom else { break };
        let mut category = Category::default();
        // SAFETY: `category` has the layout the list fills in.
        if unsafe { list.GetCategory(index, 1, &mut category) }.is_err() {
            continue;
        }
        match category.kind {
            CATEGORY_CUSTOM => {
                // SAFETY: For an app's own category the union holds its name,
                // allocated for the caller.
                let name = unsafe { take_string(PWSTR(category.name_or_known as *mut u16)) };
                let Some(name) = name.map(|name| indirect(&name)) else {
                    continue;
                };
                let entries = destinations(list, index).unwrap_or_default();
                push_group(&mut groups, Section::Custom(name), entries, room, &mut seen);
            }
            CATEGORY_KNOWN => {
                let (section, kind) = match category.name_or_known as u32 as i32 {
                    KNOWN_FREQUENT => (Section::Frequent, LIST_FREQUENT),
                    KNOWN_RECENT => (Section::Recent, LIST_RECENT),
                    _ => continue,
                };
                let entries = automatic
                    .as_ref()
                    .and_then(|list| automatic_entries(list, kind))
                    .unwrap_or_default();
                push_group(&mut groups, section, entries, room, &mut seen);
            }
            CATEGORY_TASKS if tasks.is_none() => tasks = Some(index),
            _ => {}
        }
        room = MAX_ITEMS.saturating_sub(shown(&groups));
    }
    // Without a list of its own, an app gets its recent items.
    if count == 0 {
        let entries = automatic
            .as_ref()
            .and_then(|list| automatic_entries(list, LIST_RECENT))
            .unwrap_or_default();
        push_group(&mut groups, Section::Recent, entries, room, &mut seen);
    }
    if let (Some(list), Some(index)) = (&custom, tasks) {
        let entries = destinations(list, index).unwrap_or_default();
        push_group(
            &mut groups,
            Section::Tasks,
            entries,
            usize::MAX,
            &mut Vec::new(),
        );
    }
    groups
}

/// Opens a jump list entry. A file goes to `program`, the app whose list it
/// is, as on the taskbar, rather than to the file type's default app.
pub fn open(target: &Target, program: Option<&str>) -> windows::core::Result<()> {
    allow_foreground();
    match target {
        Target::Item { id_list } => open_item(id_list, program),
        Target::Link { stream } => open_link(stream),
    }
}

fn shown(groups: &[Group]) -> usize {
    groups
        .iter()
        .filter(|group| !matches!(group.section, Section::Pinned | Section::Tasks))
        .map(|group| group.entries.len())
        .sum()
}

/// Adds the entries not already shown (an item pinned also stays in the
/// recent list), up to `room`, and the group only if any remain.
fn push_group(
    groups: &mut Vec<Group>,
    section: Section,
    entries: Vec<(String, Entry)>,
    room: usize,
    seen: &mut Vec<String>,
) {
    let mut kept = Vec::new();
    for (key, entry) in entries {
        if kept.len() >= room {
            break;
        }
        if !key.is_empty() && seen.contains(&key) {
            continue;
        }
        if !key.is_empty() {
            seen.push(key);
        }
        kept.push(entry);
    }
    // A separator never starts, ends or doubles in a list.
    let mut entries: Vec<Entry> = Vec::with_capacity(kept.len());
    for entry in kept {
        let after_separator = entries
            .last()
            .is_none_or(|last| matches!(last, Entry::Separator));
        if matches!(entry, Entry::Separator) && after_separator {
            continue;
        }
        entries.push(entry);
    }
    if matches!(entries.last(), Some(Entry::Separator)) {
        entries.pop();
    }
    if !entries.is_empty() {
        groups.push(Group { section, entries });
    }
}

fn automatic_list(app_id: &HSTRING) -> Option<IAutomaticDestinationList> {
    // SAFETY: `app_id` lives through the call.
    unsafe {
        let list: IAutomaticDestinationList = CoCreateInstance::<_, IUnknown>(
            &CLSID_AUTOMATIC_DESTINATION_LIST,
            None,
            CLSCTX_INPROC_SERVER,
        )
        .ok()?
        .cast()
        .ok()?;
        list.Initialize(PCWSTR(app_id.as_ptr()), PCWSTR::null(), PCWSTR::null())
            .ok()
            .ok()?;
        Some(list)
    }
}

fn custom_list(app_id: &HSTRING) -> Option<IDestinationList> {
    // SAFETY: Each interface pointer queried is owned by the wrapper it is
    // turned into; `app_id` lives through the call.
    unsafe {
        let object: IUnknown =
            CoCreateInstance(&DestinationList, None, CLSCTX_INPROC_SERVER).ok()?;
        let list = IID_DESTINATION_LISTS.iter().find_map(|iid| {
            let mut raw = std::ptr::null_mut();
            object
                .query(iid, &mut raw)
                .is_ok()
                .then(|| IDestinationList::from_raw(raw))
        })?;
        list.SetApplicationID(PCWSTR(app_id.as_ptr())).ok().ok()?;
        Some(list)
    }
}

fn automatic_entries(list: &IAutomaticDestinationList, kind: i32) -> Option<Vec<(String, Entry)>> {
    let mut raw = std::ptr::null_mut();
    // SAFETY: The returned pointer is an `IObjectArray`, owned from here.
    unsafe {
        list.GetList(kind, MAX_ITEMS as u32 * 2, 1, &IObjectArray::IID, &mut raw)
            .ok()
            .ok()?;
        Some(entries(&IObjectArray::from_raw(raw), false))
    }
}

fn destinations(list: &IDestinationList, index: u32) -> Option<Vec<(String, Entry)>> {
    let mut raw = std::ptr::null_mut();
    // SAFETY: The returned pointer is an `IObjectArray`, owned from here.
    unsafe {
        list.EnumerateCategoryDestinations(index, &IObjectArray::IID, &mut raw)
            .ok()
            .ok()?;
        let array = IObjectArray::from_raw(raw);
        let category_is_tasks = {
            let mut category = Category::default();
            list.GetCategory(index, 1, &mut category).is_ok() && category.kind == CATEGORY_TASKS
        };
        Some(entries(&array, category_is_tasks))
    }
}

/// Each entry with the key that tells whether it is already shown.
fn entries(array: &IObjectArray, tasks: bool) -> Vec<(String, Entry)> {
    // SAFETY: Reads the array's own count.
    let count = unsafe { array.GetCount() }.unwrap_or(0);
    (0..count)
        // SAFETY: Each index is below the count the array reports.
        .filter_map(|index| unsafe { array.GetAt::<IUnknown>(index) }.ok())
        .filter_map(|object| entry(&object, tasks))
        .collect()
}

fn entry(object: &IUnknown, tasks: bool) -> Option<(String, Entry)> {
    if let Ok(item) = object.cast::<IShellItem>() {
        // SAFETY: Each returned string and ID list is freed once copied.
        unsafe {
            let label = take_string(item.GetDisplayName(SIGDN_NORMALDISPLAY).ok()?)?;
            let key = item
                .GetDisplayName(SIGDN_DESKTOPABSOLUTEPARSING)
                .ok()
                .and_then(|name| take_string(name))
                .unwrap_or_default()
                .to_uppercase();
            let id_list = copy_id_list(SHGetIDListFromObject(&item).ok()?)?;
            return Some((
                key,
                Entry::Item {
                    label: shorten(&label),
                    target: Target::Item { id_list },
                    icon: icons::item_icon(&item, ICON_SIZE),
                },
            ));
        }
    }
    let link = object.cast::<IShellLinkW>().ok()?;
    let store = link.cast::<IPropertyStore>().ok();
    if tasks
        && store
            .as_ref()
            .is_some_and(|store| property(store, &PKEY_AppUserModel_IsDestListSeparator) == "true")
    {
        return Some((String::new(), Entry::Separator));
    }
    let title = store
        .as_ref()
        .map(|store| property(store, &PKEY_Title))
        .filter(|title| !title.is_empty())
        .map(|title| indirect(&title));
    // SAFETY: The ID list is freed after its names and icon are read.
    let (name, path, target_icon) = unsafe {
        match link.GetIDList() {
            Ok(id_list) if !id_list.is_null() => {
                let name = |kind: SIGDN| {
                    SHGetNameFromIDList(id_list, kind)
                        .ok()
                        .and_then(|name| take_string(name))
                };
                let names = (
                    name(SIGDN_NORMALDISPLAY),
                    name(SIGDN_DESKTOPABSOLUTEPARSING),
                    SHCreateItemFromIDList::<IShellItem>(id_list)
                        .ok()
                        .and_then(|item| icons::item_icon(&item, ICON_SIZE)),
                );
                CoTaskMemFree(Some(id_list as *const c_void));
                names
            }
            _ => (None, None, None),
        }
    };
    let label = title.or(name).filter(|label| !label.is_empty())?;
    let arguments = store
        .as_ref()
        .map(|store| property(store, &PKEY_Link_Arguments))
        .unwrap_or_default();
    let key = format!("{}\u{0}{arguments}", path.as_deref().unwrap_or_default()).to_uppercase();
    let stream = save_link(&link)?;
    // The icon the app chose for the entry; for a Store app's task, which
    // opens the app rather than a file, the app's; else that of what it opens.
    let opens_nothing = path.as_deref().is_none_or(|path| !Path::new(path).exists());
    let icon = link_icon(&link)
        .or_else(|| {
            opens_nothing
                .then(|| {
                    store
                        .as_ref()
                        .map(|store| property(store, &PKEY_AppUserModel_ID))
                })
                .flatten()
                .and_then(|app_id| app_icon(&app_id))
        })
        .or(target_icon);
    Some((
        key,
        Entry::Item {
            label: shorten(&label),
            target: Target::Link { stream },
            icon,
        },
    ))
}

fn link_icon(link: &IShellLinkW) -> Option<IconImage> {
    let mut path = [0u16; 1024];
    let mut index = 0;
    // SAFETY: The buffer and index outlive the call.
    unsafe { link.GetIconLocation(&mut path, &mut index) }.ok()?;
    let len = path.iter().position(|&unit| unit == 0).unwrap_or(0);
    let path = expand_variables(&String::from_utf16_lossy(&path[..len]));
    if path.is_empty() {
        return None;
    }
    icons::load_icon_at(&path, index, ICON_SIZE).ok()
}

/// The icon of the app filed as `app_id` in the Apps folder.
fn app_icon(app_id: &str) -> Option<IconImage> {
    if app_id.is_empty() {
        return None;
    }
    // SAFETY: Only resolves the app's shell item.
    let item = unsafe {
        SHCreateItemInKnownFolder::<_, IShellItem>(
            &FOLDERID_AppsFolder,
            KF_FLAG_DEFAULT,
            &HSTRING::from(app_id),
        )
    }
    .ok()?;
    icons::item_icon(&item, ICON_SIZE)
}

/// Replaces `%NAME%` with the environment variable, as icon locations write
/// `%ProgramFiles%`. Unknown names are kept as they are.
fn expand_variables(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) => {
                let name = &after[..end];
                match env::var(name) {
                    Ok(value) if !name.is_empty() => out.push_str(&value),
                    _ => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

fn save_link(link: &IShellLinkW) -> Option<Vec<u8>> {
    // SAFETY: The buffer has the length the stream reports and is read into
    // only up to it.
    unsafe {
        let persist: IPersistStream = link.cast().ok()?;
        let stream: IStream = SHCreateMemStream(None)?;
        persist.Save(&stream, true).ok()?;
        let mut stat = STATSTG::default();
        stream.Stat(&mut stat, STATFLAG_NONAME).ok()?;
        let length = u32::try_from(stat.cbSize).ok()?;
        stream.Seek(0, STREAM_SEEK_SET, None).ok()?;
        let mut bytes = vec![0u8; length as usize];
        let mut read = 0;
        stream
            .Read(bytes.as_mut_ptr().cast(), length, Some(&mut read))
            .ok()
            .ok()?;
        bytes.truncate(read as usize);
        Some(bytes)
    }
}

fn load_link(stream: &[u8]) -> windows::core::Result<IShellLinkW> {
    // SAFETY: The stream is a copy of `stream`, alive through the load.
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        let memory = SHCreateMemStream(Some(stream))
            .ok_or_else(|| windows::core::Error::from_hresult(E_OUTOFMEMORY))?;
        link.cast::<IPersistStream>()?.Load(&memory)?;
        Ok(link)
    }
}

/// Copies and frees an ID list the shell allocated.
///
/// # Safety
/// `id_list` must be null or a valid ID list allocated with `CoTaskMemAlloc`.
unsafe fn copy_id_list(id_list: *mut ITEMIDLIST) -> Option<Vec<u8>> {
    if id_list.is_null() {
        return None;
    }
    // SAFETY: `ILGetSize` gives the size of the whole list, terminator included.
    unsafe {
        let size = ILGetSize(Some(id_list)) as usize;
        let bytes = std::slice::from_raw_parts(id_list.cast::<u8>(), size).to_vec();
        CoTaskMemFree(Some(id_list as *const c_void));
        is_id_list(&bytes).then_some(bytes)
    }
}

/// `true` when `bytes` is a whole ID list: items whose sizes stay inside it,
/// then the two-byte zero that ends it. Checked before the shell reads an ID
/// list received from another process.
fn is_id_list(bytes: &[u8]) -> bool {
    let mut offset = 0usize;
    loop {
        let Some(size) = bytes
            .get(offset..offset + 2)
            .map(|size| usize::from(u16::from_le_bytes([size[0], size[1]])))
        else {
            return false;
        };
        if size == 0 {
            return offset + 2 == bytes.len();
        }
        if size < 2 {
            return false;
        }
        offset += size;
    }
}

fn open_item(id_list: &[u8], program: Option<&str>) -> windows::core::Result<()> {
    if !is_id_list(id_list) {
        return Err(windows::core::Error::from_hresult(E_INVALIDARG));
    }
    let pointer = id_list.as_ptr().cast::<ITEMIDLIST>();
    let mut path = [0u16; 260];
    // SAFETY: `id_list` is a whole ID list (checked above).
    let file = unsafe { SHGetPathFromIDListW(pointer, &mut path) }
        .as_bool()
        .then(|| {
            let len = path
                .iter()
                .position(|&unit| unit == 0)
                .unwrap_or(path.len());
            String::from_utf16_lossy(&path[..len])
        })
        .filter(|file| Path::new(file).is_file());
    let program = program.filter(|program| tracker::is_program(program));
    let (file_name, parameters) = match (program, file) {
        (Some(program), Some(file)) => (
            Some(HSTRING::from(program)),
            Some(HSTRING::from(format!("\"{file}\""))),
        ),
        _ => (None, None),
    };
    let mut request = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_LOG_USAGE,
        lpVerb: w!("open"),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    if let (Some(file_name), Some(parameters)) = (&file_name, &parameters) {
        request.lpFile = PCWSTR(file_name.as_ptr());
        request.lpParameters = PCWSTR(parameters.as_ptr());
    } else {
        request.fMask |= SEE_MASK_IDLIST;
        request.lpVerb = PCWSTR::null();
        request.lpIDList = pointer.cast_mut().cast();
    }
    // SAFETY: The strings and the ID list live through the call.
    unsafe { ShellExecuteExW(&mut request) }
}

/// A shortcut opens through its default command, as the taskbar does; a
/// Store app's task, through the app with the task's activation context.
fn open_link(stream: &[u8]) -> windows::core::Result<()> {
    let link = load_link(stream)?;
    let mut menu: IContextMenu = link.cast()?;
    let mut parameters = String::new();
    if let Ok(store) = link.cast::<IPropertyStore>() {
        let app_id = property(&store, &PKEY_AppUserModel_ID);
        if !app_id.is_empty() {
            // SAFETY: Only resolves and queries the app's own shell item.
            let packaged = unsafe {
                SHCreateItemInKnownFolder::<_, IShellItem2>(
                    &FOLDERID_AppsFolder,
                    KF_FLAG_DEFAULT,
                    &HSTRING::from(app_id),
                )
            }
            .ok()
            .filter(|app| {
                // SAFETY: Reads a number property of the item.
                unsafe { app.GetUInt32(&PKEY_APP_USER_MODEL_HOST_ENVIRONMENT) }
                    .is_ok_and(|host| host != 0)
            });
            if let Some(app) = packaged {
                // SAFETY: Binds to the item's own context menu.
                let app_menu =
                    unsafe { app.BindToHandler::<_, IContextMenu>(None, &BHID_SFUIObject) };
                if let Ok(app_menu) = app_menu {
                    menu = app_menu;
                    parameters = property(&store, &PKEY_APP_USER_MODEL_ACTIVATION_CONTEXT);
                }
            }
        }
    }
    invoke_default(&menu, &parameters)
}

fn invoke_default(menu: &IContextMenu, parameters: &str) -> windows::core::Result<()> {
    // SAFETY: The popup menu is destroyed before returning; the strings live
    // through the call.
    unsafe {
        let popup = CreatePopupMenu()?;
        let result = (|| {
            menu.QueryContextMenu(popup, 0, 1, 0x7FFF, CMF_DEFAULTONLY)
                .ok()?;
            let id = GetMenuDefaultItem(popup, 0, GET_MENU_DEFAULT_ITEM_FLAGS(0));
            if id == u32::MAX || id == 0 {
                return Err(windows::core::Error::from_hresult(E_FAIL));
            }
            let verb = (id - 1) as usize;
            let parameters = HSTRING::from(parameters);
            let request = CMINVOKECOMMANDINFOEX {
                cbSize: size_of::<CMINVOKECOMMANDINFOEX>() as u32,
                fMask: CMIC_MASK_UNICODE | CMIC_MASK_NOASYNC | CMIC_MASK_FLAG_LOG_USAGE,
                lpVerb: PCSTR(verb as *const u8),
                lpVerbW: PCWSTR(verb as *const u16),
                lpParametersW: if parameters.is_empty() {
                    PCWSTR::null()
                } else {
                    PCWSTR(parameters.as_ptr())
                },
                nShow: SW_SHOWNORMAL.0,
                ..Default::default()
            };
            menu.InvokeCommand((&raw const request).cast::<CMINVOKECOMMANDINFO>())
        })();
        let _ = DestroyMenu(popup);
        result
    }
}

fn property(store: &IPropertyStore, key: &PROPERTYKEY) -> String {
    // SAFETY: Reads a property into an owned value.
    unsafe { store.GetValue(key) }.map_or_else(|_| String::new(), |value| value.to_string())
}

/// Resolves `@dll,-id` and `ms-resource:` labels.
fn indirect(text: &str) -> String {
    if !text.starts_with('@') {
        return text.to_owned();
    }
    let mut buffer = [0u16; 512];
    // SAFETY: Writes into a bounded buffer.
    if unsafe { SHLoadIndirectString(&HSTRING::from(text), &mut buffer, None) }.is_ok() {
        let len = buffer
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(buffer.len());
        if len > 0 {
            return String::from_utf16_lossy(&buffer[..len]);
        }
    }
    text.to_owned()
}

fn shorten(label: &str) -> String {
    label.chars().take(MAX_LABEL).collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_lists_must_end_inside_their_bytes() {
        assert!(is_id_list(&[0, 0]));
        assert!(is_id_list(&[4, 0, 7, 7, 0, 0]));
        assert!(!is_id_list(&[]));
        assert!(!is_id_list(&[4, 0, 7, 7]));
        assert!(!is_id_list(&[9, 0, 7, 7, 0, 0]));
        assert!(!is_id_list(&[1, 0, 0, 0]));
        assert!(!is_id_list(&[0, 0, 0]));
    }

    #[test]
    fn separators_never_start_end_or_double_a_group() {
        let item = |label: &str| {
            (
                label.to_owned(),
                Entry::Item {
                    label: label.to_owned(),
                    target: Target::Item {
                        id_list: vec![0, 0],
                    },
                    icon: None,
                },
            )
        };
        let separator = || (String::new(), Entry::Separator);
        let mut groups = Vec::new();
        push_group(
            &mut groups,
            Section::Tasks,
            vec![
                separator(),
                item("A"),
                separator(),
                separator(),
                item("B"),
                separator(),
            ],
            usize::MAX,
            &mut Vec::new(),
        );
        let shape: Vec<bool> = groups[0]
            .entries
            .iter()
            .map(|entry| matches!(entry, Entry::Separator))
            .collect();
        assert_eq!(shape, [false, true, false]);
    }

    #[test]
    fn items_already_shown_and_beyond_the_room_are_left_out() {
        let item = |key: &str| {
            (
                key.to_owned(),
                Entry::Item {
                    label: key.to_owned(),
                    target: Target::Item {
                        id_list: vec![0, 0],
                    },
                    icon: None,
                },
            )
        };
        let mut groups = Vec::new();
        let mut seen = vec!["PINNED".to_owned()];
        push_group(
            &mut groups,
            Section::Recent,
            vec![item("PINNED"), item("A"), item("B"), item("C")],
            2,
            &mut seen,
        );
        push_group(
            &mut groups,
            Section::Frequent,
            vec![item("PINNED")],
            5,
            &mut seen,
        );
        assert_eq!(groups.len(), 1);
        let labels: Vec<&str> = groups[0]
            .entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Item { label, .. } => Some(label.as_str()),
                Entry::Separator => None,
            })
            .collect();
        assert_eq!(labels, ["A", "B"]);
    }

    #[test]
    fn icon_locations_expand_known_variables_and_keep_the_rest() {
        // SAFETY: Only this test reads the variable.
        unsafe { std::env::set_var("NOSIH_JUMP_TEST", r"C:\Apps") };
        assert_eq!(
            expand_variables(r"%NOSIH_JUMP_TEST%\app.exe"),
            r"C:\Apps\app.exe"
        );
        assert_eq!(expand_variables("%NO_SUCH_VAR_X%\\a"), "%NO_SUCH_VAR_X%\\a");
        assert_eq!(expand_variables("50% off"), "50% off");
        assert_eq!(expand_variables(r"C:\plain.exe"), r"C:\plain.exe");
    }

    /// Prints the jump lists of the dock's usual apps, to check the
    /// undocumented interfaces against a new Windows build.
    #[test]
    #[ignore = "reads this user's jump lists; prints them"]
    fn prints_jump_lists_of_common_apps() {
        use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
        // SAFETY: COM for this test thread.
        let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        let windir = std::env::var("WINDIR").unwrap_or_default();
        let mut ids = vec![
            format!("{windir}\\explorer.exe"),
            format!("{windir}\\notepad.exe"),
            "shell:AppsFolder\\Microsoft.Windows.Explorer".to_owned(),
        ];
        ids.extend(pins::taskbar_pins());
        for id in ids {
            let app = app_id(&id, None);
            println!("{id} -> {app:?}");
            for group in app.as_deref().map(read).unwrap_or_default() {
                println!("  [{:?}]", group.section);
                for entry in &group.entries {
                    match entry {
                        Entry::Item {
                            label,
                            target,
                            icon,
                        } => println!(
                            "    {label} ({}, icon {})",
                            match target {
                                Target::Item { id_list } =>
                                    format!("item, {} bytes", id_list.len()),
                                Target::Link { stream } => {
                                    assert!(load_link(stream).is_ok(), "{label} does not reload");
                                    format!("link, {} bytes", stream.len())
                                }
                            },
                            icon.as_ref().map_or_else(
                                || "none".to_owned(),
                                |icon| format!("{}x{}", icon.width, icon.height)
                            )
                        ),
                        Entry::Separator => println!("    ----"),
                    }
                }
            }
        }
    }
}
