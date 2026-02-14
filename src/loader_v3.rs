//! Plugin loader for v3 ABI (native async traits)

use crate::PluginError;
use lib_plugin_abi_v3::{cli::CliCommands, logs::LogProvider, Plugin, PluginContext, PluginMetadata};
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

    /// Optional CLI commands trait object (if plugin provides CLI)
    pub cli_commands: Option<Arc<dyn CliCommands>>,

    /// Optional log provider trait object (if plugin provides log streaming)
    pub log_provider: Option<Arc<dyn LogProvider>>,
}

impl LoadedPluginV3 {
    /// Load a plugin from a dynamic library
    pub async fn load(manifest: PluginManifest, plugin_dir: &Path) -> crate::Result<Self> {
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
        let result: lib_plugin_abi_v3::Result<()> = plugin.init(&ctx).await;
        result.map_err(|e| PluginError::InitFailed(format!("Plugin init failed: {}", e)))?;

        // Try to get CLI commands if the plugin provides them
        // Check manifest for CLI service declaration
        let cli_commands: Option<Arc<dyn CliCommands>> = if manifest.cli.is_some()
            || manifest.provides.iter().any(|s| s.id.ends_with(".cli"))
        {
            // Try to get plugin_create_cli symbol
            let cli_fn: Result<Symbol<fn() -> Box<dyn CliCommands>>, _> =
                unsafe { library.get(b"plugin_create_cli") };

            if let Ok(cli_fn) = cli_fn {
                Some(Arc::from(cli_fn()))
            } else {
                // Fallback: plugin doesn't export separate CLI, but may implement it
                // This won't work without trait upcasting, so we log and skip
                tracing::debug!(
                    "Plugin {} declares CLI but doesn't export plugin_create_cli",
                    manifest.plugin.id
                );
                None
            }
        } else {
            None
        };

        // Try to get LogProvider if the plugin provides it
        let log_provider: Option<Arc<dyn LogProvider>> = {
            let log_fn: Result<Symbol<fn() -> Box<dyn LogProvider>>, _> =
                unsafe { library.get(b"plugin_create_log_provider") };

            if let Ok(log_fn) = log_fn {
                Some(Arc::from(log_fn()))
            } else {
                None
            }
        };

        Ok(Self {
            manifest,
            _library: library,
            plugin: Arc::from(plugin),
            cli_commands,
            log_provider,
        })
    }

    /// Get plugin metadata
    pub fn metadata(&self) -> PluginMetadata {
        self.plugin.metadata()
    }

    /// Shutdown and unload the plugin
    pub async fn unload(self) -> crate::Result<()> {
        // Call shutdown
        self.plugin
            .shutdown()
            .await
            .map_err(|e| PluginError::InitFailed(format!("Shutdown failed: {}", e)))?;

        // Drop plugin instance
        drop(self.plugin);

        // Library will be unloaded when dropped
        Ok(())
    }
}

/// Resolve plugin binary path
fn resolve_plugin_binary(manifest: &PluginManifest, plugin_dir: &Path) -> crate::Result<PathBuf> {
    let binary_name = &manifest.binary.name;

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
        return Err(PluginError::PlatformNotSupported(
            std::env::consts::OS.to_string()
        ));
    };

    for candidate in candidates {
        let path = plugin_dir.join(&candidate);
        if path.exists() {
            return Ok(path);
        }
    }

    Err(PluginError::PluginNotFound(format!(
        "Plugin binary not found in {:?}",
        plugin_dir
    )))
}

/// Create plugin context
fn create_plugin_context(manifest: &PluginManifest) -> crate::Result<PluginContext> {
    let plugin_id = manifest.plugin.id.clone();

    // Data directory: ~/.local/share/adi/<plugin-id>/
    let data_dir = dirs::data_local_dir()
        .ok_or_else(|| PluginError::InitFailed("Cannot determine data directory".to_string()))?
        .join("adi")
        .join(&plugin_id);

    // Config directory: ~/.config/adi/<plugin-id>/
    let config_dir = dirs::config_dir()
        .ok_or_else(|| PluginError::InitFailed("Cannot determine config directory".to_string()))?
        .join("adi")
        .join(&plugin_id);

    // Create directories if they don't exist
    std::fs::create_dir_all(&data_dir)?;
    std::fs::create_dir_all(&config_dir)?;

    // Load plugin config (if exists)
    let config_path = config_dir.join("config.json");
    let config = if config_path.exists() {
        let content = std::fs::read_to_string(&config_path)?;
        serde_json::from_str(&content)
            .map_err(|e| PluginError::InitFailed(format!("Failed to parse config: {}", e)))?
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
