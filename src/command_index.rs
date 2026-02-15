//! Command index: symlink-based O(1) lookup for CLI commands.
//!
//! Maintains two layers of symlinks:
//!
//! 1. **`latest` symlink** inside each plugin dir — points to the current version directory.
//!    `adi.hive/latest -> 0.8.8/`
//!
//! 2. **`commands/` index** — maps command names to `plugin.toml` via the `latest` symlink.
//!    `commands/hive -> ../adi.hive/latest/plugin.toml`
//!
//! On version update, only the `latest` symlink changes — command index stays stable.

use std::path::{Path, PathBuf};

use lib_plugin_manifest::PluginManifest;

use crate::HostError;

/// Name of the command index directory inside the plugins directory.
pub const COMMANDS_DIR_NAME: &str = "commands";

/// Name of the symlink inside each plugin dir that points to the current version.
pub const LATEST_LINK_NAME: &str = "latest";

/// Returns the path to the commands index directory.
pub fn commands_dir(plugins_dir: &Path) -> PathBuf {
    plugins_dir.join(COMMANDS_DIR_NAME)
}

/// Create or update the `latest` symlink inside a plugin directory.
///
/// `<plugins_dir>/<plugin_id>/latest -> <version>/`
pub fn update_latest_link(
    plugins_dir: &Path,
    plugin_id: &str,
    version: &str,
) -> Result<(), HostError> {
    let plugin_dir = plugins_dir.join(plugin_id);
    let link_path = plugin_dir.join(LATEST_LINK_NAME);
    let target = PathBuf::from(version);

    let _ = std::fs::remove_file(&link_path);

    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &link_path)?;

    #[cfg(windows)]
    std::fs::write(&link_path, target.to_string_lossy().as_bytes())?;

    Ok(())
}

/// Create command symlinks for a plugin's CLI commands.
///
/// Creates symlinks: `<plugins_dir>/commands/<command>` -> `../<plugin_id>/latest/plugin.toml`
/// Also creates symlinks for all aliases.
///
/// Requires that the `latest` symlink already exists (call `update_latest_link` first).
pub fn create_command_symlinks(
    plugins_dir: &Path,
    plugin_id: &str,
    version: &str,
) -> Result<(), HostError> {
    let cmds_dir = commands_dir(plugins_dir);
    std::fs::create_dir_all(&cmds_dir)?;

    let manifest_path = plugins_dir
        .join(plugin_id)
        .join(version)
        .join("plugin.toml");
    let manifest = PluginManifest::from_file(&manifest_path)?;

    let Some(cli) = &manifest.cli else {
        return Ok(());
    };

    // Point through latest/ so symlinks survive version updates
    let target = PathBuf::from("..")
        .join(plugin_id)
        .join(LATEST_LINK_NAME)
        .join("plugin.toml");

    create_symlink(&cmds_dir, &cli.command, &target)?;

    for alias in &cli.aliases {
        create_symlink(&cmds_dir, alias, &target)?;
    }

    Ok(())
}

/// Remove all command symlinks that point to a given plugin.
pub fn remove_command_symlinks(plugins_dir: &Path, plugin_id: &str) -> Result<(), HostError> {
    let cmds_dir = commands_dir(plugins_dir);
    if !cmds_dir.exists() {
        return Ok(());
    }

    let expected_prefix = format!("../{}/", plugin_id);

    let entries = std::fs::read_dir(&cmds_dir)?;
    for entry in entries.flatten() {
        if let Ok(target) = std::fs::read_link(entry.path()) {
            if target.to_string_lossy().starts_with(&expected_prefix) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    Ok(())
}

/// Rebuild the entire command index from scratch.
///
/// Scans all installed plugins, creates `latest` symlinks and command index.
/// Used as a fallback when the commands/ directory is missing or corrupt.
pub fn rebuild_index(plugins_dir: &Path) -> Result<(), HostError> {
    let cmds_dir = commands_dir(plugins_dir);

    if cmds_dir.exists() {
        std::fs::remove_dir_all(&cmds_dir)?;
    }
    std::fs::create_dir_all(&cmds_dir)?;

    let entries = std::fs::read_dir(plugins_dir)?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if name_str == COMMANDS_DIR_NAME || !path.is_dir() {
            continue;
        }

        let version_file = path.join(".version");
        let Ok(version) = std::fs::read_to_string(&version_file) else {
            continue;
        };
        let version = version.trim();

        let _ = update_latest_link(plugins_dir, &name_str, version);
        let _ = create_command_symlinks(plugins_dir, &name_str, version);
    }

    Ok(())
}

/// Resolve a command name to its plugin manifest path via the index.
///
/// Returns `Some(absolute_path_to_plugin_toml)` if the symlink chain resolves.
pub fn resolve_command(plugins_dir: &Path, command: &str) -> Option<PathBuf> {
    let link_path = commands_dir(plugins_dir).join(command);

    if link_path.is_symlink() {
        std::fs::canonicalize(&link_path).ok()
    } else {
        None
    }
}

/// List all indexed commands.
///
/// Returns `Vec<(command_name, resolved_manifest_path)>` for all valid entries.
pub fn list_indexed_commands(plugins_dir: &Path) -> Vec<(String, PathBuf)> {
    let cmds_dir = commands_dir(plugins_dir);
    if !cmds_dir.exists() {
        return Vec::new();
    }

    let mut result = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&cmds_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if let Some(resolved) = resolve_command(plugins_dir, &name) {
                result.push((name, resolved));
            }
        }
    }
    result
}

