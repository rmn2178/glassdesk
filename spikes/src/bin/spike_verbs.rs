//! Lists the shell context menu verbs of the items pinned to the taskbar,
//! without invoking any: is "taskbarunpin" there for `pins::unpin_from_taskbar`?

use std::{env, fs, path::PathBuf};

use windows::{
    Win32::{
        System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx},
        UI::{
            Shell::{
                BHID_SFUIObject, CMF_NORMAL, GCS_VERBW, IContextMenu, IShellItem,
                SHCreateItemFromParsingName,
            },
            WindowsAndMessaging::{CreatePopupMenu, DestroyMenu, GetMenuItemCount, GetMenuItemID},
        },
    },
    core::{HSTRING, PSTR},
};

fn verbs(path: &str) -> Vec<String> {
    let mut out = Vec::new();
    unsafe {
        let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(path), None)
        else {
            return vec!["(no item)".into()];
        };
        let Ok(menu) = item.BindToHandler::<_, IContextMenu>(None, &BHID_SFUIObject) else {
            return vec!["(no menu)".into()];
        };
        let Ok(popup) = CreatePopupMenu() else {
            return out;
        };
        if menu
            .QueryContextMenu(popup, 0, 1, 0x7FFF, CMF_NORMAL)
            .is_ok()
        {
            for position in 0..GetMenuItemCount(Some(popup)).max(0) {
                let command = GetMenuItemID(popup, position);
                if command == u32::MAX || command == 0 {
                    continue;
                }
                let mut name = [0u16; 64];
                if menu
                    .GetCommandString(
                        (command - 1) as usize,
                        GCS_VERBW,
                        None,
                        PSTR(name.as_mut_ptr().cast()),
                        name.len() as u32,
                    )
                    .is_ok()
                {
                    let len = name.iter().position(|&u| u == 0).unwrap_or(0);
                    if len > 0 {
                        out.push(String::from_utf16_lossy(&name[..len]));
                    }
                }
            }
        }
        let _ = DestroyMenu(popup);
    }
    out
}

fn main() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let folder = PathBuf::from(env::var("APPDATA").unwrap_or_default())
        .join(r"Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar");
    for entry in fs::read_dir(folder).into_iter().flatten().flatten() {
        let path = entry.path().to_string_lossy().into_owned();
        println!(
            "{} -> {:?}",
            entry.file_name().to_string_lossy(),
            verbs(&path)
        );
    }
    for id in nosih_shell::pins::taskbar_pins()
        .iter()
        .filter(|id| id.starts_with("shell:"))
    {
        println!("{id} -> {:?}", verbs(id));
    }
}
