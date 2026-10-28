//! The tools a run spawns: where `nixos-rebuild`, `home-manager`, `nix-env`
//! and `nix-collect-garbage` are, and the argv a rebuild is spelled with.

use std::path::Path;

use crate::common::scope::Scope;
use crate::common::time::backup_stamp;

#[derive(Debug, Clone)]
pub struct ToolPaths {
    pub nixos_rebuild: String,
    pub home_manager: String,
    pub nix_env: String,
    pub nix_collect_garbage: String,
    /// Declared when the flake reads a path outside its own tree; every checkout-reading command then carries `--impure`.
    pub impure: bool,
}

/// Env names forwarded to an elevated step, so a nested call reads the same tools.
pub const FORWARDED_TOOLS: &[&str] = &[
    "NIXOS_REBUILD_PATH",
    "HOME_MANAGER_PATH",
    "NIX_ENV_PATH",
    "NIX_COLLECT_GARBAGE_PATH",
];

impl ToolPaths {
    /// The path behind a forwarded name, or `None` when the name is not one of them.
    pub fn path(&self, variable: &str) -> Option<&str> {
        match variable {
            "NIXOS_REBUILD_PATH" => Some(&self.nixos_rebuild),
            "HOME_MANAGER_PATH" => Some(&self.home_manager),
            "NIX_ENV_PATH" => Some(&self.nix_env),
            "NIX_COLLECT_GARBAGE_PATH" => Some(&self.nix_collect_garbage),
            _ => None,
        }
    }
}

impl ToolPaths {
    pub fn from_env() -> Self {
        Self {
            nixos_rebuild: tool("NIXOS_REBUILD_PATH", "nixos-rebuild"),
            home_manager: tool("HOME_MANAGER_PATH", "home-manager"),
            nix_env: tool("NIX_ENV_PATH", "nix-env"),
            nix_collect_garbage: tool("NIX_COLLECT_GARBAGE_PATH", "nix-collect-garbage"),
            // `impure` is a declaration the caller applies, so tests never depend on the environment.
            impure: false,
        }
    }

    /// The `nix` binary, which sits beside the `nix-env` the wrapper points at.
    pub fn nix(&self) -> String {
        Path::new(&self.nix_env)
            .with_file_name("nix")
            .display()
            .to_string()
    }

    pub fn rebuild_command(&self, scope: Scope, flake: &str, boot: bool) -> Vec<String> {
        self.rebuild_command_at(scope, flake, boot, &backup_stamp(), std::process::id())
    }

    /// Append `--impure` only when the machine declared it; pure is Nix's default.
    pub fn push_impure(&self, command: &mut Vec<String>) {
        if self.impure {
            command.push("--impure".into());
        }
    }

    /// The rebuild argv with a caller-supplied backup stamp, so tests assert the exact vector.
    /// Home Manager's `-b` suffix must stay unique or a second change to a path refuses to switch.
    pub fn rebuild_command_at(
        &self,
        scope: Scope,
        flake: &str,
        boot: bool,
        stamp: &str,
        pid: u32,
    ) -> Vec<String> {
        match scope {
            Scope::System => {
                let mut command = vec![
                    self.nixos_rebuild.clone(),
                    if boot { "boot" } else { "switch" }.into(),
                    "--flake".into(),
                    flake.into(),
                ];
                self.push_impure(&mut command);
                command
            }
            Scope::Home => {
                let mut command = vec![
                    self.home_manager.clone(),
                    "switch".into(),
                    "--flake".into(),
                    flake.into(),
                    "-b".into(),
                    format!("backup-{stamp}-{pid}"),
                ];
                self.push_impure(&mut command);
                command
            }
            // `Scope::Both` has no single command: the sequence activates one configuration per step.
            Scope::Both => unreachable!("no single command activates both scopes"),
        }
    }
}

/// The `json-log-path` a run's children get on top of the user's `NIX_CONFIG`, so events land in one log.
pub fn nix_config_for(log_path: &Path) -> String {
    nix_config(
        std::env::var("NIX_CONFIG").ok().as_deref(),
        &log_path.display().to_string(),
    )
}

