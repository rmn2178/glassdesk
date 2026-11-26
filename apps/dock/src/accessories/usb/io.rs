//! Native HID operations used only inside a disposable query process.
use super::protocol::{self, Model, Profile, Reading, Wire};
use anyhow::{Result, bail, ensure};
use std::time::{Duration, Instant};
use windows::{
    Win32::{
        Devices::HumanInterfaceDevice::{
            HIDD_ATTRIBUTES, HIDP_BUTTON_CAPS, HIDP_CAPS, HIDP_VALUE_CAPS, HidD_FreePreparsedData,
            HidD_GetAttributes, HidD_GetPreparsedData, HidP_GetButtonCaps, HidP_GetCaps,
            HidP_GetValueCaps, HidP_Output, PHIDP_PREPARSED_DATA,
        },
        Foundation::{ERROR_IO_PENDING, GENERIC_READ, GENERIC_WRITE, HANDLE, WAIT_OBJECT_0},
        Storage::FileSystem::{
            CreateFileW, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
            ReadFile, WriteFile,
        },
        System::{
            IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED},
            Threading::{CreateEventW, ResetEvent, WaitForMultipleObjects, WaitForSingleObject},
        },
    },
    core::{HSTRING, Owned},
};

struct Preparsed(PHIDP_PREPARSED_DATA);
impl Drop for Preparsed {
    fn drop(&mut self) {
        unsafe {
            HidD_FreePreparsedData(self.0);
        }
    }
}
struct Hid {
    handle: Owned<HANDLE>,
    input: usize,
    output: usize,
    until: Instant,
}
impl Hid {
    fn open(path: &str, profile: Profile) -> Result<Self> {
        // Metadata-only handle first; do not send anything before actual USB
        // identity, vendor collection and report bounds have been verified.
        let path = HSTRING::from(path);
        unsafe {
            let metadata = Owned::new(CreateFileW(
                &path,
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                None,
            )?);
            let mut attributes = HIDD_ATTRIBUTES {
                Size: size_of::<HIDD_ATTRIBUTES>() as u32,
                ..Default::default()
            };
            ensure!(
                HidD_GetAttributes(*metadata, &mut attributes),
                "HID identity unavailable"
            );
            ensure!(
                attributes.VendorID == profile.vendor && attributes.ProductID == profile.product,
                "HID identity changed"
            );
            let mut data = PHIDP_PREPARSED_DATA::default();
            ensure!(
                HidD_GetPreparsedData(*metadata, &mut data),
                "HID descriptor unavailable"
            );
            let data = Preparsed(data);
            let mut caps = HIDP_CAPS::default();
            HidP_GetCaps(data.0, &mut caps).ok()?;
            let (input, output) = (
                usize::from(caps.InputReportByteLength),
                usize::from(caps.OutputReportByteLength),
            );
            ensure!(
                profile.accepts_collection(caps.UsagePage, caps.Usage, input, output),
                "unsupported HID collection or report sizes"
            );
            ensure!(
                output_report_present(data.0, &caps, profile.output_report(output))?,
                "HID output report unavailable"
            );
            let handle = Owned::new(CreateFileW(
                &path,
                GENERIC_READ.0 | GENERIC_WRITE.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                None,
            )?);
            Ok(Self {
                handle,
                input,
                output,
                until: Instant::now() + Duration::from_millis(1250),
            })
        }
    }
    fn transfer(&self, buffer: &mut [u8], write: bool, until: Instant) -> Result<usize> {
        // All buffers and OVERLAPPED stay alive through completion/cancellation.
        // A broken cancellation is ultimately bounded by the process supervisor.
        unsafe {
            let event = Owned::new(CreateEventW(None, true, false, None)?);
            let mut operation = OVERLAPPED {
                hEvent: *event,
                ..Default::default()
            };
            let result = if write {
                WriteFile(*self.handle, Some(buffer), None, Some(&mut operation))
            } else {
                ReadFile(*self.handle, Some(buffer), None, Some(&mut operation))
            };
            if let Err(error) = result {
                if error.code() != ERROR_IO_PENDING.to_hresult() {
                    return Err(error.into());
                }
                let wait = until
                    .saturating_duration_since(Instant::now())
                    .as_millis()
                    .min(150) as u32;
                if WaitForSingleObject(*event, wait) != WAIT_OBJECT_0 {
                    let _ = CancelIoEx(*self.handle, Some(&operation));
                    let mut discarded = 0;
                    let _ = GetOverlappedResult(*self.handle, &operation, &mut discarded, true);
                    bail!("HID operation expired");
                }
            }
            let mut bytes = 0;
            GetOverlappedResult(*self.handle, &operation, &mut bytes, false)?;
            Ok(bytes as usize)
        }
    }
}

