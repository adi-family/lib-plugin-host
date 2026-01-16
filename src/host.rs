//! Main plugin host implementation.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use lib_plugin_abi::ServiceVersion;
use lib_plugin_manifest::{current_platform, BinaryInfo, Manifest};
use lib_plugin_registry::{RegistryClient, SearchKind, SearchResults};
use lib_plugin_verify::{verify_checksum, Verifier};

use crate::callbacks::{CallbackBridge, DefaultCallbacks, HostCallbacks};
use crate::config::PluginConfig;
use crate::error::HostError;
use crate::installed::{InstallStatus, InstalledPackage, InstalledPlugin};
use crate::loader::{LoadedPlugin, PluginLoader};
use crate::service_registry::ServiceRegistry;

/// Main plugin host that manages all plugins.
pub struct PluginHost {
    config: PluginConfig,
    packages: HashMap<String, InstalledPackage>,
    plugins: HashMap<String, InstalledPlugin>,
    loaded: HashMap<String, LoadedPlugin>,
    install_status: HashMap<String, InstallStatus>,
    registry: Option<RegistryClient>,
    #[allow(dead_code)] // Will be used for signature verification
    verifier: Verifier,
    callback_bridge: CallbackBridge,
    /// Service registry for inter-plugin communication
    service_registry: Arc<ServiceRegistry>,
    /// Custom callbacks for host operations
    callbacks: Arc<dyn HostCallbacks>,
}

/// Find the binary path, trying multiple filename variants.
/// Handles both standard lib-prefixed names and bare names.
fn find_binary_path(dir: &Path, binary: &BinaryInfo) -> PathBuf {
    let ext = if cfg!(target_os = "macos") {
        "dylib"
    } else if cfg!(target_os = "windows") {
        "dll"
    } else {
        "so"
    };

    // Try variants in order of preference
    let variants = [
        format!("{}.{}", binary.name, ext),    // plugin.dylib
        format!("lib{}.{}", binary.name, ext), // libplugin.dylib
        format!("{}.{}", binary.name.trim_start_matches("lib"), ext), // plugin.dylib (if name is libplugin)
    ];

    for variant in &variants {
        let path = dir.join(variant);
        if path.exists() {
            return path;
        }
    }

    // Fallback to first variant (even if doesn't exist, for error messages)
    dir.join(&variants[0])
}

impl PluginHost {
    /// Create a new plugin host with default callbacks.
    pub fn new(config: PluginConfig) -> Result<Self, HostError> {
        let callbacks = Arc::new(DefaultCallbacks::new(config.plugins_dir.clone()));
        Self::with_callbacks(config, callbacks)
    }

    /// Create a new plugin host with custom callbacks.
    pub fn with_callbacks(
        config: PluginConfig,
        callbacks: Arc<dyn HostCallbacks>,
    ) -> Result<Self, HostError> {
        config.ensure_dirs()?;

        let registry = config
            .registry_url
            .as_ref()
            .map(|url| RegistryClient::new(url).with_cache(config.cache_dir.clone()));

        let verifier = Verifier::new()
            .with_trusted_keys(config.trusted_keys.iter().cloned())
            .require_signatures(config.require_signatures);

        let service_registry = Arc::new(ServiceRegistry::new());
        let callback_bridge = CallbackBridge::with_service_registry(
            callbacks.clone(),
            Some(service_registry.clone()),
        );

        Ok(Self {
            config,
            packages: HashMap::new(),
            plugins: HashMap::new(),
            loaded: HashMap::new(),
            install_status: HashMap::new(),
            registry,
            verifier,
            callback_bridge,
            service_registry,
            callbacks,
        })
    }

    /// Get the configuration.
    pub fn config(&self) -> &PluginConfig {
        &self.config
    }

    // === Discovery ===

