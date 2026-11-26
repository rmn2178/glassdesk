//! Times copying a window-sized rectangle of the screen, the step that delays
//! the genie's take-off.

use std::time::Instant;

use nosih_shell::screen;
use windows::Win32::{
    Foundation::RECT,
    Graphics::Gdi::{
        BitBlt, CAPTUREBLT, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject,
        GetDC, ReleaseDC, SRCCOPY, SelectObject,
    },
};

fn main() {
    let rect = RECT {
        left: 200,
        top: 100,
        right: 1500,
        bottom: 900,
    };
    for round in 0..6 {
        let start = Instant::now();
        let image = screen::capture(rect);
        println!(
            "round {round}: screen::capture {:.1} ms ({})",
            start.elapsed().as_secs_f32() * 1000.0,
            image.is_some()
        );
        // SAFETY: Every GDI object is released before the block ends.
        unsafe {
            let screen_dc = GetDC(None);
            let memory = CreateCompatibleDC(Some(screen_dc));
            let bitmap = CreateCompatibleBitmap(screen_dc, 1300, 800);
            let previous = SelectObject(memory, bitmap.into());
            for (name, rop) in [("plain", SRCCOPY), ("captureblt", SRCCOPY | CAPTUREBLT)] {
                let start = Instant::now();
                let _ = BitBlt(memory, 0, 0, 1300, 800, Some(screen_dc), 200, 100, rop);
                println!(
                    "  BitBlt {name}: {:.1} ms",
                    start.elapsed().as_secs_f32() * 1000.0
                );
            }
            SelectObject(memory, previous);
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(memory);
            ReleaseDC(None, screen_dc);
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
}
