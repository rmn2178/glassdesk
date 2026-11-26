//! Prints what the dock would import from the Windows taskbar's pinned apps,
//! whether each one still exists, the input language indicator and which
//! privacy indicators (microphone, camera, location) are on.

use nosih_shell::{
    pins::{app_exists, taskbar_pins},
    status::privacy,
    system::input_language,
};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};

fn main() {
    // SAFETY: COM for this thread, needed to read shortcuts and shell items.
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    for (index, id) in taskbar_pins().iter().enumerate() {
        println!("{}. {id} (exists: {})", index + 1, app_exists(id));
    }
    for id in [
        r"c:\program files\no-such-app\gone.exe",
        r"shell:AppsFolder\Not.Installed_8wekyb3d8bbwe!App",
    ] {
        println!("missing check: {id} -> exists {}", app_exists(id));
    }
    println!("input language: {:?}", input_language());
    println!("privacy: {:?}", privacy());
}