    /// Scan the plugins directory for installed packages.
    pub fn scan_installed(&mut self) -> Result<(), HostError> {
        self.packages.clear();
        self.plugins.clear();

        let plugins_dir = &self.config.plugins_dir;
        if !plugins_dir.exists() {
            return Ok(());
        }

        for entry in std::fs::read_dir(plugins_dir)? {
            let entry = entry?;
            let path = entry.path();

            if !path.is_dir() {
                continue;
            }

            // Determine the actual plugin path (may be in versioned subdirectory)
            let plugin_path = if path.join(".version").exists() {
                // Versioned layout: plugins/id/.version + plugins/id/<version>/
                let version = std::fs::read_to_string(path.join(".version"))
                    .map(|v| v.trim().to_string())
                    .unwrap_or_default();
                if version.is_empty() {
                    continue;
                }
                path.join(&version)
            } else {
                // Flat layout: plugins/id/plugin.toml
                path.clone()
            };

            // Try to load manifest - skip plugins with invalid manifests rather than failing
            let manifest = if plugin_path.join("package.toml").exists() {
                match Manifest::from_file(&plugin_path.join("package.toml")) {
                    Ok(m) => m,
                    Err(e) => {
                        eprintln!("Warning: Skipping plugin {:?}: {}", path.file_name(), e);
                        continue;
                    }
                }
            } else if plugin_path.join("plugin.toml").exists() {
                match Manifest::from_file(&plugin_path.join("plugin.toml")) {
                    Ok(m) => m,
                    Err(e) => {
                        eprintln!("Warning: Skipping plugin {:?}: {}", path.file_name(), e);
                        continue;
                    }
                }
            } else {
                continue;
            };

            let plugin_ids = manifest
                .plugin_ids()
                .iter()
                .map(|s| s.to_string())
                .collect();

            // Register package
            let package = InstalledPackage {
                manifest: manifest.clone(),
                path: plugin_path.clone(),
                plugin_ids,
            };
            self.packages.insert(package.id().to_string(), package);

            // Register individual plugins
            match &manifest {
                Manifest::Single(m) => {
                    let binary_path = find_binary_path(&plugin_path, &m.binary);
                    let plugin = InstalledPlugin {
                        manifest: m.clone(),
                        path: binary_path,
                        package_id: m.plugin.id.clone(),
                        enabled: false,
                    };
                    self.plugins.insert(plugin.id().to_string(), plugin);
                }
                Manifest::Package(pm) => {
                    for plugin_manifest in pm.expand_plugins() {
                        let plugin_dir =
                            plugin_path.join("plugins").join(&plugin_manifest.plugin.id);
                        let binary_path = find_binary_path(&plugin_dir, &plugin_manifest.binary);
                        let plugin = InstalledPlugin {
                            path: binary_path,
                            package_id: pm.package.id.clone(),
                            manifest: plugin_manifest,
                            enabled: false,
                        };
                        self.plugins.insert(plugin.id().to_string(), plugin);
                    }
                }
            }
        }

        Ok(())
    }

    // === Package Operations ===

    /// Get all installed packages.
    pub fn packages(&self) -> impl Iterator<Item = &InstalledPackage> {
        self.packages.values()
    }

    /// Get a package by ID.
    pub fn get_package(&self, id: &str) -> Option<&InstalledPackage> {
        self.packages.get(id)
    }

    /// Install a package from the registry.
    pub async fn install_package(&mut self, id: &str, version: &str) -> Result<(), HostError> {
        let registry = self.registry.as_ref().ok_or(HostError::Registry(
            lib_plugin_registry::RegistryError::InvalidResponse("No registry configured".into()),
        ))?;

        // Update status
        self.install_status
            .insert(id.to_string(), InstallStatus::Installing { progress: 0.0 });

        // Get package info
        let info = registry.get_package_version(id, version).await?;

        // Find platform build
        let platform = current_platform();
        let build = info
            .platforms
            .iter()
            .find(|p| p.platform == platform)
            .ok_or(HostError::PlatformNotSupported(platform.clone()))?;

        // Download
        let data = registry
            .download_package(id, version, &platform, |done, total| {
                let progress = if total > 0 {
                    done as f32 / total as f32
                } else {
                    0.0
                };
                // Could update status here with progress
                let _ = progress;
            })
            .await?;

        // Verify checksum
        if !verify_checksum(&data, &build.checksum) {
            self.install_status.insert(
                id.to_string(),
                InstallStatus::Failed {
                    error: "Checksum mismatch".into(),
                },
            );
            return Err(HostError::SignatureInvalid("Checksum mismatch".into()));
        }

        // Extract to plugins directory
        let package_dir = self.config.plugins_dir.join(id);
        std::fs::create_dir_all(&package_dir)?;

        // TODO: Actually extract the tarball
        // For now, just write the raw data as a placeholder
        std::fs::write(package_dir.join("package.tar.gz"), &data)?;

        // Update status
        self.install_status.insert(
            id.to_string(),
            InstallStatus::Installed {
                version: version.to_string(),
            },
        );

        // Rescan
        self.scan_installed()?;

        Ok(())
    }

