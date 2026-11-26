//! Shell icons and names are loaded away from the compositor's UI thread.
//! Only plain pixel buffers cross threads; composition brushes stay on the UI.

use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicIsize, Ordering},
        mpsc::{Receiver, SyncSender, sync_channel},
    },
    thread,
};

use anyhow::Result;
use nosih_shell::{
    icon_cache::{AppVisual, IconCache},
    power,
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize},
    UI::WindowsAndMessaging::PostMessageW,
};

const WORKERS: usize = 2;
const QUEUE_CAPACITY: usize = 64;
const ICON_SIZE: u32 = 256;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Request {
    id: String,
    variant: &'static str,
}

struct Loaded {
    request: Request,
    visual: Option<AppVisual>,
}

pub struct Visuals {
    requests: SyncSender<Request>,
    results: Receiver<Loaded>,
    ready: HashMap<Request, Option<AppVisual>>,
    pending: HashSet<Request>,
    window: Arc<AtomicIsize>,
}

impl Visuals {
    pub fn new(window: HWND, message: u32) -> Result<Self> {
        Self::start(window, message, |id, variant| {
            IconCache::default()
                .get_variant(id, variant, ICON_SIZE)
                .inspect_err(|error| eprintln!("nosih: {error}"))
                .ok()
        })
    }

    fn start(
        window: HWND,
        message: u32,
        load: impl Fn(&str, &str) -> Option<AppVisual> + Send + Sync + 'static,
    ) -> Result<Self> {
        let (requests, incoming) = sync_channel::<Request>(QUEUE_CAPACITY);
        let (outgoing, results) = sync_channel(QUEUE_CAPACITY);
        let incoming = Arc::new(Mutex::new(incoming));
        let load = Arc::new(load);
        let window = Arc::new(AtomicIsize::new(window.0 as isize));
        for index in 0..WORKERS {
            let incoming = Arc::clone(&incoming);
            let outgoing = outgoing.clone();
            let load = Arc::clone(&load);
            let window = Arc::clone(&window);
            thread::Builder::new()
                .name(format!("nosih-icons-{index}"))
                .spawn(move || {
                    power::set_thread_efficiency(true);
                    // SAFETY: Each worker owns its COM apartment and balances
                    // initialization after all shell interfaces are released.
                    let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
                    loop {
                        let request = {
                            let Ok(receiver) = incoming.lock() else { break };
                            receiver.recv()
                        };
                        let Ok(request) = request else { break };
                        // Never hold the queue mutex while calling shell extensions.
                        let visual = com
                            .is_ok()
                            .then(|| load(&request.id, request.variant))
                            .flatten();
                        if outgoing.send(Loaded { request, visual }).is_err() {
                            break;
                        }
                        let hwnd = window.load(Ordering::Relaxed);
                        if hwnd != 0 {
                            // SAFETY: Notification carries no pointer. Drop clears
                            // the target before the application's windows disappear.
                            unsafe {
                                let _ = PostMessageW(
                                    Some(HWND(hwnd as *mut _)),
                                    message,
                                    WPARAM(0),
                                    LPARAM(0),
                                );
                            }
                        }
                    }
                    if com.is_ok() {
                        // SAFETY: Balances successful initialization on this thread.
                        unsafe { CoUninitialize() };
                    }
                })?;
        }
        Ok(Self {
            requests,
            results,
            ready: HashMap::new(),
            pending: HashSet::new(),
            window,
        })
    }

    /// Never waits for disk, COM or a worker, even when the queue is full.
    pub fn get(&mut self, id: &str, variant: &'static str) -> Option<&AppVisual> {
        let request = Request {
            id: id.to_owned(),
            variant,
        };
        if !self.ready.contains_key(&request)
            && !self.pending.contains(&request)
            && self.requests.try_send(request.clone()).is_ok()
        {
            self.pending.insert(request.clone());
        }
        self.ready.get(&request).and_then(Option::as_ref)
    }

    /// Like `get`, but only reads what is loaded already: several loaded
    /// visuals can then be borrowed at once (a group's tile).
    pub fn peek(&self, id: &str, variant: &'static str) -> Option<&AppVisual> {
        self.ready
            .get(&Request {
                id: id.to_owned(),
                variant,
            })
            .and_then(Option::as_ref)
    }

