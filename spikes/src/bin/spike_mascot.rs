//! Plays one of the crab's acts (the real `mascot.rs` and `gfx.rs`) on the
//! screen and saves frames of it, to review the choreography. Usage:
//! `spike_mascot <fresh|nervous|exhausted|charging|unplugged|sleepy> <out folder>`.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

#[path = "../../../apps/dock/src/gfx.rs"]
mod gfx;
#[path = "../../../apps/dock/src/mascot.rs"]
mod mascot;

use std::{
    io::Write,
    time::{Duration, Instant},
};

use nosih_shell::screen;
use windows::Win32::{
    Foundation::RECT,
    System::WinRT::{
        CreateDispatcherQueueController, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT,
        DispatcherQueueOptions,
    },
    UI::{
        HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext},
        WindowsAndMessaging::{DispatchMessageW, MSG, PM_REMOVE, PeekMessageW},
    },
};

fn main() {
    let act = match std::env::args().nth(1).as_deref() {
        Some("nervous") => mascot::Act::Nervous,
        Some("exhausted") => mascot::Act::Exhausted,
        Some("charging") => mascot::Act::Charging,
        Some("unplugged") => mascot::Act::Unplugged,
        Some("sleepy") => mascot::Act::Sleepy,
        _ => mascot::Act::Fresh,
    };
    let out = std::path::PathBuf::from(std::env::args().nth(2).expect("out folder"));
    std::fs::create_dir_all(&out).unwrap();
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _queue = CreateDispatcherQueueController(DispatcherQueueOptions {
            dwSize: size_of::<DispatcherQueueOptions>() as u32,
            threadType: DQTYPE_THREAD_CURRENT,
            apartmentType: DQTAT_COM_STA,
        })
        .unwrap();
        let gfx = gfx::Gfx::new().unwrap();
        // Optional exact place: `under <x> <top>` or `along <left> <right> <top>`,
        // for the demo recording (then no frames are saved).
        let numbers: Vec<i32> = std::env::args()
            .skip(4)
            .filter_map(|v| v.parse().ok())
            .collect();
        let exact = match (std::env::args().nth(3).as_deref(), numbers.as_slice()) {
            (Some("under"), [x, top]) => Some(mascot::Place::Under { x: *x, top: *top }),
            // A trailing `left` or `right` plays it on a side dock's inner
            // wall: `left`/`right` then span the dock vertically and `top` is
            // the wall's x.
            (Some("along"), [left, right, top]) => Some(mascot::Place::Along {
                left: *left,
                right: *right,
                top: *top,
                edge: match std::env::args().nth(7).as_deref() {
                    Some("left") => nosih_core::frame::DockEdge::Left,
                    Some("right") => nosih_core::frame::DockEdge::Right,
                    _ => nosih_core::frame::DockEdge::Bottom,
                },
            }),
            _ => None,
        };
        if let Some(place) = exact {
            let mut crab = mascot::Mascot::play(&gfx, mascot::Character::Crab, act, place, 1.25)
                .expect("play");
            let mut msg = MSG::default();
            while !crab.finished() {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    DispatchMessageW(&msg);
                }
                std::thread::sleep(Duration::from_millis(5));
                let _ = &mut crab;
            }
            return;
        }
        let (place, area) = match act {
            mascot::Act::Charging | mascot::Act::Unplugged | mascot::Act::Sleepy => (
                mascot::Place::Along {
                    left: 560,
                    right: 1360,
                    top: 900,
                    edge: nosih_core::frame::DockEdge::Bottom,
                },
                RECT {
                    left: 540,
                    top: 760,
                    right: 1380,
                    bottom: 920,
                },
            ),
            _ => (
                mascot::Place::Under { x: 960, top: 300 },
                RECT {
                    left: 860,
                    top: 290,
                    right: 1060,
                    bottom: 460,
                },
            ),
        };
        let mut crab =
            mascot::Mascot::play(&gfx, mascot::Character::Crab, act, place, 1.25).expect("play");
        let start = Instant::now();
        let marks = [0.15, 0.35, 0.7, 1.2, 1.8, 2.4, 3.1, 4.2];
        let mut next = 0;
        let mut msg = MSG::default();
        while !crab.finished() {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                DispatchMessageW(&msg);
            }
            let t = start.elapsed().as_secs_f32();
            if next < marks.len() && t >= marks[next] {
                if let Some(shot) = screen::capture(area) {
                    let path = out.join(format!("{next}_{:.2}.bgra", marks[next]));
                    let mut file = std::fs::File::create(path).unwrap();
                    file.write_all(&((area.right - area.left) as u32).to_le_bytes())
                        .unwrap();
                    file.write_all(&((area.bottom - area.top) as u32).to_le_bytes())
                        .unwrap();
                    file.write_all(&shot.bgra).unwrap();
                }
                next += 1;
            }
            std::thread::sleep(Duration::from_millis(5));
            let _ = &mut crab;
        }
        println!("act over after {:.2} s", start.elapsed().as_secs_f32());
    }
}
