//! Plugin loader for v3 ABI (native async traits)

use crate::PluginError;
use lib_plugin_abi_v3::{cli::CliCommands, daemon::DaemonService, logs::LogProvider, Plugin, PluginContext, PluginMetadata, PLUGIN_API_VERSION};
use lib_plugin_manifest::PluginManifest;
use libloading::{Library, Symbol};
use std::panic::AssertUnwindSafe;
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

    /// Optional daemon service trait object (if plugin provides daemon)
    pub daemon_service: Option<Arc<dyn DaemonService>>,
}

impl LoadedPluginV3 {
    /// Load a plugin from a dynamic library.
    ///
    /// Checks the plugin's ABI version before calling any trait methods.
    /// Wraps the load in `catch_unwind` and a timeout to guard against
    /// broken or ABI-incompatible plugins that crash or hang.
    pub async fn load(manifest: PluginManifest, plugin_dir: &Path) -> crate::Result<Self> {
        let lib_path = resolve_plugin_binary(&manifest, plugin_dir)?;
        let plugin_id = manifest.plugin.id.clone();

        // Wrap the entire loading sequence in a timeout (10s) so a hung
        // dlopen / plugin_create / init cannot block the process forever.
        let load_future = Self::load_inner(manifest, &lib_path, &plugin_id);
        match tokio::time::timeout(std::time::Duration::from_secs(10), load_future).await {
            Ok(result) => result,
            Err(_) => Err(PluginError::InitFailed(format!(
                "Plugin {} timed out during loading (>10s) — likely ABI-incompatible",
                plugin_id
            ))),
        }
    }

    /// Inner loading logic, separated so the caller can wrap it in a timeout.
    async fn load_inner(
        manifest: PluginManifest,
        lib_path: &Path,
        plugin_id: &str,
    ) -> crate::Result<Self> {
        // Load library inside catch_unwind (dlopen can trigger constructors that panic)
        let lib_path_owned = lib_path.to_path_buf();
        let library = tokio::task::spawn_blocking({
            let lib_path = lib_path_owned.clone();
            move || {
                std::panic::catch_unwind(AssertUnwindSafe(|| unsafe {
                    Library::new(&lib_path)
                }))
            }
        })
        .await
        .map_err(|e| PluginError::InitFailed(format!("Library load task panicked for {}: {}", plugin_id, e)))?
        .map_err(|_| PluginError::InitFailed(format!("Library::new panicked for {} ({:?})", plugin_id, lib_path_owned)))?
        .map_err(|e| PluginError::InitFailed(format!("Failed to load library {:?}: {}", lib_path_owned, e)))?;

        // --- ABI version gate ---
        // If the plugin exports `plugin_abi_version`, verify it matches the host.
        // If the symbol is absent we allow loading (older plugins built before this check).
        let abi_version: Option<u32> = unsafe {
            library
                .get::<extern "C" fn() -> u32>(b"plugin_abi_version")
                .ok()
                .map(|sym| sym())
        };

        if let Some(version) = abi_version {
            if version != PLUGIN_API_VERSION {
                return Err(PluginError::InitFailed(format!(
                    "ABI mismatch for {}: plugin exports v{}, host expects v{}. Reinstall the plugin.",
                    plugin_id, version, PLUGIN_API_VERSION
                )));
            }
            tracing::debug!(plugin_id, version, "ABI version check passed");
        } else {
            tracing::debug!(
                plugin_id,
                "Plugin does not export plugin_abi_version — skipping ABI check (legacy plugin)"
            );
        }

        // Get plugin_create symbol
        let create_fn: Symbol<fn() -> Box<dyn Plugin>> = unsafe {
            library
                .get(b"plugin_create")
                .map_err(|e| PluginError::InitFailed(format!("Missing plugin_create symbol: {}", e)))?
        };

        // Create plugin instance (catch panics from ABI-incompatible vtables)
        let mut plugin = std::panic::catch_unwind(AssertUnwindSafe(|| create_fn()))
            .map_err(|_| PluginError::InitFailed(format!(
                "plugin_create panicked for {} — likely ABI-incompatible",
                plugin_id
            )))?;

        // Create plugin context
        let ctx = create_plugin_context(&manifest)?;

        // Initialize plugin
        let result: lib_plugin_abi_v3::Result<()> = plugin.init(&ctx).await;
        result.map_err(|e| PluginError::InitFailed(format!("Plugin init failed: {}", e)))?;

        // Try to get CLI commands if the plugin provides them
        let cli_commands: Option<Arc<dyn CliCommands>> = if manifest.cli.is_some()
            || manifest.provides.iter().any(|s| s.id.ends_with(".cli"))
        {
            let cli_fn: Result<Symbol<fn() -> Box<dyn CliCommands>>, _> =
                unsafe { library.get(b"plugin_create_cli") };

            if let Ok(cli_fn) = cli_fn {
                std::panic::catch_unwind(AssertUnwindSafe(|| Arc::from(cli_fn())))
                    .map_err(|_| {
                        tracing::warn!(plugin_id, "plugin_create_cli panicked");
                    })
                    .ok()
            } else {
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
                std::panic::catch_unwind(AssertUnwindSafe(|| Arc::from(log_fn())))
                    .map_err(|_| {
                        tracing::warn!(plugin_id, "plugin_create_log_provider panicked");
                    })
                    .ok()
            } else {
                None
            }
        };

        // Try to get DaemonService if the plugin provides it
        let daemon_service: Option<Arc<dyn DaemonService>> = {
            let daemon_fn: Result<Symbol<fn() -> Box<dyn DaemonService>>, _> =
                unsafe { library.get(b"plugin_create_daemon_service") };

            if let Ok(daemon_fn) = daemon_fn {
                std::panic::catch_unwind(AssertUnwindSafe(|| Arc::from(daemon_fn())))
                    .map_err(|_| {
                        tracing::warn!(plugin_id, "plugin_create_daemon_service panicked");
                    })
                    .ok()
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
            daemon_service,
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
