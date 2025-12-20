//! Service registry for inter-plugin communication.
//!
//! The service registry allows core plugins to register services
//! that extension plugins can discover and consume.

use std::collections::HashMap;
use std::sync::RwLock;

use lib_plugin_abi::{ServiceDescriptor, ServiceError, ServiceHandle, ServiceVersion};

/// Thread-safe service registry.
pub struct ServiceRegistry {
    services: RwLock<HashMap<String, RegisteredService>>,
}

struct RegisteredService {
    descriptor: ServiceDescriptor,
    handle: ServiceHandle,
}

impl ServiceRegistry {
    /// Create a new empty service registry.
    pub fn new() -> Self {
        Self {
            services: RwLock::new(HashMap::new()),
        }
    }

    /// Register a service.
    /// Returns error if service ID already registered.
    pub fn register(
        &self,
        descriptor: ServiceDescriptor,
        handle: ServiceHandle,
    ) -> Result<(), ServiceError> {
        let mut services = self
            .services
            .write()
            .map_err(|_| ServiceError::internal("Lock poisoned"))?;

        let id = descriptor.id.as_str().to_string();

        if services.contains_key(&id) {
            return Err(ServiceError::already_registered(&id));
        }

        tracing::info!(
            "Registered service: {} v{}.{}.{} from {}",
            id,
            descriptor.version.major,
            descriptor.version.minor,
            descriptor.version.patch,
            descriptor.provider_id.as_str(),
        );

        services.insert(id, RegisteredService { descriptor, handle });
        Ok(())
    }

    /// Lookup a service by ID.
    pub fn lookup(&self, service_id: &str) -> Option<ServiceHandle> {
        self.services
            .read()
            .ok()?
            .get(service_id)
            .map(|s| s.handle.clone())
    }

    /// Lookup service with version constraint.
    pub fn lookup_versioned(
        &self,
        service_id: &str,
        min_version: &ServiceVersion,
    ) -> Result<ServiceHandle, ServiceError> {
        let services = self
            .services
            .read()
            .map_err(|_| ServiceError::internal("Lock poisoned"))?;

        let registered = services
            .get(service_id)
            .ok_or_else(|| ServiceError::not_found(service_id))?;

        if !registered
            .descriptor
            .version
            .is_compatible_with(min_version)
        {
            return Err(ServiceError::version_mismatch(
                service_id,
                min_version,
                &registered.descriptor.version,
            ));
        }

        Ok(registered.handle.clone())
    }

    /// List all registered services.
    pub fn list(&self) -> Vec<ServiceDescriptor> {
        self.services
            .read()
            .map(|s| s.values().map(|r| r.descriptor.clone()).collect())
            .unwrap_or_default()
    }

    /// Unregister all services from a provider.
    pub fn unregister_provider(&self, provider_id: &str) {
        if let Ok(mut services) = self.services.write() {
            let to_remove: Vec<String> = services
                .iter()
                .filter(|(_, v)| v.descriptor.provider_id.as_str() == provider_id)
                .map(|(k, _)| k.clone())
                .collect();

            for key in to_remove {
                tracing::info!("Unregistered service: {} from {}", key, provider_id);
                services.remove(&key);
            }
        }
    }

    /// Check if a service is registered.
    pub fn has_service(&self, service_id: &str) -> bool {
        self.services
            .read()
            .map(|s| s.contains_key(service_id))
            .unwrap_or(false)
    }

    /// Get the number of registered services.
    pub fn len(&self) -> usize {
        self.services.read().map(|s| s.len()).unwrap_or(0)
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for ServiceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lib_plugin_abi::{ServiceId, ServiceVTable};
    use std::ffi::c_void;

    // Mock service vtable for testing
    extern "C" fn mock_invoke(
        _handle: *const c_void,
        _method: abi_stable::std_types::RStr<'_>,
        _args: abi_stable::std_types::RStr<'_>,
    ) -> abi_stable::std_types::RResult<abi_stable::std_types::RString, ServiceError> {
        abi_stable::std_types::RResult::ROk(abi_stable::std_types::RString::from("ok"))
    }

    extern "C" fn mock_list_methods(
        _handle: *const c_void,
    ) -> abi_stable::std_types::RVec<lib_plugin_abi::ServiceMethod> {
        abi_stable::std_types::RVec::new()
    }

    static MOCK_VTABLE: ServiceVTable = ServiceVTable {
        invoke: mock_invoke,
        list_methods: mock_list_methods,
    };

    fn create_test_handle(service_id: &str) -> ServiceHandle {
        unsafe {
            ServiceHandle::new(
                ServiceId::new(service_id),
                std::ptr::null(),
                &MOCK_VTABLE as *const _,
            )
        }
    }

    #[test]
    fn test_register_and_lookup() {
        let registry = ServiceRegistry::new();

        let descriptor =
            ServiceDescriptor::new("test.service", ServiceVersion::new(1, 0, 0), "test.plugin");
        let handle = create_test_handle("test.service");

        registry.register(descriptor, handle).unwrap();

        assert!(registry.has_service("test.service"));
        assert!(registry.lookup("test.service").is_some());
        assert!(registry.lookup("nonexistent").is_none());
    }

    #[test]
    fn test_duplicate_registration() {
        let registry = ServiceRegistry::new();

        let descriptor =
            ServiceDescriptor::new("test.service", ServiceVersion::new(1, 0, 0), "test.plugin");
        let handle = create_test_handle("test.service");

        registry
            .register(descriptor.clone(), handle.clone())
            .unwrap();
        let result = registry.register(descriptor, handle);

        assert!(result.is_err());
    }

    #[test]
    fn test_version_lookup() {
        let registry = ServiceRegistry::new();

        let descriptor =
            ServiceDescriptor::new("test.service", ServiceVersion::new(1, 2, 0), "test.plugin");
        let handle = create_test_handle("test.service");

        registry.register(descriptor, handle).unwrap();

        // Compatible versions
        assert!(registry
            .lookup_versioned("test.service", &ServiceVersion::new(1, 0, 0))
            .is_ok());
        assert!(registry
            .lookup_versioned("test.service", &ServiceVersion::new(1, 2, 0))
            .is_ok());

        // Incompatible versions
        assert!(registry
            .lookup_versioned("test.service", &ServiceVersion::new(1, 3, 0))
            .is_err());
        assert!(registry
            .lookup_versioned("test.service", &ServiceVersion::new(2, 0, 0))
            .is_err());
    }

    #[test]
    fn test_unregister_provider() {
        let registry = ServiceRegistry::new();

        // Register multiple services from same provider
        for i in 1..=3 {
            let descriptor = ServiceDescriptor::new(
                format!("test.service{}", i),
                ServiceVersion::new(1, 0, 0),
                "test.plugin",
            );
            let handle = create_test_handle(&format!("test.service{}", i));
            registry.register(descriptor, handle).unwrap();
        }

        // Register service from different provider
        let other_descriptor = ServiceDescriptor::new(
            "other.service",
            ServiceVersion::new(1, 0, 0),
            "other.plugin",
        );
        let other_handle = create_test_handle("other.service");
        registry.register(other_descriptor, other_handle).unwrap();

        assert_eq!(registry.len(), 4);

        registry.unregister_provider("test.plugin");

        assert_eq!(registry.len(), 1);
        assert!(!registry.has_service("test.service1"));
        assert!(registry.has_service("other.service"));
    }
}
