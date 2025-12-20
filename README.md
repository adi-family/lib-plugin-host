# lib-plugin-host

Plugin host for loading and managing plugins in the universal Rust plugin system.

## Overview

The main integration crate that applications use to:
- Scan and track installed plugins
- Download from registries
- Load native plugins (.so/.dylib/.dll)
- Manage plugin lifecycle

## Usage

```rust
use lib_plugin_host::{PluginHost, PluginConfig, HostCallbacks};
use std::path::PathBuf;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = PluginConfig::new(
        PathBuf::from("~/.myapp/plugins"),
        PathBuf::from("~/.cache/myapp/plugins"),
    )
    .with_registry("https://plugins.example.com")
    .with_host_version(env!("CARGO_PKG_VERSION"));

    let mut host = PluginHost::new(config)?;

    // Scan for installed plugins
    host.scan_installed()?;

    // Install from registry
    host.install_plugin("vendor.cool-plugin", "1.0.0").await?;

    // Enable (load and initialize)
    host.enable("vendor.cool-plugin")?;

    // Send messages to plugin
    let response = host.send_message("vendor.cool-plugin", "get_config", "{}")?;

    // Update all loaded plugins
    host.update_all()?;

    // Disable when done
    host.disable("vendor.cool-plugin")?;

    Ok(())
}
```

## Custom Callbacks

Implement `HostCallbacks` to customize how plugins interact with your app:

```rust
use lib_plugin_host::{HostCallbacks, PluginHost};

struct MyCallbacks { /* ... */ }

impl HostCallbacks for MyCallbacks {
    fn log(&self, level: u8, message: &str) {
        println!("[plugin] {}", message);
    }

    fn config_get(&self, key: &str) -> Option<String> {
        // Return config values
        None
    }

    fn config_set(&self, key: &str, value: &str) -> bool {
        // Store config values
        true
    }

    fn data_dir(&self) -> PathBuf {
        PathBuf::from("~/.myapp/plugin-data")
    }
}

let host = PluginHost::with_callbacks(config, Arc::new(MyCallbacks))?;
```

## Re-exports

This crate re-exports all plugin system crates for convenience:
- `lib_plugin_abi` - ABI types
- `lib_plugin_manifest` - Manifest parsing
- `lib_plugin_verify` - Verification
- `lib_plugin_registry` - Registry client

## License

MIT
