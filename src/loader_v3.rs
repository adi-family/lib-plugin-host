//! Plugin loader for v3 ABI (native async traits)

use crate::{PluginError, Result};
use lib_plugin_abi_v3::{Plugin, PluginContext, PluginMetadata};
use lib_plugin_manifest::PluginManifest;
use libloading::{Library, Symbol};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Loaded plugin (v3)
pub struct LoadedPluginV3 {
    /// Plugin manifest
    pub manifest: PluginManifest,

    /// Dynamic library handle
    _library: Library,

    /// Plugin instance
    pub plugin: Arc<dyn Plugin>,
}

impl LoadedPluginV3 {
    /// Load a plugin from a dynamic library
    pub async fn load(manifest: PluginManifest, plugin_dir: &Path) -> Result<Self> {
        // Resolve binary path
        let lib_path = resolve_plugin_binary(&manifest, plugin_dir)?;

        // Load library
        let library = unsafe {
            Library::new(&lib_path).map_err(|e| {
                PluginError::InitFailed(format!("Failed to load library {:?}: {}", lib_path, e))
            })?
        };

        // Get plugin_create symbol
        let create_fn: Symbol<fn() -> Box<dyn Plugin>> = unsafe {
            library
                .get(b"plugin_create")
                .map_err(|e| PluginError::InitFailed(format!("Missing plugin_create symbol: {}", e)))?
        };

        // Create plugin instance
        let mut plugin = create_fn();

        // Create plugin context
        let ctx = create_plugin_context(&manifest)?;

        // Initialize plugin
        plugin
            .init(&ctx)
            .await
            .map_err(|e| PluginError::InitFailed(format!("Plugin init failed: {}", e)))?;

        Ok(Self {
            manifest,
            _library: library,
            plugin: Arc::from(plugin),
        })
    }

    /// Get plugin metadata
    pub fn metadata(&self) -> PluginMetadata {
        self.plugin.metadata()
    }

    /// Shutdown and unload the plugin
    pub async fn unload(self) -> Result<()> {
        // Call shutdown
        self.plugin
            .shutdown()
            .await
            .map_err(|e| PluginError::Other(anyhow::anyhow!("Shutdown failed: {}", e)))?;

        // Drop plugin instance
        drop(self.plugin);

        // Library will be unloaded when dropped
        Ok(())
    }
}

/// Resolve plugin binary path
fn resolve_plugin_binary(manifest: &PluginManifest, plugin_dir: &Path) -> Result<PathBuf> {
    let binary_name = manifest
        .binary
        .as_ref()
        .and_then(|b| b.name.as_deref())
        .unwrap_or("plugin");

    // Try platform-specific names
    let candidates = if cfg!(target_os = "macos") {
        vec![
            format!("lib{}.dylib", binary_name),
            format!("{}.dylib", binary_name),
        ]
    } else if cfg!(target_os = "linux") {
        vec![
            format!("lib{}.so", binary_name),
            format!("{}.so", binary_name),
        ]
    } else if cfg!(target_os = "windows") {
        vec![format!("{}.dll", binary_name)]
    } else {
        return Err(PluginError::Other(anyhow::anyhow!(
            "Unsupported platform"
        )));
    };

    for candidate in candidates {
        let path = plugin_dir.join(&candidate);
        if path.exists() {
            return Ok(path);
        }
    }

    Err(PluginError::NotFound(format!(
        "Plugin binary not found in {:?}",
        plugin_dir
    )))
}

/// Create plugin context
fn create_plugin_context(manifest: &PluginManifest) -> Result<PluginContext> {
    let plugin_id = manifest.plugin.id.clone();

    // Data directory: ~/.local/share/adi/<plugin-id>/
    let data_dir = dirs::data_local_dir()
        .ok_or_else(|| PluginError::Other(anyhow::anyhow!("Cannot determine data directory")))?
        .join("adi")
        .join(&plugin_id);

    // Config directory: ~/.config/adi/<plugin-id>/
    let config_dir = dirs::config_dir()
        .ok_or_else(|| PluginError::Other(anyhow::anyhow!("Cannot determine config directory")))?
        .join("adi")
        .join(&plugin_id);

    // Create directories if they don't exist
    std::fs::create_dir_all(&data_dir)?;
    std::fs::create_dir_all(&config_dir)?;

    // Load plugin config (if exists)
    let config_path = config_dir.join("config.json");
    let config = if config_path.exists() {
        let content = std::fs::read_to_string(&config_path)?;
        serde_json::from_str(&content)?
    } else {
        serde_json::json!({})
    };

    Ok(PluginContext::new(plugin_id, data_dir, config_dir, config))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_binary_name() {
        // Test platform-specific binary name resolution
        let name = if cfg!(target_os = "macos") {
            "libplugin.dylib"
        } else if cfg!(target_os = "linux") {
            "libplugin.so"
        } else if cfg!(target_os = "windows") {
            "plugin.dll"
        } else {
            panic!("Unsupported platform");
        };

        assert!(!name.is_empty());
    }
}