/// Create a single symlink, removing any existing one first.
fn create_symlink(cmds_dir: &Path, name: &str, target: &Path) -> Result<(), HostError> {
    let link_path = cmds_dir.join(name);
    let _ = std::fs::remove_file(&link_path);

    #[cfg(unix)]
    std::os::unix::fs::symlink(target, &link_path)?;

    #[cfg(windows)]
    std::fs::write(&link_path, target.to_string_lossy().as_bytes())?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_plugin_toml(dir: &Path, plugin_id: &str, version: &str, command: &str, aliases: &[&str]) {
        let plugin_dir = dir.join(plugin_id).join(version);
        fs::create_dir_all(&plugin_dir).unwrap();

        let aliases_str = aliases
            .iter()
            .map(|a| format!("\"{}\"", a))
            .collect::<Vec<_>>()
            .join(", ");

        let toml = format!(
            r#"[plugin]
id = "{plugin_id}"
name = "Test"
version = "{version}"
type = "core"

[cli]
command = "{command}"
description = "Test command"
aliases = [{aliases_str}]

[binary]
name = "plugin"
"#
        );
        fs::write(plugin_dir.join("plugin.toml"), toml).unwrap();
        fs::write(dir.join(plugin_id).join(".version"), version).unwrap();
    }

    fn write_plugin_toml_no_cli(dir: &Path, plugin_id: &str, version: &str) {
        let plugin_dir = dir.join(plugin_id).join(version);
        fs::create_dir_all(&plugin_dir).unwrap();

        let toml = format!(
            r#"[plugin]
id = "{plugin_id}"
name = "Test"
version = "{version}"
type = "core"

[binary]
name = "plugin"
"#
        );
        fs::write(plugin_dir.join("plugin.toml"), toml).unwrap();
        fs::write(dir.join(plugin_id).join(".version"), version).unwrap();
    }

    #[test]
    fn test_update_latest_link() {
        let tmp = tempfile::tempdir().unwrap();
        let plugins_dir = tmp.path();

        write_plugin_toml(plugins_dir, "adi.hive", "0.8.8", "hive", &[]);
        update_latest_link(plugins_dir, "adi.hive", "0.8.8").unwrap();

        let latest = plugins_dir.join("adi.hive").join(LATEST_LINK_NAME);
        assert!(latest.is_symlink());
        assert!(latest.join("plugin.toml").exists());
    }

    #[test]
    fn test_update_latest_link_version_change() {
        let tmp = tempfile::tempdir().unwrap();
        let plugins_dir = tmp.path();

        write_plugin_toml(plugins_dir, "adi.hive", "0.8.8", "hive", &[]);
        write_plugin_toml(plugins_dir, "adi.hive", "0.9.0", "hive", &[]);

        update_latest_link(plugins_dir, "adi.hive", "0.8.8").unwrap();
        let latest = plugins_dir.join("adi.hive").join(LATEST_LINK_NAME);
        assert_eq!(fs::read_link(&latest).unwrap(), PathBuf::from("0.8.8"));

        update_latest_link(plugins_dir, "adi.hive", "0.9.0").unwrap();
        assert_eq!(fs::read_link(&latest).unwrap(), PathBuf::from("0.9.0"));
    }

    #[test]
    fn test_command_symlinks_use_latest() {
        let tmp = tempfile::tempdir().unwrap();
        let plugins_dir = tmp.path();

        write_plugin_toml(plugins_dir, "adi.hive", "0.8.8", "hive", &[]);
        update_latest_link(plugins_dir, "adi.hive", "0.8.8").unwrap();
        create_command_symlinks(plugins_dir, "adi.hive", "0.8.8").unwrap();

        // Command symlink points through latest/
        let link = commands_dir(plugins_dir).join("hive");
        let target = fs::read_link(&link).unwrap();
        assert_eq!(
            target,
            PathBuf::from("..").join("adi.hive").join("latest").join("plugin.toml")
        );

        assert!(resolve_command(plugins_dir, "hive").is_some());
    }

    #[test]
    fn test_version_update_preserves_command_symlinks() {
        let tmp = tempfile::tempdir().unwrap();
        let plugins_dir = tmp.path();

        // Install v0.8.8
        write_plugin_toml(plugins_dir, "adi.hive", "0.8.8", "hive", &[]);
        update_latest_link(plugins_dir, "adi.hive", "0.8.8").unwrap();
        create_command_symlinks(plugins_dir, "adi.hive", "0.8.8").unwrap();
        assert!(resolve_command(plugins_dir, "hive").is_some());

        // Install v0.9.0 — only update latest, command symlinks untouched
        write_plugin_toml(plugins_dir, "adi.hive", "0.9.0", "hive", &[]);
        update_latest_link(plugins_dir, "adi.hive", "0.9.0").unwrap();

        // Command symlink still resolves — now through 0.9.0
        let resolved = resolve_command(plugins_dir, "hive").unwrap();
        assert!(resolved.to_string_lossy().contains("0.9.0"));
    }

    #[test]
    fn test_create_command_symlinks_with_aliases() {
        let tmp = tempfile::tempdir().unwrap();
        let plugins_dir = tmp.path();

        write_plugin_toml(plugins_dir, "adi.tasks", "0.5.0", "tasks", &["t"]);
        update_latest_link(plugins_dir, "adi.tasks", "0.5.0").unwrap();
        create_command_symlinks(plugins_dir, "adi.tasks", "0.5.0").unwrap();

        assert!(resolve_command(plugins_dir, "tasks").is_some());
        assert!(resolve_command(plugins_dir, "t").is_some());
    }

    #[test]
    fn test_no_cli_section_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let plugins_dir = tmp.path();

        write_plugin_toml_no_cli(plugins_dir, "adi.embed", "1.0.0");
        update_latest_link(plugins_dir, "adi.embed", "1.0.0").unwrap();
        create_command_symlinks(plugins_dir, "adi.embed", "1.0.0").unwrap();

        assert!(list_indexed_commands(plugins_dir).is_empty());
    }

    #[test]
    fn test_remove_command_symlinks() {
        let tmp = tempfile::tempdir().unwrap();
        let plugins_dir = tmp.path();

        write_plugin_toml(plugins_dir, "adi.tasks", "0.5.0", "tasks", &["t"]);
        update_latest_link(plugins_dir, "adi.tasks", "0.5.0").unwrap();
        create_command_symlinks(plugins_dir, "adi.tasks", "0.5.0").unwrap();

        assert!(resolve_command(plugins_dir, "tasks").is_some());
        assert!(resolve_command(plugins_dir, "t").is_some());

        remove_command_symlinks(plugins_dir, "adi.tasks").unwrap();

        assert!(resolve_command(plugins_dir, "tasks").is_none());
        assert!(resolve_command(plugins_dir, "t").is_none());
    }

    #[test]
    fn test_rebuild_index() {
        let tmp = tempfile::tempdir().unwrap();
        let plugins_dir = tmp.path();

        write_plugin_toml(plugins_dir, "adi.hive", "0.8.8", "hive", &[]);
        write_plugin_toml(plugins_dir, "adi.tasks", "0.5.0", "tasks", &["t"]);
        write_plugin_toml_no_cli(plugins_dir, "adi.embed", "1.0.0");

        rebuild_index(plugins_dir).unwrap();

        let cmds = list_indexed_commands(plugins_dir);
        let names: Vec<_> = cmds.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"hive"));
        assert!(names.contains(&"tasks"));
        assert!(names.contains(&"t"));
        assert!(!names.contains(&"embed"));

        // latest symlinks should also exist
        assert!(plugins_dir.join("adi.hive").join(LATEST_LINK_NAME).is_symlink());
        assert!(plugins_dir.join("adi.tasks").join(LATEST_LINK_NAME).is_symlink());
    }

    #[test]
    fn test_resolve_nonexistent_command() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(resolve_command(tmp.path(), "nonexistent").is_none());
    }

    #[test]
    fn test_list_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(list_indexed_commands(tmp.path()).is_empty());
    }
}