fn output_report_present(
    data: PHIDP_PREPARSED_DATA,
    caps: &HIDP_CAPS,
    expected: u8,
) -> Result<bool> {
    // Descriptors are untrusted input. Bound temporary metadata allocations;
    // both value arrays and button arrays may describe a vendor output report.
    ensure!(
        caps.NumberOutputValueCaps <= 256 && caps.NumberOutputButtonCaps <= 256,
        "oversized HID descriptor"
    );
    let mut values = vec![HIDP_VALUE_CAPS::default(); caps.NumberOutputValueCaps as usize];
    let mut count = caps.NumberOutputValueCaps;
    if count != 0 {
        unsafe {
            HidP_GetValueCaps(HidP_Output, values.as_mut_ptr(), &mut count, data).ok()?;
        }
        ensure!(
            count as usize <= values.len(),
            "invalid HID output descriptor"
        );
        if values[..count as usize]
            .iter()
            .any(|cap| cap.ReportID == expected)
        {
            return Ok(true);
        }
    }
    let mut buttons = vec![HIDP_BUTTON_CAPS::default(); caps.NumberOutputButtonCaps as usize];
    count = caps.NumberOutputButtonCaps;
    if count != 0 {
        unsafe {
            HidP_GetButtonCaps(HidP_Output, buttons.as_mut_ptr(), &mut count, data).ok()?;
        }
        ensure!(
            count as usize <= buttons.len(),
            "invalid HID output descriptor"
        );
        return Ok(buttons[..count as usize]
            .iter()
            .any(|cap| cap.ReportID == expected));
    }
    Ok(false)
}
impl Wire for Hid {
    fn exchange(&mut self, request: &[u8], matches: impl Fn(&[u8]) -> bool) -> Result<Vec<u8>> {
        ensure!(
            Instant::now() < self.until && request.len() <= self.output,
            "HID query budget exhausted"
        );
        let end = self.until.min(Instant::now() + Duration::from_millis(150));
        let mut output = vec![0; self.output];
        output[..request.len()].copy_from_slice(request);
        let mut input = PendingRead::new(self)?;
        input.start()?;
        ensure!(
            self.transfer(&mut output, true, end)? == self.output,
            "incomplete HID request"
        );
        for _ in 0..16 {
            let remaining = end.saturating_duration_since(Instant::now()).as_millis() as u32;
            ensure!(
                remaining > 0
                    && unsafe { WaitForSingleObject(*input.event, remaining) } == WAIT_OBJECT_0,
                "HID response expired"
            );
            let count = input.complete()?;
            if matches(&input.buffer[..count]) {
                return Ok(input.buffer[..count].to_vec());
            }
            input.start()?;
        }
        bail!("unrelated HID responses exceeded budget")
    }
}

struct PendingRead<'a> {
    hid: &'a Hid,
    buffer: Vec<u8>,
    operation: Box<OVERLAPPED>,
    event: Owned<HANDLE>,
    active: bool,
}
impl<'a> PendingRead<'a> {
    fn new(hid: &'a Hid) -> Result<Self> {
        Ok(Self {
            hid,
            buffer: vec![0; hid.input],
            operation: Box::default(),
            event: unsafe { Owned::new(CreateEventW(None, true, false, None)?) },
            active: false,
        })
    }
    fn start(&mut self) -> Result<()> {
        ensure!(!self.active, "HID read still pending");
        unsafe {
            ResetEvent(*self.event)?;
            *self.operation = OVERLAPPED {
                hEvent: *self.event,
                ..Default::default()
            };
            let result = ReadFile(
                *self.hid.handle,
                Some(&mut self.buffer),
                None,
                Some(&mut *self.operation),
            );
            if let Err(error) = result
                && error.code() != ERROR_IO_PENDING.to_hresult()
            {
                return Err(error.into());
            }
        }
        self.active = true;
        Ok(())
    }
    fn complete(&mut self) -> Result<usize> {
        let mut bytes = 0;
        unsafe {
            GetOverlappedResult(*self.hid.handle, &*self.operation, &mut bytes, false)?;
        }
        self.active = false;
        ensure!(
            bytes as usize <= self.buffer.len(),
            "oversized HID response"
        );
        Ok(bytes as usize)
    }
}
impl Drop for PendingRead<'_> {
    fn drop(&mut self) {
        if self.active {
            // This is exclusively in the disposable child. Its supervisor can
            // terminate a broken driver wait without blocking the menu bar.
            unsafe {
                let _ = CancelIoEx(*self.hid.handle, Some(&*self.operation));
                let mut discarded = 0;
                let _ =
                    GetOverlappedResult(*self.hid.handle, &*self.operation, &mut discarded, true);
            }
        }
    }
}

