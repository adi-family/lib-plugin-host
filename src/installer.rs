//! Core plugin installer — download, extract, update, uninstall, dependency resolution.
//!
//! Contains no UI logic. Callers handle progress bars, i18n messages, and prompts.

use std::collections::HashSet;
use std::path::PathBuf;

use lib_plugin_manifest::PluginManifest;
use lib_plugin_registry::{PluginEntry, PluginInfo, RegistryClient, SearchKind, SearchResults};

use crate::HostError;

/// Result of a successful plugin installation.
#[derive(Debug, Clone)]
pub struct InstallResult {
    pub id: String,
    pub version: String,
    pub path: PathBuf,
}

/// Result of an update check.
#[derive(Debug, Clone)]
pub enum UpdateCheck {
    /// Already at the latest version.
    AlreadyLatest { version: String },
    /// An update is available.
    Available { current: String, latest: String },
}

/// Core plugin installer — download, extract, update, uninstall, dependency resolution.
///
/// Contains no UI logic. Callers handle progress bars, i18n messages, and prompts.
pub struct PluginInstaller {
    client: RegistryClient,
    install_dir: PathBuf,
}

impl PluginInstaller {
    /// Create an installer from a `PluginConfig`.
    pub fn from_config(config: &crate::PluginConfig) -> Self {
        let url = config
            .registry_url
            .as_deref()
            .unwrap_or("https://registry.example.com");
        let client = RegistryClient::new(url).with_cache(config.cache_dir.clone());
        Self {
            client,
            install_dir: config.plugins_dir.clone(),
        }
    }

    /// Create with explicit registry URL and directories.
    pub fn new(registry_url: &str, install_dir: PathBuf, cache_dir: PathBuf) -> Self {
        let client = RegistryClient::new(registry_url).with_cache(cache_dir);
        Self {
            client,
            install_dir,
        }
    }

    /// The directory where plugins are installed.
    pub fn install_dir(&self) -> &PathBuf {
        &self.install_dir
    }

    /// Path to a specific plugin's root directory.
    pub fn plugin_path(&self, id: &str) -> PathBuf {
        self.install_dir.join(id)
    }

    // -- Registry operations --

    /// Search the plugin registry.
    pub async fn search(&self, query: &str) -> Result<SearchResults, HostError> {
        Ok(self.client.search(query, SearchKind::All).await?)
    }

    /// List all available plugins in the registry.
    pub async fn list_available(&self) -> Result<Vec<PluginEntry>, HostError> {
        Ok(self.client.list_plugins().await?)
    }

