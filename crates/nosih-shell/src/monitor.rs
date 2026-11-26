//! CPU, memory and network speed for the menu bar's system monitor (issue
//! #64), as iStat Menus or Stats show them on macOS.
//!
//! Each reading is three cheap kernel queries: no performance counters, no
//! process list, no thread of its own. The caller samples only while the
//! option is on and the bar is on screen.

use std::time::Instant;

use windows::Win32::{
    Foundation::{FILETIME, NO_ERROR},
    NetworkManagement::{
        IpHelper::{FreeMibTable, GetIfTable2, IF_TYPE_SOFTWARE_LOOPBACK, MIB_IF_TABLE2},
        Ndis::IfOperStatusUp,
    },
    System::{
        SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX},
        Threading::GetSystemTimes,
    },
};

/// What the monitor shows; `None` until there is a value to show (CPU and
/// network need two samples) or if Windows refused the query.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reading {
    /// Share of the processor time in use since the last sample, 0 to 100.
    pub cpu: Option<u8>,
    /// Share of physical memory in use, 0 to 100, as Task Manager counts it.
    pub memory: Option<u8>,
    /// Bytes per second received and sent by the network adapters.
    pub down: Option<u64>,
    pub up: Option<u64>,
}

/// Counters at one moment; the reading is the change between two of them.
#[derive(Clone, Copy, Debug)]
struct Totals {
    at: Instant,
    /// Processor time idle, and in total (kernel time includes idle time),
    /// in 100 ns units.
    cpu: Option<(u64, u64)>,
    /// Bytes received and sent since the adapters started.
    network: Option<(u64, u64)>,
}

/// Turns counters into rates between consecutive calls.
#[derive(Debug, Default)]
pub struct Sampler {
    last: Option<Totals>,
}

impl Sampler {
    /// Takes a sample. The first one has no CPU or network values yet.
    pub fn sample(&mut self) -> Reading {
        let now = Totals {
            at: Instant::now(),
            cpu: cpu_times(),
            network: network_bytes(),
        };
        let reading = Reading {
            memory: memory_load(),
            ..self
                .last
                .map_or_else(Reading::default, |last| between(&last, &now))
        };
        self.last = Some(now);
        reading
    }

    /// Forgets the last sample: after a pause, the next reading would
    /// otherwise average over all of it.
    pub fn reset(&mut self) {
        self.last = None;
    }
}

fn between(last: &Totals, now: &Totals) -> Reading {
    let seconds = now.at.duration_since(last.at).as_secs_f64();
    let cpu = last
        .cpu
        .zip(now.cpu)
        .and_then(|((idle_then, total_then), (idle_now, total_now))| {
            cpu_percent(
                idle_now.checked_sub(idle_then)?,
                total_now.checked_sub(total_then)?,
            )
        });
    let (down, up) = last.network.zip(now.network).map_or(
        (None, None),
        |((in_then, out_then), (in_now, out_now))| {
            (
                rate(in_then, in_now, seconds),
                rate(out_then, out_now, seconds),
            )
        },
    );
    Reading {
        cpu,
        memory: None,
        down,
        up,
    }
}

/// Busy share of `total` processor time, `idle` of it idle.
fn cpu_percent(idle: u64, total: u64) -> Option<u8> {
    if total == 0 {
        return None;
    }
    let busy = total.saturating_sub(idle) as f64 / total as f64;
    Some((busy * 100.0).round().clamp(0.0, 100.0) as u8)
}

/// Bytes per second from two counter values. A counter that went back (an
/// adapter reset or removed) gives no value rather than a huge one.
fn rate(then: u64, now: u64, seconds: f64) -> Option<u64> {
    (seconds > 0.0)
        .then(|| now.checked_sub(then))
        .flatten()
        .map(|bytes| (bytes as f64 / seconds).round() as u64)
}

fn cpu_times() -> Option<(u64, u64)> {
    let (mut idle, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    // SAFETY: Three valid out parameters for the duration of the call.
    unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) }.ok()?;
    let ticks =
        |time: FILETIME| u64::from(time.dwHighDateTime) << 32 | u64::from(time.dwLowDateTime);
    Some((ticks(idle), ticks(kernel) + ticks(user)))
}