/// How long one HID++ request through a receiver may wait for its reply.
/// The radio wakes a mouse in power saving first (a G305 spends most of its
/// time there), which takes longer than a wired reply; the reference monitor
/// waits 400 ms for its ping. The query's total budget is unchanged.
const HIDPP_REPLY: Duration = Duration::from_millis(400);

struct LogitechWire {
    collections: Vec<Hid>,
    until: Instant,
}
impl Wire for LogitechWire {
    fn exchange(&mut self, request: &[u8], matches: impl Fn(&[u8]) -> bool) -> Result<Vec<u8>> {
        ensure!(Instant::now() < self.until, "HID query budget exhausted");
        let output = self
            .collections
            .iter()
            .max_by_key(|hid| hid.output)
            .ok_or_else(|| anyhow::anyhow!("missing HID collection"))?;
        ensure!(request.len() == 20, "invalid HID++ request size");
        if output.output == 7 {
            ensure!(
                request[7..].iter().all(|b| *b == 0),
                "HID++ request needs long reports"
            );
        }
        let mut packet = request[..output.output].to_vec();
        packet[0] = if output.output == 7 { 0x10 } else { 0x11 };
        let end = self.until.min(Instant::now() + HIDPP_REPLY);
        let mut reads = self
            .collections
            .iter()
            .map(PendingRead::new)
            .collect::<Result<Vec<_>>>()?;
        for read in &mut reads {
            read.start()?;
        }
        ensure!(
            output.transfer(&mut packet, true, end)? == packet.len(),
            "incomplete HID request"
        );
        let events: Vec<_> = reads.iter().map(|read| *read.event).collect();
        for _ in 0..16 {
            let remaining = end.saturating_duration_since(Instant::now()).as_millis() as u32;
            ensure!(remaining > 0, "HID response expired");
            let selected = unsafe { WaitForMultipleObjects(&events, false, remaining) };
            let index = selected.0.wrapping_sub(WAIT_OBJECT_0.0) as usize;
            ensure!(index < reads.len(), "HID response expired or failed");
            let read = &mut reads[index];
            let count = read.complete()?;
            if matches(&read.buffer[..count]) {
                return Ok(read.buffer[..count].to_vec());
            }
            read.start()?;
        }
        bail!("unrelated HID responses exceeded budget")
    }
}

