//! End-to-end check of issue #8 (shortcuts with arguments): builds a
//! `wscript.exe "script.vbs"` shortcut in a scratch folder, pins it the way
//! the dock does (with %APPDATA% pointed at the scratch folder) and opens
//! it, then checks that the script really ran. Usage: `spike_shortcut <dir>`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::{
    fs,
    path::Path,
    thread::sleep,
    time::{Duration, Instant},
};

use nosih_shell::{launcher, pins};
use windows::{
    Win32::{
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
            IPersistFile,
        },
        UI::Shell::{IShellLinkW, ShellLink},
    },
    core::{HSTRING, Interface},
};

fn make_link(path: &Path, target: &str, arguments: &str) {
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).unwrap();
        link.SetPath(&HSTRING::from(target)).unwrap();
        link.SetArguments(&HSTRING::from(arguments)).unwrap();
        link.cast::<IPersistFile>()
            .unwrap()
            .Save(&HSTRING::from(path.as_os_str()), true)
            .unwrap();
    }
}

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("dir"));
    fs::create_dir_all(&dir).unwrap();
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        // Only this process: the kept copy goes to the scratch folder.
        std::env::set_var("APPDATA", dir.join("appdata"));
    }
    let marker = dir.join("ran.txt");
    let _ = fs::remove_file(&marker);
    let script = dir.join("Launch-Hidden.vbs");
    fs::write(&script, format!(
        "Set f = CreateObject(\"Scripting.FileSystemObject\").CreateTextFile(\"{}\", True)\r\nf.WriteLine \"ok\"\r\nf.Close\r\n",
        marker.display()
    )).unwrap();
    let wscript = r"C:\Windows\System32\wscript.exe";
    let with_args = dir.join("Launch Hidden.lnk");
    let plain = dir.join("Plain wscript.lnk");
    make_link(&with_args, wscript, &format!("\"{}\"", script.display()));
    make_link(&plain, wscript, "");

    let to_script = dir.join("Script itself.lnk");
    make_link(&to_script, &script.to_string_lossy(), "");
    println!("with arguments  -> {:?}", pins::shortcut_id(&with_args));
    println!("no arguments    -> {:?}", pins::shortcut_id(&plain));
    println!("to a .vbs file  -> {:?}", pins::shortcut_id(&to_script));

    let pinned = pins::keep_shortcut(&with_args).expect("pinned");
    let kept_dir = dir.join("appdata").join("Nosih").join("Pins");
    let kept: Vec<_> = fs::read_dir(&kept_dir)
        .map(|d| d.flatten().map(|e| e.file_name()).collect())
        .unwrap_or_default();
    println!("pin id          -> {pinned} (kept copies: {kept:?})");
    println!(
        "plain pin       -> {:?} (no copy needed)",
        pins::keep_shortcut(&plain)
    );
    let target = pins::launch_target(&pinned);
    println!("opens through   -> {target:?}");
    let cache = nosih_shell::icon_cache::IconCache::new(dir.join("cache"));
    let visual = cache.get(&pinned, 48).expect("visual");
    println!(
        "dock label      -> {:?} (icon {}x{})",
        visual.name, visual.icon.width, visual.icon.height
    );

    let started = Instant::now();
    launcher::open(&target.unwrap_or(pinned.clone())).expect("open");
    while !marker.exists() && started.elapsed() < Duration::from_secs(10) {
        sleep(Duration::from_millis(50));
    }
    println!(
        "script ran      -> {} ({} ms)",
        marker.exists(),
        started.elapsed().as_millis()
    );
    pins::forget_shortcut(&pinned);
    let left = fs::read_dir(&kept_dir).map(|d| d.count()).unwrap_or(0);
    println!("unpinned        -> kept copies left: {left}");
}