    /// Uninstall a package.
    pub fn uninstall_package(&mut self, id: &str) -> Result<(), HostError> {
        // Get plugin IDs and path first (clone to avoid borrow issues)
        let (plugin_ids, path) = {
            let package = self
                .packages
                .get(id)
                .ok_or_else(|| HostError::PackageNotFound(id.to_string()))?;
            (package.plugin_ids.clone(), package.path.clone())
        };

        // Disable all plugins first
        for plugin_id in &plugin_ids {
            let _ = self.disable(plugin_id);
        }

        // Remove from disk
        std::fs::remove_dir_all(&path)?;

        // Remove from tracking
        self.packages.remove(id);
        for plugin_id in &plugin_ids {
            self.plugins.remove(plugin_id);
        }
        self.install_status.remove(id);

        Ok(())
    }

    // === Plugin Operations ===

    /// Get all installed plugins.
    pub fn plugins(&self) -> impl Iterator<Item = &InstalledPlugin> {
        self.plugins.values()
    }

    /// Get a plugin by ID.
    pub fn get_plugin(&self, id: &str) -> Option<&InstalledPlugin> {
        self.plugins.get(id)
    }

    /// Install a single plugin from the registry.
    pub async fn install_plugin(&mut self, id: &str, version: &str) -> Result<(), HostError> {
        // For now, single plugins are treated as single-plugin packages
        self.install_package(id, version).await
    }

    /// Uninstall a plugin.
    ///
    /// If the plugin is part of a multi-plugin package, only disables it.
    /// To fully remove, uninstall the package.
    pub fn uninstall_plugin(&mut self, id: &str) -> Result<(), HostError> {
        let plugin = self
            .plugins
            .get(id)
            .ok_or_else(|| HostError::PluginNotFound(id.to_string()))?;

        // Check if it's a standalone plugin
        if plugin.package_id == id {
            // It's standalone, uninstall the package
            self.uninstall_package(id)
        } else {
            // It's part of a package, just disable it
            self.disable(id)
        }
    }

    // === Search ===

    /// Search the registry.
    pub async fn search(&self, query: &str) -> Result<SearchResults, HostError> {
        let registry = self.registry.as_ref().ok_or(HostError::Registry(
            lib_plugin_registry::RegistryError::InvalidResponse("No registry configured".into()),
        ))?;

        Ok(registry.search(query, SearchKind::All).await?)
    }

    // === Lifecycle ===

    /// Enable a plugin (load and initialize).
    pub fn enable(&mut self, plugin_id: &str) -> Result<(), HostError> {
        if self.loaded.contains_key(plugin_id) {
            return Ok(()); // Already enabled
        }

        let plugin = self
            .plugins
            .get(plugin_id)
            .ok_or_else(|| HostError::PluginNotFound(plugin_id.to_string()))?;

        if !plugin.path.exists() {
            return Err(HostError::LoadFailed(format!(
                "Plugin binary not found: {:?}",
                plugin.path
            )));
        }

        // Update the callback bridge with the current service registry
        let callback_bridge = CallbackBridge::with_service_registry(
            self.callbacks.clone(),
            Some(self.service_registry.clone()),
        );

        // Load the plugin
        let loader = unsafe { PluginLoader::new(callback_bridge.vtable_ptr()) };
        let loaded = unsafe { loader.load_and_init(&plugin.path, plugin.manifest.clone())? };

        // Mark as enabled
        if let Some(p) = self.plugins.get_mut(plugin_id) {
            p.enabled = true;
        }

        self.loaded.insert(plugin_id.to_string(), loaded);
        // Store the callback bridge (keep it alive)
        self.callback_bridge = callback_bridge;
        Ok(())
    }

