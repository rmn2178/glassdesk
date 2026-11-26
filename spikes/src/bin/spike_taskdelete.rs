//! Checks that a scheduled task can be deleted through the Task Scheduler API
//! (what `autostart::delete_legacy_task` does), with a throwaway task.
//! Usage: `spike_taskdelete <name>`; prints whether it existed and is gone.

use windows::{
    Win32::System::{
        Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx},
        TaskScheduler::{ITaskService, TaskScheduler},
        Variant::VARIANT,
    },
    core::{BSTR, Result},
};

fn main() -> Result<()> {
    let name = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "NosihTest-borrar".into());
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let service: ITaskService = CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER)?;
        let none = VARIANT::default();
        service.Connect(&none, &none, &none, &none)?;
        let folder = service.GetFolder(&BSTR::from("\\"))?;
        let existed = folder.GetTask(&BSTR::from(name.as_str())).is_ok();
        let deleted = folder.DeleteTask(&BSTR::from(name.as_str()), 0);
        let still = folder.GetTask(&BSTR::from(name.as_str())).is_ok();
        println!("existed={existed} delete={deleted:?} still_there={still}");
    }
    Ok(())
}
