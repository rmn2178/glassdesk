//! A small log next to the dock's data. The installed dock has no console,
//! so what it writes to stderr is lost; this keeps the rare events that
//! explain a problem afterwards (errors, the dock hiding for a full-screen
//! app, starting). Nothing is written while the dock is idle.

use std::{
    env,
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    sync::{
        OnceLock,
        mpsc::{self, SyncSender},
    },
    thread,
    time::Duration,
};

use windows::Win32::{Foundation::SYSTEMTIME, System::SystemInformation::GetLocalTime};

/// Past this size the log becomes `nosih.old.log` and starts again, so
/// it never holds more than twice this.
const LIMIT: u64 = 256 * 1024;
const LINE_LIMIT: usize = 4096;
static WRITER: OnceLock<Option<SyncSender<Message>>> = OnceLock::new();

enum Message {
    Line(SYSTEMTIME, String),
    Flush(mpsc::Sender<()>),
}

/// Queues diagnostic output. A slow disk or a full queue never holds the UI.
pub fn line(text: &str) {
    let writer = WRITER.get_or_init(|| {
        let (sender, receiver) = mpsc::sync_channel(64);
        thread::Builder::new()
            .name("nosih-log".into())
            .spawn(move || {
                let path = env::var_os("LOCALAPPDATA")
                    .map(|folder| Path::new(&folder).join("Nosih").join("nosih.log"));
                for message in receiver {
                    match message {
                        Message::Line(now, text) => {
                            eprintln!("nosih: {text}");
                            if let Some(path) = &path {
                                let _ = write_record(path, now, &text);
                            }
                        }
                        Message::Flush(done) => {
                            let _ = done.send(());
                        }
                    }
                }
            })
            .ok()
            .map(|_| sender)
    });
    if let Some(writer) = writer {
        let end = text.floor_char_boundary(text.len().min(LINE_LIMIT));
        // SAFETY: Read-only clock query; timestamps reflect the event time.
        let now = unsafe { GetLocalTime() };
        let _ = writer.try_send(Message::Line(now, text[..end].to_owned()));
    }
}

/// Best-effort drain at process exit, with a fixed upper bound.
pub fn flush() {
    if let Some(Some(writer)) = WRITER.get() {
        let (done, wait) = mpsc::channel();
        if writer.try_send(Message::Flush(done)).is_ok() {
            let _ = wait.recv_timeout(Duration::from_millis(150));
        }
    }
}

fn write_record(path: &Path, now: SYSTEMTIME, text: &str) -> std::io::Result<()> {
    let record = format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03} {text}",
        now.wYear, now.wMonth, now.wDay, now.wHour, now.wMinute, now.wSecond, now.wMilliseconds
    );
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder)?;
    }
    if fs::metadata(path).is_ok_and(|meta| meta.len() + record.len() as u64 + 1 > LIMIT) {
        let old = path.with_extension("old.log");
        match fs::remove_file(&old) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
        fs::rename(path, old)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{record}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_replaces_the_previous_backup_and_bounds_both_files() -> std::io::Result<()> {
        let folder = env::temp_dir().join(format!("nosih-log-test-{}", std::process::id()));
        let path = folder.join("nosih.log");
        let message = "a".repeat(LINE_LIMIT);
        for _ in 0..200 {
            write_record(&path, SYSTEMTIME::default(), &message)?;
        }
        assert!(fs::metadata(&path)?.len() <= LIMIT);
        let old = path.with_extension("old.log");
        assert!(fs::metadata(&old)?.len() <= LIMIT);
        fs::remove_file(path)?;
        fs::remove_file(old)?;
        fs::remove_dir(folder)
    }
}