    /// Disable a plugin (cleanup and unload).
    pub fn disable(&mut self, plugin_id: &str) -> Result<(), HostError> {
        if let Some(mut loaded) = self.loaded.remove(plugin_id) {
            loaded.cleanup();
        }

        if let Some(p) = self.plugins.get_mut(plugin_id) {
            p.enabled = false;
        }

        Ok(())
    }

    /// Enable all plugins in a package.
    pub fn enable_package(&mut self, package_id: &str) -> Result<(), HostError> {
        let package = self
            .packages
            .get(package_id)
            .ok_or_else(|| HostError::PackageNotFound(package_id.to_string()))?;

        let plugin_ids = package.plugin_ids.clone();
        for plugin_id in plugin_ids {
            self.enable(&plugin_id)?;
        }

        Ok(())
    }

    /// Disable all plugins in a package.
    pub fn disable_package(&mut self, package_id: &str) -> Result<(), HostError> {
        let package = self
            .packages
            .get(package_id)
            .ok_or_else(|| HostError::PackageNotFound(package_id.to_string()))?;

        let plugin_ids = package.plugin_ids.clone();
        for plugin_id in plugin_ids {
            let _ = self.disable(&plugin_id);
        }

        Ok(())
    }

    // === Runtime ===

    /// Check if a plugin is loaded.
    pub fn is_loaded(&self, plugin_id: &str) -> bool {
        self.loaded.contains_key(plugin_id)
    }

    /// Send a message to a plugin.
    pub fn send_message(
        &mut self,
        plugin_id: &str,
        msg_type: &str,
        msg_data: &str,
    ) -> Result<String, HostError> {
        let loaded = self
            .loaded
            .get_mut(plugin_id)
            .ok_or_else(|| HostError::NotEnabled(plugin_id.to_string()))?;

        loaded.send_message(msg_type, msg_data)
    }

    /// Update all loaded plugins.
    pub fn update_all(&mut self) -> Result<(), HostError> {
        for loaded in self.loaded.values_mut() {
            loaded.update()?;
        }
        Ok(())
    }

    /// Get install status for a package/plugin.
    pub fn install_status(&self, id: &str) -> InstallStatus {
        self.install_status
            .get(id)
            .cloned()
            .unwrap_or(InstallStatus::NotInstalled)
    }

    // === Service Registry ===

    /// Get the service registry.
    pub fn service_registry(&self) -> &Arc<ServiceRegistry> {
        &self.service_registry
    }

    // === Dependency-Aware Loading ===

    /// Enable a plugin with all its dependencies.
    ///
    /// This method loads plugins in dependency order using topological sort.
    /// If any dependency fails to load, the operation fails.
    pub fn enable_with_dependencies(&mut self, plugin_id: &str) -> Result<(), HostError> {
        let load_order = self.resolve_load_order(plugin_id)?;

        for id in load_order {
            if !self.loaded.contains_key(&id) {
                self.enable_single(&id).map_err(|e| {
                    if id != plugin_id {
                        HostError::DependencyLoadFailed {
                            plugin: plugin_id.to_string(),
                            dependency: id.clone(),
                            error: e.to_string(),
                        }
                    } else {
                        e
                    }
                })?;
            }
        }

        Ok(())
    }

    /// Resolve load order via topological sort.
    fn resolve_load_order(&self, plugin_id: &str) -> Result<Vec<String>, HostError> {
        let mut result = Vec::new();
        let mut visited = HashSet::new();
        let mut in_progress = HashSet::new();

        self.visit_deps(plugin_id, &mut visited, &mut in_progress, &mut result)?;

        Ok(result)
    }

    fn visit_deps(
        &self,
        id: &str,
        visited: &mut HashSet<String>,
        in_progress: &mut HashSet<String>,
        result: &mut Vec<String>,
    ) -> Result<(), HostError> {
        if visited.contains(id) {
            return Ok(());
        }

        if in_progress.contains(id) {
            return Err(HostError::CircularDependency(id.to_string()));
        }

        in_progress.insert(id.to_string());

        // Get plugin dependencies
        if let Some(plugin) = self.plugins.get(id) {
            for dep in &plugin.manifest.compatibility.depends_on {
                // Check if dependency exists
                if !self.plugins.contains_key(dep) {
                    return Err(HostError::DependencyNotFound(dep.clone()));
                }
                self.visit_deps(dep, visited, in_progress, result)?;
            }
        }

        in_progress.remove(id);
        visited.insert(id.to_string());
        result.push(id.to_string());

        Ok(())
    }

