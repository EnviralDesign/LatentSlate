use std::collections::VecDeque;
use std::sync::{OnceLock, RwLock};

const MAX_PREVIEW_FRAMES: usize = 2;

#[derive(Clone)]
struct PreviewFrame {
    version: u64,
    bytes: Vec<u8>,
}

struct PreviewStore {
    latest_version: u64,
    frames: VecDeque<PreviewFrame>,
}

impl PreviewStore {
    fn new() -> Self {
        Self {
            latest_version: 0,
            frames: VecDeque::new(),
        }
    }

    fn push_frame(&mut self, bytes: Vec<u8>) -> u64 {
        let mut version = self.latest_version.wrapping_add(1);
        if version == 0 {
            version = 1;
        }
        self.latest_version = version;
        self.frames.push_back(PreviewFrame { version, bytes });
        while self.frames.len() > MAX_PREVIEW_FRAMES {
            self.frames.pop_front();
        }
        version
    }

    fn get_frame(&self, version: u64) -> Option<Vec<u8>> {
        if let Some(frame) = self.frames.iter().find(|frame| frame.version == version) {
            return Some(frame.bytes.clone());
        }
        self.frames.back().map(|frame| frame.bytes.clone())
    }

    #[allow(dead_code)]
    // Retained for automation/debug consumers that need the current preview
    // without tracking a version id.
    fn get_latest(&self) -> Option<Vec<u8>> {
        self.frames.back().map(|frame| frame.bytes.clone())
    }
}

fn preview_store() -> &'static RwLock<PreviewStore> {
    static STORE: OnceLock<RwLock<PreviewStore>> = OnceLock::new();
    STORE.get_or_init(|| RwLock::new(PreviewStore::new()))
}

/// Store RGBA preview bytes and return the new version identifier.
pub fn store_preview_frame(width: u32, height: u32, bytes: Vec<u8>) -> Option<u64> {
    if width == 0 || height == 0 || bytes.is_empty() {
        return None;
    }
    let expected_len = width as usize * height as usize * 4;
    if bytes.len() != expected_len {
        return None;
    }
    let store = preview_store();
    let mut store = store.write().ok()?;
    Some(store.push_frame(bytes))
}

/// Fetch preview bytes for a version, falling back to the latest frame if needed.
pub fn get_preview_bytes(version: u64) -> Option<Vec<u8>> {
    let store = preview_store();
    let store = store.read().ok()?;
    store.get_frame(version)
}

/// Fetch the most recent preview bytes, if any.
#[allow(dead_code)]
// Retained for automation/debug consumers that need the current preview without
// tracking a version id.
pub fn get_latest_preview_bytes() -> Option<Vec<u8>> {
    let store = preview_store();
    let store = store.read().ok()?;
    store.get_latest()
}
