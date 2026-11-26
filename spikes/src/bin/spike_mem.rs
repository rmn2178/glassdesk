//! Measures the memory cost of each part of the dock's graphics stack to guide
//! optimization with data rather than assumptions.
//!
//! Usage: `cargo run --release -p spikes --bin spike_mem`

use std::mem::size_of;

use windows::{
    UI::Composition::Compositor,
    Win32::{
        Foundation::HMODULE,
        Graphics::{
            Direct2D::D2D1CreateDevice,
            Direct3D::{D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP},
            Direct3D11::{
                D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_SINGLETHREADED,
                D3D11_SDK_VERSION, D3D11CreateDevice,
            },
            DirectWrite::{DWRITE_FACTORY_TYPE_SHARED, DWriteCreateFactory, IDWriteFactory},
            Dxgi::IDXGIDevice,
        },
        System::{
            ProcessStatus::{
                EmptyWorkingSet, GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
                PROCESS_MEMORY_COUNTERS_EX,
            },
            Threading::GetCurrentProcess,
            WinRT::{
                Composition::ICompositorInterop, CreateDispatcherQueueController, DQTAT_COM_STA,
                DQTYPE_THREAD_CURRENT, DispatcherQueueOptions,
            },
        },
    },
    core::{Interface, Result},
};

fn report(stage: &str) {
    let mut counters = PROCESS_MEMORY_COUNTERS_EX {
        cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        ..Default::default()
    };
    // SAFETY: The structure has its size initialized; PROCESS_MEMORY_COUNTERS_EX
    // begins with the fields of PROCESS_MEMORY_COUNTERS.
    unsafe {
        let _ = GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&raw mut counters).cast::<PROCESS_MEMORY_COUNTERS>(),
            counters.cb,
        );
    }
    let mb = |bytes: usize| bytes as f64 / 1_048_576.0;
    println!(
        "{stage:<44} working set {:>6.1} MB   privada (commit) {:>6.1} MB",
        mb(counters.WorkingSetSize),
        mb(counters.PrivateUsage)
    );
}

fn device(kind: D3D_DRIVER_TYPE, single_thread: bool) -> Result<IDXGIDevice> {
    let mut flags = D3D11_CREATE_DEVICE_BGRA_SUPPORT;
    if single_thread {
        flags |= D3D11_CREATE_DEVICE_SINGLETHREADED;
    }
    let mut d3d = None;
    // SAFETY: The output pointer refers to `d3d`, which remains alive for the call.
    unsafe {
        D3D11CreateDevice(
            None,
            kind,
            HMODULE::default(),
            flags,
            None,
            D3D11_SDK_VERSION,
            Some(&mut d3d),
            None,
            None,
        )?;
    }
    d3d.ok_or_else(windows::core::Error::empty)?.cast()
}

fn main() -> Result<()> {
    let warp = std::env::args().any(|a| a == "--warp");
    let single = std::env::args().any(|a| a == "--single");
    report("0. arranque");
    // SAFETY: The dispatcher options remain valid for this current-thread call.
    let _queue = unsafe {
        CreateDispatcherQueueController(DispatcherQueueOptions {
            dwSize: size_of::<DispatcherQueueOptions>() as u32,
            threadType: DQTYPE_THREAD_CURRENT,
            apartmentType: DQTAT_COM_STA,
        })?
    };
    let compositor = Compositor::new()?;
    report("1. + compositor");
    let kind = if warp {
        D3D_DRIVER_TYPE_WARP
    } else {
        D3D_DRIVER_TYPE_HARDWARE
    };
    let dxgi = device(kind, single)?;
    report(if warp {
        "2. + Direct3D (WARP, por CPU)"
    } else {
        "2. + Direct3D (GPU)"
    });
    // SAFETY: `dxgi` is a live device, and returned COM interfaces are owned by their wrappers.
    let _graphics = unsafe {
        let d2d = D2D1CreateDevice(&dxgi, None)?;
        compositor
            .cast::<ICompositorInterop>()?
            .CreateGraphicsDevice(&d2d)?
    };
    report("3. + Direct2D y dispositivo de composición");
    // SAFETY: Windows initializes the returned COM interface, which is held by `_dwrite`.
    let _dwrite: IDWriteFactory = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)? };
    report("4. + DirectWrite");
    // SAFETY: Trims this process's own working set.
    unsafe {
        let _ = EmptyWorkingSet(GetCurrentProcess());
    }
    report("5. tras recortar el working set");
    Ok(())
}
