//! Installed plugin and package tracking.

use std::path::PathBuf;

use lib_plugin_manifest::{Manifest, PluginManifest};

/// An installed package (may contain 1+ plugins).
#[derive(Debug, Clone)]
pub struct InstalledPackage {
    /// The manifest (Single or Package)
    pub manifest: Manifest,
    /// Installation path
    pub path: PathBuf,
    /// IDs of plugins in this package
    pub plugin_ids: Vec<String>,
}

impl InstalledPackage {
    /// Get the package ID.
    pub fn id(&self) -> &str {
        self.manifest.id()
    }

    /// Get the package version.
    pub fn version(&self) -> &str {
        self.manifest.version()
    }

    /// Check if this is a multi-plugin package.
    pub fn is_multi_plugin(&self) -> bool {
        self.manifest.is_package()
    }
}

/// An installed plugin (belongs to a package).
#[derive(Debug, Clone)]
pub struct InstalledPlugin {
    /// Plugin manifest
    pub manifest: PluginManifest,
    /// Path to the plugin binary
    pub path: PathBuf,
    /// Parent package ID
    pub package_id: String,
    /// Whether the plugin is enabled
    pub enabled: bool,
}

impl InstalledPlugin {
    /// Get the plugin ID.
    pub fn id(&self) -> &str {
        &self.manifest.plugin.id
    }

    /// Get the plugin version.
    pub fn version(&self) -> &str {
        &self.manifest.plugin.version
    }

    /// Get the plugin type.
    pub fn plugin_type(&self) -> &str {
        &self.manifest.plugin.plugin_type
    }

    /// Get the plugin name.
    pub fn name(&self) -> &str {
        &self.manifest.plugin.name
    }
}

/// Install status for ongoing operations.
#[derive(Debug, Clone)]
pub enum InstallStatus {
    /// Not installed
    NotInstalled,
    /// Currently installing
    Installing {
        /// Progress 0.0 to 1.0
        progress: f32,
    },
    /// Installed successfully
    Installed {
        /// Version installed
        version: String,
    },
    /// Update available
    UpdateAvailable {
        /// Current version
        current: String,
        /// Latest version
        latest: String,
    },
    /// Installation failed
    Failed {
        /// Error message
        error: String,
    },
}

impl InstallStatus {
    /// Check if installed.
    pub fn is_installed(&self) -> bool {
        matches!(
            self,
            InstallStatus::Installed { .. } | InstallStatus::UpdateAvailable { .. }
        )
    }

    /// Check if an update is available.
    pub fn has_update(&self) -> bool {
        matches!(self, InstallStatus::UpdateAvailable { .. })
    }
}