    /// Enable a single plugin (internal, assumes deps already loaded).
    fn enable_single(&mut self, plugin_id: &str) -> Result<(), HostError> {
        if self.loaded.contains_key(plugin_id) {
            return Ok(()); // Already enabled
        }

        let plugin = self
            .plugins
            .get(plugin_id)
            .ok_or_else(|| HostError::PluginNotFound(plugin_id.to_string()))?;

        if !plugin.path.exists() {
            return Err(HostError::LoadFailed(format!(
                "Plugin binary not found: {:?}",
                plugin.path
            )));
        }

        // Verify required services are available
        self.verify_required_services(plugin_id)?;

        // Update the callback bridge with the current service registry
        // (needed because thread-local storage is per-thread)
        let callback_bridge = CallbackBridge::with_service_registry(
            self.callbacks.clone(),
            Some(self.service_registry.clone()),
        );

        // Load the plugin
        let loader = unsafe { PluginLoader::new(callback_bridge.vtable_ptr()) };
        let loaded = unsafe { loader.load_and_init(&plugin.path, plugin.manifest.clone())? };

        // Mark as enabled
        if let Some(p) = self.plugins.get_mut(plugin_id) {
            p.enabled = true;
        }

        self.loaded.insert(plugin_id.to_string(), loaded);
        // Store the callback bridge (keep it alive)
        self.callback_bridge = callback_bridge;

        Ok(())
    }

    /// Verify all required services are registered.
    fn verify_required_services(&self, plugin_id: &str) -> Result<(), HostError> {
        let plugin = self
            .plugins
            .get(plugin_id)
            .ok_or_else(|| HostError::PluginNotFound(plugin_id.to_string()))?;

        for req in &plugin.manifest.requires {
            if !req.optional {
                if let Some(min_version_str) = &req.min_version {
                    let min_version = ServiceVersion::parse(min_version_str).ok_or_else(|| {
                        HostError::ServiceNotAvailable {
                            service: req.id.clone(),
                            error: format!("Invalid version string: {}", min_version_str),
                        }
                    })?;

                    self.service_registry
                        .lookup_versioned(&req.id, &min_version)
                        .map_err(|e| HostError::ServiceNotAvailable {
                            service: req.id.clone(),
                            error: e.message.to_string(),
                        })?;
                } else if self.service_registry.lookup(&req.id).is_none() {
                    return Err(HostError::ServiceNotAvailable {
                        service: req.id.clone(),
                        error: "Service not found".to_string(),
                    });
                }
            }
        }

        Ok(())
    }

    /// Disable plugin and cascade to dependents.
    pub fn disable_with_dependents(&mut self, plugin_id: &str) -> Result<(), HostError> {
        // Find all plugins that depend on this one
        let dependents = self.find_dependents(plugin_id);

        // Disable in reverse order (dependents first)
        for dep_id in dependents.into_iter().rev() {
            if let Some(mut loaded) = self.loaded.remove(&dep_id) {
                loaded.cleanup();
            }
            if let Some(p) = self.plugins.get_mut(&dep_id) {
                p.enabled = false;
            }
        }

        // Unregister services from this provider
        self.service_registry.unregister_provider(plugin_id);

        // Disable the plugin itself
        if let Some(mut loaded) = self.loaded.remove(plugin_id) {
            loaded.cleanup();
        }
        if let Some(p) = self.plugins.get_mut(plugin_id) {
            p.enabled = false;
        }

        Ok(())
    }

    /// Find all plugins that depend on the given plugin.
    fn find_dependents(&self, plugin_id: &str) -> Vec<String> {
        let mut dependents = Vec::new();
        let mut to_check = vec![plugin_id.to_string()];
        let mut checked = HashSet::new();

        while let Some(id) = to_check.pop() {
            if checked.contains(&id) {
                continue;
            }
            checked.insert(id.clone());

            for (other_id, plugin) in &self.plugins {
                if plugin.manifest.compatibility.depends_on.contains(&id)
                    && !checked.contains(other_id)
                {
                    dependents.push(other_id.clone());
                    to_check.push(other_id.clone());
                }
            }
        }

        dependents
    }
}