pub fn query(paths: &[String]) -> Result<Vec<Reading>> {
    ensure!(
        (1..=2).contains(&paths.len()),
        "invalid HID collection count"
    );
    let profile = protocol::profile_path(&paths[0])
        .ok_or_else(|| anyhow::anyhow!("unrecognized HID target"))?;
    let key = protocol::receiver_key(&paths[0]);
    ensure!(
        key.is_some() && paths.iter().all(|path| protocol::receiver_key(path) == key),
        "HID collections belong to different receivers"
    );
    let collections = paths
        .iter()
        .map(|path| Hid::open(path, profile))
        .collect::<Result<Vec<_>>>()?;
    match profile.model {
        Model::Logitech | Model::LogitechHeadset => {
            ensure!(
                collections.len() == 1 || collections[0].input != collections[1].input,
                "duplicate HID++ collections"
            );
            let mut hid = LogitechWire {
                collections,
                until: Instant::now() + Duration::from_millis(1250),
            };
            let mut found = Vec::new();
            if profile.model == Model::LogitechHeadset {
                return Ok(
                    protocol::logitech_headset(&mut hid, std::process::id() as u8)?
                        .into_iter()
                        .collect(),
                );
            }
            for slot in 1..=profile.slots {
                let nonce = (std::process::id() as u8).wrapping_add(slot);
                if let Ok(Some(reading)) = protocol::logitech(&mut hid, slot, nonce) {
                    found.push(reading);
                }
            }
            Ok(found)
        }
        model => {
            ensure!(collections.len() == 1, "unexpected headset collections");
            let mut hid = collections
                .into_iter()
                .next()
                .ok_or_else(|| anyhow::anyhow!("missing HID collection"))?;
            Ok(super::headsets::read(model, &mut hid)?
                .into_iter()
                .collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::{
        Foundation::ERROR_PIPE_CONNECTED,
        Storage::FileSystem::PIPE_ACCESS_DUPLEX,
        System::Pipes::{
            ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_MESSAGE, PIPE_TYPE_MESSAGE, PIPE_WAIT,
        },
    };

    // Native overlapped I/O against local fixtures, never a real HID device.
    fn collection(size: usize, suffix: &str) -> Result<(Owned<HANDLE>, Hid)> {
        let name = HSTRING::from(format!(
            r"\\.\pipe\nosih-hid-{}-{}-{suffix}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        unsafe {
            let server = CreateNamedPipeW(
                &name,
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT,
                1,
                512,
                512,
                0,
                None,
            );
            ensure!(!server.is_invalid(), "test pipe unavailable");
            let server = Owned::new(server);
            let client = Owned::new(CreateFileW(
                &name,
                GENERIC_READ.0 | GENERIC_WRITE.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                None,
            )?);
            if let Err(error) = ConnectNamedPipe(*server, None)
                && error.code() != ERROR_PIPE_CONNECTED.to_hresult()
            {
                return Err(error.into());
            }
            Ok((
                server,
                Hid {
                    handle: client,
                    input: size,
                    output: size,
                    until: Instant::now() + Duration::from_secs(2),
                },
            ))
        }
    }

    #[test]
    fn headset_exchange_pads_output_and_cancels_a_timed_out_read() -> Result<()> {
        let (server, mut hid) = collection(31, "headset")?;
        let mut response = [0; 31];
        response[..4].copy_from_slice(&[0x21, 0xbb, 3, 0]);
        unsafe {
            let mut count = 0;
            WriteFile(*server, Some(&response), Some(&mut count), None)?;
        }
        let reply = hid.exchange(&[0x21, 0xbb, 3], |raw| raw.starts_with(&[0x21, 0xbb, 3]))?;
        assert_eq!(reply, response);
        let mut sent = [0; 31];
        unsafe {
            let mut count = 0;
            ReadFile(*server, Some(&mut sent), Some(&mut count), None)?;
            assert_eq!(count, 31);
        }
        assert_eq!(&sent[..3], &[0x21, 0xbb, 3]);
        assert!(sent[3..].iter().all(|byte| *byte == 0));
        let start = Instant::now();
        assert!(hid.exchange(&[0x21, 0xbb, 3], |_| true).is_err());
        assert!(start.elapsed() < Duration::from_secs(1));
        // A cancelled overlapped read must not retain or corrupt the next buffer.
        let mut read = PendingRead::new(&hid)?;
        read.start()?;
        drop(read);
        Ok(())
    }

    #[test]
    fn long_requests_receive_short_or_long_replies_and_cancel_the_other_read() -> Result<()> {
        for size in [7, 20] {
            let (short_server, short) = collection(7, "short")?;
            let (long_server, long) = collection(20, "long")?;
            let request = protocol::hidpp_request(1, 5, 0, &[]);
            let mut response = request[..size].to_vec();
            response[0] = if size == 7 { 0x10 } else { 0x11 };
            response[4..7].copy_from_slice(&[90, 50, 0]);
            let server = if size == 7 {
                *short_server
            } else {
                *long_server
            };
            unsafe {
                let mut written = 0;
                WriteFile(server, Some(&response), Some(&mut written), None)?;
                ensure!(written as usize == response.len(), "incomplete test reply");
            }
            let mut wire = LogitechWire {
                collections: vec![short, long],
                until: Instant::now() + Duration::from_secs(2),
            };
            assert_eq!(
                wire.exchange(&request, |reply| protocol::hidpp_matches(&request, reply))?,
                response
            );
            let mut observed = [0; 20];
            unsafe {
                let mut count = 0;
                ReadFile(*long_server, Some(&mut observed), Some(&mut count), None)?;
                assert_eq!(count, 20);
            }
            assert_eq!(observed, request);
        }
        Ok(())
    }
}