    /// Check if a plugin exists in the registry (without downloading).
    ///
    /// Returns `Ok(Some(info))` if found, `Ok(None)` if not found.
    pub async fn get_plugin_info(&self, id: &str) -> Result<Option<PluginInfo>, HostError> {
        match self.client.get_plugin_latest(id).await {
            Ok(info) => Ok(Some(info)),
            Err(lib_plugin_registry::RegistryError::NotFound(_)) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    // -- Installation status --

    /// Check if a plugin is installed. Returns the version string if installed.
    pub fn is_installed(&self, id: &str) -> Option<String> {
        let version_file = self.install_dir.join(id).join(".version");
        std::fs::read_to_string(version_file)
            .ok()
            .map(|v| v.trim().to_string())
    }

    /// List all installed plugins as `(id, version)` pairs.
    pub async fn list_installed(&self) -> Result<Vec<(String, String)>, HostError> {
        let mut installed = Vec::new();
        if !self.install_dir.exists() {
            return Ok(installed);
        }

        let mut entries = tokio::fs::read_dir(&self.install_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_dir() {
                // Skip the command index directory
                if entry.file_name() == crate::command_index::COMMANDS_DIR_NAME {
                    continue;
                }
                let version_file = path.join(".version");
                if version_file.exists() {
                    let version = tokio::fs::read_to_string(&version_file).await?;
                    let name = path.file_name().unwrap().to_string_lossy().to_string();
                    installed.push((name, version.trim().to_string()));
                }
            }
        }

        Ok(installed)
    }

    // -- Install --

    /// Install a single plugin from the registry.
    ///
    /// Downloads the appropriate platform build, extracts to the install directory,
    /// writes a `.version` file, and sets executable permissions on Unix.
    ///
    /// `on_progress` is called with `(bytes_done, bytes_total)` during download.
    pub async fn install(
        &self,
        id: &str,
        version: Option<&str>,
        on_progress: impl Fn(u64, u64),
    ) -> Result<InstallResult, HostError> {
        let platform = lib_plugin_manifest::current_platform();

        let info = if let Some(v) = version {
            self.client.get_plugin_version(id, v).await?
        } else {
            self.client.get_plugin_latest(id).await?
        };

        // Verify platform support
        info.platforms
            .iter()
            .find(|p| p.platform == platform)
            .ok_or_else(|| {
                HostError::PlatformNotSupported(format!(
                    "Plugin {} does not support platform {}",
                    id, platform
                ))
            })?;

        // Download
        let bytes = self
            .client
            .download_plugin(id, &info.version, &platform, |done, total| {
                on_progress(done, total);
            })
            .await?;

        // Extract tarball
        let plugin_dir = self.install_dir.join(id).join(&info.version);
        tokio::fs::create_dir_all(&plugin_dir).await?;

        let decoder = flate2::read::GzDecoder::new(&bytes[..]);
        let mut archive = tar::Archive::new(decoder);
        archive.unpack(&plugin_dir)?;

        // Write version file
        let version_file = self.install_dir.join(id).join(".version");
        tokio::fs::write(&version_file, info.version.as_bytes()).await?;

        // Set executable permissions on Unix
        #[cfg(unix)]
        set_unix_permissions(&plugin_dir).await;

        // Update latest symlink (points to current version directory)
        if let Err(e) =
            crate::command_index::update_latest_link(&self.install_dir, id, &info.version)
        {
            tracing::warn!(plugin_id = %id, error = %e, "Failed to update latest symlink");
        }

        // Update command index: remove old symlinks first (handles renamed/removed commands),
        // then create new ones from the current manifest.
        let _ = crate::command_index::remove_command_symlinks(&self.install_dir, id);
        if let Err(e) =
            crate::command_index::create_command_symlinks(&self.install_dir, id, &info.version)
        {
            tracing::warn!(plugin_id = %id, error = %e, "Failed to create command symlinks");
        }

        Ok(InstallResult {
            id: id.to_string(),
            version: info.version,
            path: plugin_dir,
        })
    }

    /// Install a plugin and all its dependencies (silent — no progress reporting).
    ///
    /// Returns the list of plugins that were actually installed (skips already-installed).
    pub async fn install_with_dependencies(
        &self,
        id: &str,
        version: Option<&str>,
    ) -> Result<Vec<InstallResult>, HostError> {
        let mut results = Vec::new();
        let mut visiting = HashSet::new();
        self.install_recursive(id, version, &mut visiting, &mut results)
            .await?;
        Ok(results)
    }

    async fn install_recursive(
        &self,
        id: &str,
        version: Option<&str>,
        visiting: &mut HashSet<String>,
        results: &mut Vec<InstallResult>,
    ) -> Result<(), HostError> {
        if visiting.contains(id) {
            return Ok(());
        }
        visiting.insert(id.to_string());

        if self.is_installed(id).is_some() {
            return Ok(());
        }

        let result = self.install(id, version, |_, _| {}).await?;
        results.push(result);

        let deps = self.get_dependencies(id);
        for dep in deps {
            Box::pin(self.install_recursive(&dep, None, visiting, results)).await?;
        }

        Ok(())
    }

    // -- Update --

    /// Check if an update is available for an installed plugin.
    pub async fn check_update(&self, id: &str) -> Result<UpdateCheck, HostError> {
        let current = self
            .is_installed(id)
            .ok_or_else(|| HostError::NotInstalled(id.to_string()))?;

        let latest = self.client.get_plugin_latest(id).await?;

        if current == latest.version {
            Ok(UpdateCheck::AlreadyLatest { version: current })
        } else {
            Ok(UpdateCheck::Available {
                current,
                latest: latest.version,
            })
        }
    }

    /// Update an installed plugin to the latest version.
    ///
    /// Returns `Ok(None)` if already at the latest version, `Ok(Some(result))` if updated.
    pub async fn update(
        &self,
        id: &str,
        on_progress: impl Fn(u64, u64),
    ) -> Result<Option<InstallResult>, HostError> {
        let current = self
            .is_installed(id)
            .ok_or_else(|| HostError::NotInstalled(id.to_string()))?;

        let latest = self.client.get_plugin_latest(id).await?;

        if current == latest.version {
            return Ok(None);
        }

        // Remove old version directory
        // Note: command symlinks don't need removal — they point through latest/
        // which install() will re-point to the new version.
        let old_dir = self.install_dir.join(id).join(&current);
        if old_dir.exists() {
            tokio::fs::remove_dir_all(&old_dir).await?;
        }

        let result = self.install(id, Some(&latest.version), on_progress).await?;
        Ok(Some(result))
    }

    // -- Uninstall --

    /// Uninstall a plugin by removing its directory.
    pub async fn uninstall(&self, id: &str) -> Result<(), HostError> {
        let plugin_dir = self.install_dir.join(id);
        if !plugin_dir.exists() {
            return Err(HostError::NotInstalled(id.to_string()));
        }

        // Remove command index symlinks before removing plugin directory
        if let Err(e) = crate::command_index::remove_command_symlinks(&self.install_dir, id) {
            tracing::warn!(plugin_id = %id, error = %e, "Failed to remove command symlinks");
        }

        tokio::fs::remove_dir_all(&plugin_dir).await?;
        Ok(())
    }

    // -- Dependencies --

    /// Read dependencies from an installed plugin's manifest.
    ///
    /// Uses `PluginManifest` deserialization (not manual TOML parsing).
    pub fn get_dependencies(&self, id: &str) -> Vec<String> {
        let plugin_dir = self.install_dir.join(id);
        let version_file = plugin_dir.join(".version");

        let version = match std::fs::read_to_string(version_file) {
            Ok(v) => v.trim().to_string(),
            Err(_) => return Vec::new(),
        };

        let manifest_path = plugin_dir.join(&version).join("plugin.toml");
        match PluginManifest::from_file(&manifest_path) {
            Ok(manifest) => manifest.compatibility.depends_on,
            Err(_) => Vec::new(),
        }
    }

    // -- Pattern matching --

    /// Find all available plugins matching a glob pattern (e.g., "adi.lang.*").
    pub async fn find_matching(&self, pattern: &str) -> Result<Vec<PluginEntry>, HostError> {
        let all = self.list_available().await?;
        Ok(all
            .into_iter()
            .filter(|p| matches_glob(&p.id, pattern))
            .collect())
    }
}

/// Set executable permissions on non-text files in a directory (Unix only).
#[cfg(unix)]
async fn set_unix_permissions(dir: &PathBuf) {
    use std::os::unix::fs::PermissionsExt;

    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(e) => e,
        Err(_) => return,
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.is_file() {
            if let Ok(metadata) = tokio::fs::metadata(&path).await {
                let mut perms = metadata.permissions();
                if !path
                    .extension()
                    .is_some_and(|e| e == "json" || e == "toml" || e == "txt" || e == "md")
                {
                    perms.set_mode(0o755);
                    let _ = tokio::fs::set_permissions(&path, perms).await;
                }
            }
        }
    }
}

/// Check if a string contains glob wildcards.
pub fn is_glob_pattern(s: &str) -> bool {
    s.contains('*')
}

/// Match a string against a simple glob pattern (supports `*` wildcard).
pub fn matches_glob(s: &str, pattern: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();

    if parts.len() == 1 {
        return s == pattern;
    }

    let mut pos = 0;
    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            if !s.starts_with(part) {
                return false;
            }
            pos = part.len();
        } else if i == parts.len() - 1 {
            if !s.ends_with(part) {
                return false;
            }
        } else if let Some(found_pos) = s[pos..].find(part) {
            pos += found_pos + part.len();
        } else {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_glob_pattern() {
        assert!(is_glob_pattern("adi.lang.*"));
        assert!(is_glob_pattern("*"));
        assert!(!is_glob_pattern("adi.tasks"));
    }

    #[test]
    fn test_matches_glob_exact() {
        assert!(matches_glob("adi.tasks", "adi.tasks"));
        assert!(!matches_glob("adi.tasks", "adi.lint"));
    }

    #[test]
    fn test_matches_glob_wildcard() {
        assert!(matches_glob("adi.lang.rust", "adi.lang.*"));
        assert!(matches_glob("adi.lang.python", "adi.lang.*"));
        assert!(!matches_glob("adi.tasks", "adi.lang.*"));
    }

    #[test]
    fn test_matches_glob_middle_wildcard() {
        assert!(matches_glob("adi.lang.rust.plugin", "adi.*.plugin"));
        assert!(!matches_glob("adi.lang.rust.core", "adi.*.plugin"));
    }
}