fn memory_load() -> Option<u8> {
    let mut status = MEMORYSTATUSEX {
        dwLength: size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: `status` has its size set and lives for the call.
    unsafe { GlobalMemoryStatusEx(&mut status) }.ok()?;
    u8::try_from(status.dwMemoryLoad.min(100)).ok()
}

/// Bytes received and sent by the physical adapters that are up. Virtual
/// ones (VPNs, Hyper-V switches) and the filter layers Windows stacks on
/// each adapter repeat the same traffic, so they are left out, as is the
/// loopback.
fn network_bytes() -> Option<(u64, u64)> {
    const HARDWARE_INTERFACE: u8 = 1;
    const FILTER_INTERFACE: u8 = 1 << 1;
    let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
    // SAFETY: Windows allocates the table; it is freed below, once.
    if unsafe { GetIfTable2(&mut table) } != NO_ERROR || table.is_null() {
        return None;
    }
    // SAFETY: `NumEntries` rows follow the header in the same allocation.
    let rows = unsafe {
        std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize)
    };
    let totals = rows
        .iter()
        .filter(|row| {
            let flags = row.InterfaceAndOperStatusFlags._bitfield;
            flags & HARDWARE_INTERFACE != 0
                && flags & FILTER_INTERFACE == 0
                && row.OperStatus == IfOperStatusUp
                && row.Type != IF_TYPE_SOFTWARE_LOOPBACK
        })
        .fold((0u64, 0u64), |(received, sent), row| {
            (
                received.saturating_add(row.InOctets),
                sent.saturating_add(row.OutOctets),
            )
        });
    // SAFETY: The table GetIfTable2 returned, not used after this.
    unsafe { FreeMibTable(table.cast()) };
    Some(totals)
}

/// A network speed as the bar shows it, at most 4 characters plus the
/// unit: "0 KB/s", "850 KB/s", "1.2 MB/s", "35 MB/s", "1.1 GB/s". `comma`
/// writes the decimal mark as a comma (Spanish, French, Portuguese).
#[must_use]
pub fn format_rate(bytes_per_second: u64, comma: bool) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let value = bytes_per_second as f64;
    let decimal = |amount: f64, unit: &str| {
        let text = format!("{amount:.1} {unit}");
        if comma {
            text.replacen('.', ",", 1)
        } else {
            text
        }
    };
    // Each unit starts where the previous one would round to 4 digits.
    if value < 999.5 * KB {
        format!("{} KB/s", (value / KB).round())
    } else if value < 9.95 * MB {
        decimal((value / MB).max(1.0), "MB/s")
    } else if value < 999.5 * MB {
        format!("{} MB/s", (value / MB).round())
    } else {
        decimal(value / GB, "GB/s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_share_is_the_busy_part_of_the_elapsed_time() {
        assert_eq!(cpu_percent(750, 1000), Some(25));
        assert_eq!(cpu_percent(0, 1000), Some(100));
        assert_eq!(cpu_percent(1000, 1000), Some(0));
        // No time passed: nothing to say.
        assert_eq!(cpu_percent(0, 0), None);
    }

    #[test]
    fn rates_are_per_second_and_ignore_counters_that_went_back() {
        assert_eq!(rate(1000, 3000, 2.0), Some(1000));
        assert_eq!(rate(3000, 1000, 2.0), None);
        assert_eq!(rate(1000, 3000, 0.0), None);
    }

    #[test]
    fn speeds_keep_to_four_digits_and_the_chosen_decimal_mark() {
        assert_eq!(format_rate(0, false), "0 KB/s");
        assert_eq!(format_rate(300, false), "0 KB/s");
        assert_eq!(format_rate(850 * 1024, false), "850 KB/s");
        assert_eq!(format_rate(1024 * 1024 * 6 / 5, false), "1.2 MB/s");
        assert_eq!(format_rate(1024 * 1024 * 6 / 5, true), "1,2 MB/s");
        // Just under a megabyte reads as one, not as "1000 KB/s".
        assert_eq!(format_rate(1000 * 1024, false), "1.0 MB/s");
        assert_eq!(format_rate(35 * 1024 * 1024, false), "35 MB/s");
        assert_eq!(format_rate(1200 * 1024 * 1024, true), "1,2 GB/s");
    }

    #[test]
    fn the_first_sample_has_memory_but_not_yet_rates() {
        let mut sampler = Sampler::default();
        let first = sampler.sample();
        assert!(first.cpu.is_none() && first.down.is_none() && first.up.is_none());
        assert!(first.memory.is_some_and(|load| load <= 100));
        std::thread::sleep(std::time::Duration::from_millis(30));
        let second = sampler.sample();
        assert!(second.cpu.is_some_and(|cpu| cpu <= 100));
        assert!(second.down.is_some() && second.up.is_some());
        sampler.reset();
        assert!(sampler.sample().cpu.is_none());
    }
}
