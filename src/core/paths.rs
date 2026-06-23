#![allow(dead_code)]
// Resource lookup helpers support both packaged resources and app-managed data.

use std::path::{Path, PathBuf};

pub const APP_HOME_ENV: &str = "LATENTSLATE_HOME";
const LOCAL_RUNTIME_DIR: &str = "LatentSlateData";

fn resource_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    roots.push(app_runtime_root());
    roots.extend(read_only_resource_roots());
    roots
}

fn read_only_resource_roots() -> Vec<PathBuf> {
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
        if path.exists() {
            return path.to_path_buf();
        }
        if let Some(rebased) = rebase_missing_workflow_path(path) {
            return rebased;
        }
        return path.to_path_buf();
    }
    let roots = resource_roots();
    for root in &roots {
        let candidate = root.join(path);
        if candidate.exists() {
            return candidate;
        }
    }
    if is_app_data_relative_path(path) {
        return app_runtime_root().join(path);
    }
    read_only_resource_roots()
        .first()
        .map(|root| root.join(path))
        .unwrap_or_else(|| path.to_path_buf())
}

pub fn storage_resource_path(path: &Path) -> String {
    let resolved = resolve_resource_path(path);
    for root in resource_roots() {
        if let Ok(relative) = resolved.strip_prefix(&root) {
            return normalize_path_for_storage(relative);
        }
    }
    if path.is_relative() {
        normalize_path_for_storage(path)
    } else {
        path.display().to_string()
    }
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
    if let Some(home) = configured_app_home() {
        return home;
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            return parent.join(LOCAL_RUNTIME_DIR);
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        return cwd.join(LOCAL_RUNTIME_DIR);
    }

    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(LOCAL_RUNTIME_DIR)
}

fn rebase_missing_workflow_path(path: &Path) -> Option<PathBuf> {
    let workflow_suffix = workflow_path_suffix(path)?;
    for root in resource_roots() {
        let candidate = root.join(&workflow_suffix);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn workflow_path_suffix(path: &Path) -> Option<PathBuf> {
    let mut suffix = PathBuf::new();
    let mut in_workflows = false;
    for component in path.components() {
        let text = component.as_os_str().to_string_lossy();
        if in_workflows {
            suffix.push(component.as_os_str());
        } else if text.eq_ignore_ascii_case("workflows") {
            suffix.push("workflows");
            in_workflows = true;
        }
    }
    in_workflows.then_some(suffix)
}

fn normalize_path_for_storage(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn is_app_data_relative_path(path: &Path) -> bool {
    path.components().next().is_some_and(|component| {
        let text = component.as_os_str().to_string_lossy();
        matches!(
            text.as_ref(),
            "projects" | "providers" | "workflows" | "cache" | "tmp" | "logs"
        )
    })
}

pub fn app_cache_root() -> PathBuf {
    app_runtime_root().join("cache")
}

pub fn app_projects_root() -> PathBuf {
    app_runtime_root().join("projects")
}

pub fn app_tmp_root() -> PathBuf {
    app_runtime_root().join("tmp")
}

pub fn ensure_app_runtime_dirs() -> Result<PathBuf, String> {
    let root = app_runtime_root();
    let dirs = [
        root.clone(),
        root.join("projects"),
        root.join("providers"),
        root.join("workflows"),
        root.join("cache"),
        root.join("tmp"),
        root.join("logs"),
    ];
    for dir in dirs {
        std::fs::create_dir_all(&dir)
            .map_err(|err| format!("Failed to create app data folder {}: {err}", dir.display()))?;
    }
    Ok(root)
}

fn configured_app_home() -> Option<PathBuf> {
    let value = std::env::var(APP_HOME_ENV).ok()?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let path = PathBuf::from(trimmed);
    if path.is_absolute() {
        Some(path)
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(&path))
            .ok()
            .or(Some(path))
    }
}
