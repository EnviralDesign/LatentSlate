use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use image::RgbaImage;
use std::sync::Arc;

use super::utils::image_size_bytes;
use super::{CachedFrame, FrameKey, PreviewCacheStats};

struct CacheEntry {
    image: Arc<RgbaImage>,
    source_width: u32,
    source_height: u32,
    size_bytes: usize,
    last_used: u64,
}

pub struct FrameCache {
    max_bytes: usize,
    total_bytes: usize,
    access_counter: u64,
    entries: HashMap<FrameKey, CacheEntry>,
    lru_order: VecDeque<(FrameKey, u64)>,
    pub(crate) asset_index: HashMap<PathBuf, HashSet<i64>>,
}

impl FrameCache {
    pub fn new(max_bytes: usize) -> Self {
        Self {
            max_bytes,
            total_bytes: 0,
            access_counter: 0,
            entries: HashMap::new(),
            lru_order: VecDeque::new(),
            asset_index: HashMap::new(),
        }
    }

    pub(crate) fn stats(&self) -> PreviewCacheStats {
        PreviewCacheStats {
            max_bytes: self.max_bytes,
            total_bytes: self.total_bytes,
            entry_count: self.entries.len(),
            indexed_asset_count: self.asset_index.len(),
            indexed_frame_count: self.asset_index.values().map(HashSet::len).sum(),
        }
    }

    pub(crate) fn get(&mut self, key: &FrameKey) -> Option<CachedFrame> {
        let (image, source_width, source_height) = {
            let entry = self.entries.get_mut(key)?;
            self.access_counter = self.access_counter.wrapping_add(1);
            entry.last_used = self.access_counter;
            self.lru_order.push_back((key.clone(), entry.last_used));
            (
                Arc::clone(&entry.image),
                entry.source_width,
                entry.source_height,
            )
        };
        self.compact_lru_if_needed();
        Some(CachedFrame {
            image,
            source_width,
            source_height,
        })
    }

    pub(crate) fn insert(
        &mut self,
        key: FrameKey,
        image: Arc<RgbaImage>,
        source_width: u32,
        source_height: u32,
    ) {
        let size_bytes = image_size_bytes(&image);
        if size_bytes == 0 || self.max_bytes == 0 || size_bytes > self.max_bytes {
            return;
        }

        if let Some(existing) = self.entries.remove(&key) {
            self.total_bytes = self.total_bytes.saturating_sub(existing.size_bytes);
        }

        self.access_counter = self.access_counter.wrapping_add(1);
        let last_used = self.access_counter;
        self.asset_index
            .entry(key.path.clone())
            .or_default()
            .insert(key.frame_index);
        self.entries.insert(
            key.clone(),
            CacheEntry {
                image,
                source_width,
                source_height,
                size_bytes,
                last_used,
            },
        );
        self.total_bytes = self.total_bytes.saturating_add(size_bytes);
        self.lru_order.push_back((key, last_used));
        self.evict_if_needed();
        self.compact_lru_if_needed();
    }

    #[allow(dead_code)]
    // Used by the preserved cache invalidation path when thumbnail/generation
    // refreshes are reconnected to the egui shell.
    pub(crate) fn invalidate_path(&mut self, path: &Path) {
        self.asset_index.remove(path);
        let keys: Vec<FrameKey> = self
            .entries
            .keys()
            .filter(|key| key.path == path)
            .cloned()
            .collect();
        for key in keys {
            if let Some(entry) = self.entries.remove(&key) {
                self.total_bytes = self.total_bytes.saturating_sub(entry.size_bytes);
            }
        }
    }

    #[allow(dead_code)]
    // Folder-wide invalidation is dormant until provider output refreshes are
    // wired back through the egui editor controller.
    pub(crate) fn invalidate_folder(&mut self, folder: &Path) {
        let paths: Vec<PathBuf> = self
            .asset_index
            .keys()
            .filter(|path| path.starts_with(folder))
            .cloned()
            .collect();
        for path in paths {
            self.invalidate_path(&path);
        }
    }

    fn evict_if_needed(&mut self) {
        while self.total_bytes > self.max_bytes {
            let Some((key, stamp)) = self.lru_order.pop_front() else {
                break;
            };
            let Some(entry) = self.entries.get(&key) else {
                continue;
            };
            if entry.last_used != stamp {
                continue;
            }
            self.total_bytes = self.total_bytes.saturating_sub(entry.size_bytes);
            self.entries.remove(&key);
            if let Some(frames) = self.asset_index.get_mut(&key.path) {
                frames.remove(&key.frame_index);
                if frames.is_empty() {
                    self.asset_index.remove(&key.path);
                }
            }
        }
    }

    fn compact_lru_if_needed(&mut self) {
        let entry_count = self.entries.len();
        if entry_count == 0 {
            self.lru_order.clear();
            return;
        }
        let max_len = entry_count.saturating_mul(4).saturating_add(1024);
        if self.lru_order.len() <= max_len {
            return;
        }

        let mut ordered: Vec<(FrameKey, u64)> = self
            .entries
            .iter()
            .map(|(key, entry)| (key.clone(), entry.last_used))
            .collect();
        ordered.sort_by_key(|(_, stamp)| *stamp);
        self.lru_order.clear();
        for item in ordered {
            self.lru_order.push_back(item);
        }
    }
}
