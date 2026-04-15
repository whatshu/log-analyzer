use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{copy_dir_all, LogRepo};
use crate::error::{LogAnalyzerError, Result};

pub const DEFAULT_REPO_NAME: &str = "default";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkspaceConfig {
    active: String,
}

/// A workspace manages multiple named repositories under one directory.
///
/// ```text
/// <workspace_root>/
/// ├── workspace.json      # {"active": "default"}
/// ├── default/            # named repo
/// │   ├── meta.json
/// │   ├── index.json
/// │   ├── chunks/
/// │   └── operations.json
/// ├── error_analysis/
/// │   └── ...
/// ```
pub struct Workspace {
    root: PathBuf,
}

impl Workspace {
    /// Open or initialize a workspace at the given root directory.
    pub fn open(root: &Path) -> Result<Self> {
        let ws = Self {
            root: root.to_path_buf(),
        };
        Ok(ws)
    }

    /// Ensure the workspace directory and config exist.
    fn ensure_initialized(&self) -> Result<()> {
        if !self.root.exists() {
            fs::create_dir_all(&self.root)?;
        }
        if !self.config_path().exists() {
            self.save_config(&WorkspaceConfig {
                active: DEFAULT_REPO_NAME.to_string(),
            })?;
        }
        Ok(())
    }

    /// Check if the workspace is initialized (has at least one repo).
    pub fn is_initialized(&self) -> bool {
        self.config_path().exists()
    }

    /// Migrate a flat (pre-workspace) repo layout into the workspace format.
    /// If the root contains meta.json directly, move everything into a "default" subdirectory.
    pub fn migrate_if_needed(&self) -> Result<bool> {
        let root_meta = self.root.join("meta.json");
        if !root_meta.exists() {
            return Ok(false);
        }

        // Old flat layout detected — move into default/
        let default_dir = self.repo_path(DEFAULT_REPO_NAME);
        fs::create_dir_all(&default_dir)?;

        for name in &["meta.json", "index.json", "operations.json", "chunks", "snapshots"] {
            let src = self.root.join(name);
            if src.exists() {
                let dst = default_dir.join(name);
                fs::rename(&src, &dst)?;
            }
        }

        self.save_config(&WorkspaceConfig {
            active: DEFAULT_REPO_NAME.to_string(),
        })?;

        Ok(true)
    }

    /// List all repository names in this workspace.
    pub fn list(&self) -> Result<Vec<String>> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }

        let mut names = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                // A valid repo dir has meta.json inside
                if entry.path().join("meta.json").exists() {
                    names.push(name);
                }
            }
        }
        names.sort();
        Ok(names)
    }

    /// Get the currently active repo name.
    pub fn active(&self) -> Result<String> {
        let config = self.load_config()?;
        Ok(config.active)
    }

    /// Set the active repo name.
    pub fn set_active(&self, name: &str) -> Result<()> {
        // Validate the repo exists
        let path = self.repo_path(name);
        if !path.join("meta.json").exists() {
            return Err(LogAnalyzerError::Repo(format!(
                "Repository '{}' not found",
                name
            )));
        }
        self.save_config(&WorkspaceConfig {
            active: name.to_string(),
        })
    }

    /// Get the filesystem path for a named repo.
    pub fn repo_path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    /// Open a named repo.
    pub fn open_repo(&self, name: &str) -> Result<LogRepo> {
        let path = self.repo_path(name);
        LogRepo::open(&path)
    }

    /// Open the currently active repo.
    pub fn open_active(&self) -> Result<LogRepo> {
        let name = self.active()?;
        self.open_repo(&name)
    }

    /// Import a file into a new named repo.
    pub fn import_file(&self, name: &str, source_file: &Path) -> Result<LogRepo> {
        self.ensure_initialized()?;
        self.validate_new_name(name)?;
        let path = self.repo_path(name);
        let repo = LogRepo::import(&path, source_file)?;
        // Set as active if it's the first repo
        if self.list()?.len() == 1 {
            self.set_active(name)?;
        }
        Ok(repo)
    }

    /// Import raw bytes into a new named repo.
    pub fn import_bytes(
        &self,
        name: &str,
        data: &[u8],
        source_name: String,
    ) -> Result<LogRepo> {
        self.ensure_initialized()?;
        self.validate_new_name(name)?;
        let path = self.repo_path(name);
        let repo = LogRepo::import_from_bytes(&path, data, source_name)?;
        if self.list()?.len() == 1 {
            self.set_active(name)?;
        }
        Ok(repo)
    }

    /// Clone a repo under a new name.
    pub fn clone_repo(&self, src_name: &str, dst_name: &str) -> Result<LogRepo> {
        self.validate_new_name(dst_name)?;
        let src_path = self.repo_path(src_name);
        if !src_path.join("meta.json").exists() {
            return Err(LogAnalyzerError::Repo(format!(
                "Source repository '{}' not found",
                src_name
            )));
        }
        let dst_path = self.repo_path(dst_name);
        copy_dir_all(&src_path, &dst_path)?;
        LogRepo::open(&dst_path)
    }

    /// Remove a named repo.
    pub fn remove_repo(&self, name: &str) -> Result<()> {
        let path = self.repo_path(name);
        if !path.join("meta.json").exists() {
            return Err(LogAnalyzerError::Repo(format!(
                "Repository '{}' not found",
                name
            )));
        }
        fs::remove_dir_all(&path)?;

        // If the removed repo was active, switch to another or default
        if let Ok(active) = self.active() {
            if active == name {
                let repos = self.list()?;
                let new_active = repos.first().map(|s| s.as_str()).unwrap_or(DEFAULT_REPO_NAME);
                let _ = self.save_config(&WorkspaceConfig {
                    active: new_active.to_string(),
                });
            }
        }

        Ok(())
    }

    /// Check if a repo name exists.
    pub fn has_repo(&self, name: &str) -> bool {
        self.repo_path(name).join("meta.json").exists()
    }

    /// Workspace root path.
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn validate_new_name(&self, name: &str) -> Result<()> {
        if name.is_empty() {
            return Err(LogAnalyzerError::Repo(
                "Repository name cannot be empty".to_string(),
            ));
        }
        if name.contains('/') || name.contains('\\') || name == "." || name == ".." {
            return Err(LogAnalyzerError::Repo(format!(
                "Invalid repository name: '{}'",
                name
            )));
        }
        if self.has_repo(name) {
            return Err(LogAnalyzerError::Repo(format!(
                "Repository '{}' already exists",
                name
            )));
        }
        Ok(())
    }

    fn config_path(&self) -> PathBuf {
        self.root.join("workspace.json")
    }

    fn load_config(&self) -> Result<WorkspaceConfig> {
        let path = self.config_path();
        if !path.exists() {
            return Ok(WorkspaceConfig {
                active: DEFAULT_REPO_NAME.to_string(),
            });
        }
        let data = fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&data)?)
    }

    fn save_config(&self, config: &WorkspaceConfig) -> Result<()> {
        fs::create_dir_all(&self.root)?;
        let json = serde_json::to_string_pretty(config)?;
        fs::write(self.config_path(), json)?;
        Ok(())
    }
}
