use crate::state::AgentProviderEntry;
use std::{
    fs, io,
    path::{Path, PathBuf},
};
use uuid::Uuid;

pub fn root() -> PathBuf {
    super::provider_store::local_providers_root().join("agents")
}

pub fn load() -> Vec<AgentProviderEntry> {
    load_from(&root())
}

fn load_from(root: &Path) -> Vec<AgentProviderEntry> {
    let mut providers = Vec::new();
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                if let Ok(bytes) = fs::read(&path) {
                    if let Ok(provider) = serde_json::from_slice::<AgentProviderEntry>(&bytes) {
                        if path.file_stem().and_then(|s| s.to_str())
                            == Some(&provider.id.to_string())
                        {
                            providers.push(provider);
                        }
                    }
                }
            }
        }
    }
    providers.sort_by_key(|p| (p.name.to_lowercase(), p.id));
    providers
}

pub fn save(provider: &AgentProviderEntry) -> io::Result<()> {
    save_to(&root(), provider)
}

fn save_to(root: &Path, provider: &AgentProviderEntry) -> io::Result<()> {
    fs::create_dir_all(root)?;
    fs::write(
        root.join(format!("{}.json", provider.id)),
        serde_json::to_vec_pretty(provider)?,
    )
}

pub fn delete(id: Uuid) -> io::Result<()> {
    fs::remove_file(root().join(format!("{id}.json")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolated_agent_storage_roundtrip() {
        let root = std::env::temp_dir().join(format!("agent-store-{}", Uuid::new_v4()));
        let mut provider = AgentProviderEntry::default();
        save_to(&root.join("agents"), &provider).unwrap();
        assert_eq!(load_from(&root).len(), 0);
        let loaded = load_from(&root.join("agents"));
        assert!(loaded == vec![provider.clone()]);
        provider.enabled = false;
        save_to(&root.join("agents"), &provider).unwrap();
        assert!(!load_from(&root.join("agents"))[0].enabled);
        fs::remove_dir_all(root).unwrap();
    }
}
