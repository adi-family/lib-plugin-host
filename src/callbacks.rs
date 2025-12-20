//! Host callbacks that plugins can use.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use abi_stable::std_types::{ROption, RResult, RStr, RString, RVec};
use lib_plugin_abi::{
    HostVTable, OptionalServiceHandle, PluginContext, ServiceDescriptor, ServiceError,
    ServiceHandle, ServiceLookupResult, ServiceVersion, PLUGIN_API_VERSION,
};

use crate::service_registry::ServiceRegistry;

/// Trait for applications to implement custom host callbacks.
pub trait HostCallbacks: Send + Sync {
    /// Log a message. Level: 0=trace, 1=debug, 2=info, 3=warn, 4=error
    fn log(&self, level: u8, message: &str);

    /// Get a configuration value.
    fn config_get(&self, key: &str) -> Option<String>;

    /// Set a configuration value. Returns true on success.
    fn config_set(&self, key: &str, value: &str) -> bool;

    /// Get the plugin's data directory.
    fn data_dir(&self) -> PathBuf;

    /// Show a toast notification (optional).
    fn toast(&self, _level: u8, _message: &str) {}

    /// Perform a host action (optional). Returns result as JSON.
    fn host_action(&self, _action: &str, _data: &str) -> Result<String, String> {
        Err("Not implemented".to_string())
    }
}

/// Default implementation of host callbacks.
pub struct DefaultCallbacks {
    data_dir: PathBuf,
    config: Arc<RwLock<HashMap<String, String>>>,
}

impl DefaultCallbacks {
    /// Create new default callbacks.
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            data_dir,
            config: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Set initial config values.
    pub fn with_config(self, config: HashMap<String, String>) -> Self {
        *self.config.write().unwrap() = config;
        self
    }
}

impl HostCallbacks for DefaultCallbacks {
    fn log(&self, level: u8, message: &str) {
        match level {
            0 => tracing::trace!("[plugin] {}", message),
            1 => tracing::debug!("[plugin] {}", message),
            2 => tracing::info!("[plugin] {}", message),
            3 => tracing::warn!("[plugin] {}", message),
            _ => tracing::error!("[plugin] {}", message),
        }
    }

    fn config_get(&self, key: &str) -> Option<String> {
        self.config.read().ok()?.get(key).cloned()
    }

    fn config_set(&self, key: &str, value: &str) -> bool {
        if let Ok(mut config) = self.config.write() {
            config.insert(key.to_string(), value.to_string());
            true
        } else {
            false
        }
    }

    fn data_dir(&self) -> PathBuf {
        self.data_dir.clone()
    }
}

/// Bridge between Rust callbacks and C ABI.
pub struct CallbackBridge {
    #[allow(dead_code)] // Used to keep callbacks alive
    callbacks: Arc<dyn HostCallbacks>,
    #[allow(dead_code)] // Used to keep service registry alive
    service_registry: Option<Arc<ServiceRegistry>>,
    vtable: Box<HostVTable>,
}

impl CallbackBridge {
    /// Create a new callback bridge without service registry.
    pub fn new(callbacks: Arc<dyn HostCallbacks>) -> Self {
        Self::with_service_registry(callbacks, None)
    }

    /// Create a new callback bridge with service registry.
    pub fn with_service_registry(
        callbacks: Arc<dyn HostCallbacks>,
        service_registry: Option<Arc<ServiceRegistry>>,
    ) -> Self {
        // Store callbacks in thread-local for C callback access
        CURRENT_CALLBACKS.with(|c| {
            *c.borrow_mut() = Some(callbacks.clone());
        });

        // Store service registry in thread-local
        CURRENT_SERVICE_REGISTRY.with(|s| {
            *s.borrow_mut() = service_registry.clone();
        });

        let vtable = Box::new(HostVTable {
            log: bridge_log,
            config_get: bridge_config_get,
            config_set: bridge_config_set,
            data_dir: bridge_data_dir,
            toast: ROption::RSome(bridge_toast),
            host_action: ROption::RNone,
            // Service registry callbacks
            register_service: bridge_register_service,
            lookup_service: bridge_lookup_service,
            lookup_service_versioned: bridge_lookup_service_versioned,
            list_services: bridge_list_services,
        });

        Self {
            callbacks,
            service_registry,
            vtable,
        }
    }

