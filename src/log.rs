use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static LOGGER: Mutex<Option<Logger>> = Mutex::new(None);

pub struct Logger {
    file: std::fs::File,
}

impl Logger {
    fn open(path: &Path) -> Option<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()?;
        Some(Logger { file })
    }

    fn write(&mut self, msg: &str) {
        let stamp = crate::util::iso_now_plus(0);
        let _ = writeln!(self.file, "[{stamp}] {msg}");
    }
}

pub fn init(config_dir: &Path) {
    let path: PathBuf = config_dir.join("log.txt");
    let mut guard = LOGGER.lock().unwrap();
    *guard = Logger::open(&path);
}

pub fn write(msg: &str) {
    if let Ok(mut guard) = LOGGER.lock() {
        if let Some(logger) = guard.as_mut() {
            logger.write(msg);
        }
    }
}
