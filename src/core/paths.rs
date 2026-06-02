#![allow(dead_code)]
// Resource lookup helpers are dormant in the egui shell, but provider/workflow
// loading will need them again once packaged resources are reintroduced.

use std::path::{Path, PathBuf};

const LOCAL_RUNTIME_DIR: &str = ".latentslate";

fn resource_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            roots.push(parent.to_path_buf());
        }
    }
    let manifest_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if manifest_root.exists() {
        roots.push(manifest_root);
    }
    roots
}

pub fn resolve_resource_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    let roots = resource_roots();
    for root in &roots {
        let candidate = root.join(path);
        if candidate.exists() {
            return candidate;
        }
    }
    roots
        .first()
        .map(|root| root.join(path))
        .unwrap_or_else(|| path.to_path_buf())
}

pub fn resource_dir(name: &str) -> Option<PathBuf> {
    let relative = Path::new(name);
    for root in resource_roots() {
        let candidate = root.join(relative);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

pub fn app_runtime_root() -> PathBuf {
    workspace_root().join(LOCAL_RUNTIME_DIR)
}

fn workspace_root() -> PathBuf {
    let mut candidates = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.to_path_buf());
        }
    }
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")));

    for candidate in &candidates {
        for ancestor in candidate.ancestors() {
            if ancestor.join("Cargo.toml").is_file() && ancestor.join("workflows").is_dir() {
                return ancestor.to_path_buf();
            }
        }
    }

    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn app_cache_root() -> PathBuf {
    app_runtime_root().join("cache")
}