    /// Get a pointer to the vtable.
    pub fn vtable_ptr(&self) -> *const HostVTable {
        &*self.vtable
    }

    /// Create a plugin context.
    pub fn create_context(&self) -> PluginContext {
        PluginContext {
            api_version: PLUGIN_API_VERSION,
            host: self.vtable_ptr(),
            user_data: std::ptr::null_mut(),
        }
    }
}

// Thread-local storage for callbacks (needed for C ABI callbacks)
thread_local! {
    static CURRENT_CALLBACKS: std::cell::RefCell<Option<Arc<dyn HostCallbacks>>> = const { std::cell::RefCell::new(None) };
    static CURRENT_SERVICE_REGISTRY: std::cell::RefCell<Option<Arc<ServiceRegistry>>> = const { std::cell::RefCell::new(None) };
}

fn with_callbacks<T>(f: impl FnOnce(&dyn HostCallbacks) -> T) -> Option<T> {
    CURRENT_CALLBACKS.with(|c| c.borrow().as_ref().map(|cb| f(cb.as_ref())))
}

extern "C" fn bridge_log(level: u8, message: RStr<'_>) {
    with_callbacks(|cb| cb.log(level, message.as_str()));
}

extern "C" fn bridge_config_get(key: RStr<'_>) -> ROption<RString> {
    with_callbacks(|cb| {
        cb.config_get(key.as_str())
            .map(|v| ROption::RSome(RString::from(v)))
            .unwrap_or(ROption::RNone)
    })
    .unwrap_or(ROption::RNone)
}

extern "C" fn bridge_config_set(key: RStr<'_>, value: RStr<'_>) -> i32 {
    with_callbacks(|cb| {
        if cb.config_set(key.as_str(), value.as_str()) {
            0
        } else {
            1
        }
    })
    .unwrap_or(1)
}

extern "C" fn bridge_data_dir() -> RString {
    with_callbacks(|cb| RString::from(cb.data_dir().to_string_lossy().to_string()))
        .unwrap_or_else(|| RString::from("."))
}

extern "C" fn bridge_toast(level: u8, message: RStr<'_>) {
    with_callbacks(|cb| cb.toast(level, message.as_str()));
}

// === Service Registry Bridge Functions ===

fn with_service_registry<T>(f: impl FnOnce(&ServiceRegistry) -> T) -> Option<T> {
    CURRENT_SERVICE_REGISTRY.with(|s| s.borrow().as_ref().map(|sr| f(sr.as_ref())))
}

extern "C" fn bridge_register_service(descriptor: ServiceDescriptor, handle: ServiceHandle) -> i32 {
    with_service_registry(|sr| match sr.register(descriptor, handle) {
        Ok(()) => 0,
        Err(e) => {
            tracing::error!("Service registration failed: {}", e);
            e.code
        }
    })
    .unwrap_or(1) // Return error if no service registry
}

extern "C" fn bridge_lookup_service(service_id: RStr<'_>) -> OptionalServiceHandle {
    with_service_registry(|sr| {
        sr.lookup(service_id.as_str())
            .map(ROption::RSome)
            .unwrap_or(ROption::RNone)
    })
    .unwrap_or(ROption::RNone)
}

extern "C" fn bridge_lookup_service_versioned(
    service_id: RStr<'_>,
    min_version: ServiceVersion,
) -> ServiceLookupResult {
    with_service_registry(|sr| {
        sr.lookup_versioned(service_id.as_str(), &min_version)
            .map(RResult::ROk)
            .unwrap_or_else(RResult::RErr)
    })
    .unwrap_or_else(|| RResult::RErr(ServiceError::internal("No service registry available")))
}

extern "C" fn bridge_list_services() -> RVec<ServiceDescriptor> {
    with_service_registry(|sr| sr.list().into_iter().collect()).unwrap_or_else(RVec::new)
}
