//! Plugin loader using libloading.

use std::path::Path;

use libloading::{Library, Symbol};

use lib_plugin_abi::{
    PluginContext, PluginEntryFn, PluginInfo, PluginVTable, PLUGIN_API_VERSION, PLUGIN_ENTRY_SYMBOL,
};
use lib_plugin_manifest::PluginManifest;

use crate::error::HostError;

/// A loaded plugin instance.
pub struct LoadedPlugin {
    /// The dynamic library handle (must be kept alive)
    _library: Library,
    /// Plugin vtable
    vtable: &'static PluginVTable,
    /// Plugin context
    context: Box<PluginContext>,
    /// Plugin manifest
    manifest: PluginManifest,
    /// Whether the plugin has been initialized
    initialized: bool,
}

impl LoadedPlugin {
    /// Get plugin info.
    pub fn info(&self) -> PluginInfo {
        (self.vtable.info)()
    }

    /// Get the manifest.
    pub fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    /// Check if initialized.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Initialize the plugin.
    pub fn init(&mut self) -> Result<(), HostError> {
        if self.initialized {
            return Ok(());
        }

        let result = (self.vtable.init)(&mut *self.context);
        if result == 0 {
            self.initialized = true;
            Ok(())
        } else {
            Err(HostError::InitFailed(format!(
                "Init returned error code: {}",
                result
            )))
        }
    }

    /// Update the plugin (if it supports updates).
    pub fn update(&mut self) -> Result<(), HostError> {
        if !self.initialized {
            return Err(HostError::NotEnabled(self.manifest.plugin.id.clone()));
        }

        if let abi_stable::std_types::ROption::RSome(update_fn) = self.vtable.update {
            let result = update_fn(&mut *self.context);
            if result != 0 {
                return Err(HostError::InitFailed(format!(
                    "Update returned error code: {}",
                    result
                )));
            }
        }
        Ok(())
    }

    /// Send a message to the plugin.
    pub fn send_message(&mut self, msg_type: &str, msg_data: &str) -> Result<String, HostError> {
        if !self.initialized {
            return Err(HostError::NotEnabled(self.manifest.plugin.id.clone()));
        }

        use abi_stable::std_types::{ROption, RResult, RStr};

        if let ROption::RSome(handler) = self.vtable.handle_message {
            let result = handler(
                &mut *self.context,
                RStr::from(msg_type),
                RStr::from(msg_data),
            );
            match result {
                RResult::ROk(response) => Ok(response.into()),
                RResult::RErr(e) => Err(HostError::InitFailed(e.message.into())),
            }
        } else {
            Ok(String::new())
        }
    }

    /// Cleanup the plugin.
    pub fn cleanup(&mut self) {
        if self.initialized {
            (self.vtable.cleanup)(&mut *self.context);
            self.initialized = false;
        }
    }
}

impl Drop for LoadedPlugin {
    fn drop(&mut self) {
        self.cleanup();
    }
}

/// Plugin loader.
pub struct PluginLoader {
    /// Host vtable pointer for creating contexts
    host_vtable: *const lib_plugin_abi::HostVTable,
}

impl PluginLoader {
    /// Create a new plugin loader.
    ///
    /// # Safety
    /// The host_vtable pointer must remain valid for the lifetime of this loader
    /// and any plugins loaded by it.
    pub unsafe fn new(host_vtable: *const lib_plugin_abi::HostVTable) -> Self {
        Self { host_vtable }
    }

    /// Load a plugin from a path.
    ///
    /// # Safety
    /// This loads native code which could be unsafe. Ensure you trust the plugin.
    pub unsafe fn load(
        &self,
        path: &Path,
        manifest: PluginManifest,
    ) -> Result<LoadedPlugin, HostError> {
        // Load the library
        let library = Library::new(path).map_err(|e| HostError::LoadFailed(e.to_string()))?;

        // Get the entry point
        let entry: Symbol<PluginEntryFn> = library
            .get(PLUGIN_ENTRY_SYMBOL.as_bytes())
            .map_err(|e| HostError::SymbolNotFound(e.to_string()))?;

        // Get vtable
        let vtable_ptr = entry();
        if vtable_ptr.is_null() {
            return Err(HostError::InvalidVTable);
        }

        // Safety: We trust the plugin to return a valid vtable
        let vtable: &'static PluginVTable = &*vtable_ptr;

        // Check API version
        let _info = (vtable.info)();
        // Note: The manifest stores the expected API version
        if manifest.compatibility.api_version != PLUGIN_API_VERSION {
            return Err(HostError::IncompatibleApiVersion {
                expected: PLUGIN_API_VERSION,
                actual: manifest.compatibility.api_version,
            });
        }

        // Create context
        let context = Box::new(PluginContext {
            api_version: PLUGIN_API_VERSION,
            host: self.host_vtable,
            user_data: std::ptr::null_mut(),
        });

        Ok(LoadedPlugin {
            _library: library,
            vtable,
            context,
            manifest,
            initialized: false,
        })
    }

    /// Load a plugin and initialize it.
    ///
    /// # Safety
    /// Same as `load`.
    pub unsafe fn load_and_init(
        &self,
        path: &Path,
        manifest: PluginManifest,
    ) -> Result<LoadedPlugin, HostError> {
        let mut plugin = self.load(path, manifest)?;
        plugin.init()?;
        Ok(plugin)
    }
}