pub fn nix_config(existing: Option<&str>, log_path: &str) -> String {
    let mut config = existing.unwrap_or_default().trim_end().to_owned();
    if !config.is_empty() {
        config.push('\n');
    }
    config.push_str(&format!("json-log-path = {log_path}"));
    config
}

fn tool(variable: &str, fallback: &str) -> String {
    std::env::var(variable).unwrap_or_else(|_| fallback.into())
}

#[cfg(test)]
impl ToolPaths {
    /// A resolved `ToolPaths`, as a machine with every tool on `PATH` would leave it.
    pub fn fake() -> Self {
        Self {
            nixos_rebuild: "nixos-rebuild".into(),
            home_manager: "home-manager".into(),
            nix_env: "nix-env".into(),
            nix_collect_garbage: "nix-collect-garbage".into(),
            impure: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::scope::Scope;

    #[test]
    fn home_rebuild_uses_a_unique_backup_extension() {
        let tools = ToolPaths::fake();
        let command =
            tools.rebuild_command_at(Scope::Home, "/repo#sample", false, "20260920-134343", 4242);
        assert!(
            command
                .windows(2)
                .any(|pair| pair == ["-b", "backup-20260920-134343-4242"]),
            "the backup extension is unique per run, got {command:?}",
        );
    }

    #[test]
    fn system_rebuild_never_requests_a_backup_extension() {
        let tools = ToolPaths::fake();
        let command =
            tools.rebuild_command_at(Scope::System, "/repo#system", true, "20260920-134343", 1);
        assert_eq!(command[1], "boot");
        assert!(
            !command.iter().any(|arg| arg == "-b"),
            "only Home Manager takes a backup extension",
        );
    }

    #[test]
    fn production_backup_extension_is_unique_per_process() {
        let tools = ToolPaths::fake();
        let first = tools.rebuild_command(Scope::Home, "/repo#sample", false);
        let backup = |command: &[String]| {
            command
                .windows(2)
                .find(|pair| pair[0] == "-b")
                .map(|pair| pair[1].clone())
                .expect("backup extension")
        };
        let extension = backup(&first);
        assert!(
            extension.starts_with("backup-"),
            "the extension is namespaced, got {extension}",
        );
        assert!(
            extension.ends_with(&std::process::id().to_string()),
            "the extension carries the PID so two runs cannot collide, got {extension}",
        );
    }

    #[test]
    fn nix_config_preserves_existing_settings() {
        let config = super::nix_config(
            Some("experimental-features = nix-command flakes"),
            "/tmp/events.jsonl",
        );
        assert!(config.starts_with("experimental-features = nix-command flakes\n"));
        assert!(config.ends_with("json-log-path = /tmp/events.jsonl"));
    }

    #[test]
    fn an_impure_machine_asks_both_activations_to_evaluate_impurely() {
        let tools = super::ToolPaths {
            impure: true,
            ..ToolPaths::fake()
        };

        let system =
            tools.rebuild_command_at(Scope::System, "/repo#system", false, "20260920-134343", 1);
        assert_eq!(
            system.last().map(String::as_str),
            Some("--impure"),
            "the system activation produces the flag: {system:?}"
        );

        let home =
            tools.rebuild_command_at(Scope::Home, "/repo#sample", false, "20260920-134343", 1);
        assert_eq!(
            home.last().map(String::as_str),
            Some("--impure"),
            "and so does the home one, after the backup extension: {home:?}"
        );
    }

    #[test]
    fn a_machine_that_declares_nothing_passes_no_impure_flag() {
        let tools = ToolPaths::fake();
        for command in [
            tools.rebuild_command(Scope::System, "/repo#system", false),
            tools.rebuild_command(Scope::Home, "/repo#sample", false),
        ] {
            assert!(
                !command.iter().any(|arg| arg == "--impure"),
                "pure is what Nix already does: {command:?}"
            );
        }
    }
}
