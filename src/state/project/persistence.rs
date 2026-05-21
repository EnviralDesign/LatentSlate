use std::fs;
use std::io;
use std::path::Path;
use uuid::Uuid;

use super::{Project, ProjectSettings};
use crate::state::{Asset, AssetKind, GenerativeConfig};

impl Project {
    // =========================================================================
    // Save/Load
    // =========================================================================

    /// Save the project to its folder
    #[allow(dead_code)]
    pub fn save(&self) -> io::Result<()> {
        let path = self
            .project_path
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Project path not set"))?;
        self.save_to(path)
    }

    /// Save the project to a specific folder
    pub fn save_to(&self, folder: &Path) -> io::Result<()> {
        // Create the project folder structure
        fs::create_dir_all(folder)?;
        fs::create_dir_all(folder.join("audio"))?;
        fs::create_dir_all(folder.join("images"))?;
        fs::create_dir_all(folder.join("video"))?;
        fs::create_dir_all(folder.join("generated"))?;
        fs::create_dir_all(folder.join("generated/video"))?;
        fs::create_dir_all(folder.join("generated/image"))?;
        fs::create_dir_all(folder.join("generated/audio"))?;
        fs::create_dir_all(folder.join("exports"))?;

        // Write project.json
        let json = serde_json::to_string_pretty(self)?;
        fs::write(folder.join("project.json"), json)?;
        self.save_generative_configs()?;

        Ok(())
    }

    /// Load a project from a folder
    pub fn load(folder: &Path) -> io::Result<Self> {
        let project_file = folder.join("project.json");
        let json = fs::read_to_string(&project_file)?;
        let mut project: Project = serde_json::from_str(&json)?;
        project.project_path = Some(folder.to_path_buf());
        project.load_generative_configs();
        project.ensure_generative_video_durations();
        Ok(project)
    }

    /// Create a new project in a folder
    #[allow(dead_code)]
    pub fn create_in(folder: &Path, name: impl Into<String>) -> io::Result<Self> {
        let mut project = Project::new(name);
        project.project_path = Some(folder.to_path_buf());
        project.save_to(folder)?;
        Ok(project)
    }

    /// Create a new project in a folder with explicit settings
    pub fn create_in_with_settings(
        folder: &Path,
        name: impl Into<String>,
        settings: ProjectSettings,
    ) -> io::Result<Self> {
        let mut project = Project::new(name);
        project.settings = settings;
        project.project_path = Some(folder.to_path_buf());
        project.save_to(folder)?;
        Ok(project)
    }

    /// Save the current project to a new folder (initializing it)
    #[allow(dead_code)]
    pub fn save_project_as(&mut self, folder: &Path, name: impl Into<String>) -> io::Result<()> {
        self.name = name.into();
        self.project_path = Some(folder.to_path_buf());
        self.save_to(folder)?;
        Ok(())
    }

    pub fn set_generative_provider_id(&mut self, asset_id: Uuid, provider_id: Option<Uuid>) {
        let _ = self.update_generative_config(asset_id, |config| {
            config.provider_id = provider_id;
        });
    }

    pub fn update_generative_config(
        &mut self,
        asset_id: Uuid,
        update: impl FnOnce(&mut GenerativeConfig),
    ) -> bool {
        let Some(asset) = self.assets.iter().find(|asset| asset.id == asset_id) else {
            return false;
        };
        if !asset.is_generative() {
            return false;
        }

        let active_version = {
            let entry = self
                .generative_configs
                .entry(asset_id)
                .or_insert_with(GenerativeConfig::default);
            update(entry);
            entry.active_version.clone()
        };

        if let Some(asset) = self.assets.iter_mut().find(|asset| asset.id == asset_id) {
            match &mut asset.kind {
                AssetKind::GenerativeVideo {
                    active_version: stored,
                    ..
                }
                | AssetKind::GenerativeImage {
                    active_version: stored,
                    ..
                }
                | AssetKind::GenerativeAudio {
                    active_version: stored,
                    ..
                } => {
                    *stored = active_version;
                }
                _ => {}
            }
        }
        true
    }

    pub fn load_generative_configs(&mut self) {
        let Some(project_root) = self.project_path.clone() else {
            return;
        };

        self.generative_configs.clear();
        for asset in self.assets.iter_mut() {
            let (folder, active_version) = match &mut asset.kind {
                AssetKind::GenerativeVideo {
                    folder,
                    active_version,
                    ..
                }
                | AssetKind::GenerativeImage {
                    folder,
                    active_version,
                }
                | AssetKind::GenerativeAudio {
                    folder,
                    active_version,
                } => (folder, active_version),
                _ => continue,
            };

            let folder_path = project_root.join(folder);
            let config = GenerativeConfig::load(&folder_path).unwrap_or_default();
            *active_version = config.active_version.clone();
            self.generative_configs.insert(asset.id, config);
        }
    }

    pub fn save_generative_configs(&self) -> io::Result<()> {
        let Some(project_root) = self.project_path.as_ref() else {
            return Ok(());
        };

        for asset in self.assets.iter() {
            let Some(folder) = generative_folder_for_asset(asset) else {
                continue;
            };
            let Some(config) = self.generative_configs.get(&asset.id) else {
                continue;
            };
            config.save(&project_root.join(folder))?;
        }
        Ok(())
    }

    pub fn save_generative_config(&self, asset_id: Uuid) -> io::Result<()> {
        let Some(project_root) = self.project_path.as_ref() else {
            return Ok(());
        };
        let Some(config) = self.generative_configs.get(&asset_id) else {
            return Ok(());
        };
        let Some(folder) = self
            .assets
            .iter()
            .find(|asset| asset.id == asset_id)
            .and_then(generative_folder_for_asset)
        else {
            return Ok(());
        };
        config.save(&project_root.join(folder))?;
        Ok(())
    }
}

fn generative_folder_for_asset(asset: &Asset) -> Option<&std::path::PathBuf> {
    match &asset.kind {
        AssetKind::GenerativeVideo { folder, .. }
        | AssetKind::GenerativeImage { folder, .. }
        | AssetKind::GenerativeAudio { folder, .. } => Some(folder),
        _ => None,
    }
}