    pub fn collect(&mut self) -> Vec<String> {
        let mut changed = Vec::new();
        for loaded in self.results.try_iter() {
            self.pending.remove(&loaded.request);
            changed.push(loaded.request.id.clone());
            self.ready.insert(loaded.request, loaded.visual);
        }
        changed
    }

    pub fn retain(&mut self, mut needed: impl FnMut(&str) -> bool) {
        self.ready.retain(|request, _| needed(&request.id));
    }
}

impl Drop for Visuals {
    fn drop(&mut self) {
        self.window.store(0, Ordering::Relaxed);
        // Do not join shell workers: an unresponsive extension must not hold
        // the UI during shutdown. Disconnecting the channels ends idle workers.
    }
}

/// A temporary label that does no file access or shell lookup.
pub fn fallback_name(id: &str) -> String {
    let file = id.rsplit(['\\', '/']).next().unwrap_or(id);
    file.strip_suffix(".exe").unwrap_or(file).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{Condvar, mpsc::channel},
        time::{Duration, Instant},
    };

    #[test]
    fn blocked_shell_workers_do_not_block_requests_or_duplicate_work() -> Result<()> {
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let release = Arc::clone(&gate);
        let (started, waiting) = channel();
        let mut visuals = Visuals::start(HWND::default(), 0, move |id, _| {
            let _ = started.send(id.to_owned());
            let (lock, wake) = &*gate;
            let open = lock
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            drop(
                wake.wait_while(open, |open| !*open)
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            );
            None
        })?;
        visuals.get("first", "");
        visuals.get("second", "");
        // Both workers are inside the simulated shell extension. Requests
        // below must complete even though neither worker can make progress.
        for _ in 0..WORKERS {
            waiting.recv_timeout(Duration::from_secs(2))?;
        }
        for _ in 0..1_000 {
            assert!(visuals.get("first", "").is_none());
        }
        assert_eq!(visuals.pending.len(), 2);
        for index in 0..QUEUE_CAPACITY * 2 {
            visuals.get(&format!("queued-{index}"), "");
        }
        assert_eq!(visuals.pending.len(), QUEUE_CAPACITY + WORKERS);
        assert!(visuals.collect().is_empty());
        // Shutdown must also finish while the shell is still blocked.
        drop(visuals);
        *release
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        release.1.notify_all();
        Ok(())
    }

    #[test]
    fn completed_variants_are_cached_without_reloading_or_mixing_them() -> Result<()> {
        let mut visuals = Visuals::start(HWND::default(), 0, |_, variant| {
            Some(AppVisual {
                name: variant.to_owned(),
                icon: nosih_shell::icons::IconImage {
                    width: 1,
                    height: 1,
                    bgra: vec![0, 0, 0, 255],
                },
            })
        })?;
        visuals.get("bin", "empty");
        visuals.get("bin", "full");
        let deadline = Instant::now() + Duration::from_secs(2);
        while visuals.ready.len() < 2 && Instant::now() < deadline {
            visuals.collect();
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            visuals
                .get("bin", "empty")
                .map(|visual| visual.name.as_str()),
            Some("empty")
        );
        assert_eq!(
            visuals
                .get("bin", "full")
                .map(|visual| visual.name.as_str()),
            Some("full")
        );
        assert!(visuals.pending.is_empty());
        visuals.retain(|_| false);
        assert!(visuals.ready.is_empty());
        Ok(())
    }

    #[test]
    fn fallback_name_needs_no_shell_or_file_access() {
        assert_eq!(fallback_name(r"Z:\offline\example.exe"), "example");
        assert_eq!(
            fallback_name("shell:AppsFolder\\Package!App"),
            "Package!App"
        );
    }

    #[test]
    #[ignore = "Measures real Windows shell work; run explicitly with --nocapture"]
    fn measure_shell_load_and_nonblocking_requests() -> Result<()> {
        let id = format!(r"{}\explorer.exe", std::env::var("WINDIR")?);
        let shell_start = Instant::now();
        let mut visuals = Visuals::new(HWND::default(), 0)?;
        let enqueue = Instant::now();
        for _ in 0..1_000 {
            visuals.get(&id, "");
        }
        let enqueue_elapsed = enqueue.elapsed();
        let loaded = visuals.results.recv_timeout(Duration::from_secs(10))?;
        assert!(loaded.visual.is_some());
        eprintln!(
            "1000 UI requests: {enqueue_elapsed:?}; worker result: {:?}",
            shell_start.elapsed()
        );
        Ok(())
    }
}
