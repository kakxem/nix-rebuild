//! What the machine declares about itself, carried by the wrapper as `NIX_REBUILD_*`.
//! Nothing is guessed: an undeclared name stops the run and names the option that declares it.

use std::env;

use anyhow::{Result, anyhow};

/// How Home Manager is installed, which decides what `nix-rebuild home` can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeManager {
    /// A `homeConfigurations` output of its own, activated by `home-manager`.
    Standalone,
    /// A profile inside the NixOS configuration; only system activation reaches it.
    Module,
    /// No Home Manager at all, not the same as none declared.
    None,
}

impl HomeManager {
    fn declared(value: &str) -> Option<Self> {
        match value {
            "standalone" => Some(Self::Standalone),
            "module" => Some(Self::Module),
            "none" => Some(Self::None),
            _ => None,
        }
    }

    /// The user-facing reason `home` rebuilds nothing here.
    pub fn why_home_rebuilds_nothing(self) -> &'static str {
        match self {
            Self::Standalone => "home rebuilds the home configuration",
            Self::Module => {
                "Home Manager is a profile inside the NixOS configuration on this machine, so \
                 `nix-rebuild system` is the scope that rebuilds it"
            }
            Self::None => {
                "this machine does not use Home Manager, so `nix-rebuild system` is what \
                 rebuilds it"
            }
        }
    }
}

/// The declarations `module.nix` carries, forwarded when a run re-executes itself.
pub const DECLARATIONS: &[&str] = &[
    "NIX_REBUILD_SYSTEM_CONFIG",
    "NIX_REBUILD_HOME_CONFIG",
    "NIX_REBUILD_HOME_MANAGER",
    "NIX_REBUILD_IMPURE",
    "NIX_REBUILD_TRUST_FLAKE_CONFIG",
];

/// The kind the machine declares, or an error when nothing is declared.
pub fn home_manager() -> Result<HomeManager> {
    let declared = declared("NIX_REBUILD_HOME_MANAGER").unwrap_or_default();
    HomeManager::declared(&declared).ok_or_else(|| {
        anyhow!(
            "no Home Manager kind is declared: set `home.manager` in the nix-rebuild module to \
             `standalone`, `module` or `none`, or set NIX_REBUILD_HOME_MANAGER to one of them"
        )
    })
}

/// The `nixosConfigurations` attribute the system scope activates.
pub fn system_config() -> Result<String> {
    let declared = declared("NIX_REBUILD_SYSTEM_CONFIG").ok_or_else(|| {
        anyhow!(
            "no system configuration is declared: set `system.config` in the nix-rebuild module, \
             or set NIX_REBUILD_SYSTEM_CONFIG"
        )
    })?;
    Ok(attribute_name(&declared, "nixosConfigurations."))
}

/// The `homeConfigurations` attribute the home scope activates.
pub fn home_config() -> Result<String> {
    let declared = declared("NIX_REBUILD_HOME_CONFIG").ok_or_else(|| {
        anyhow!(
            "no home configuration is declared: set `home.config` in the nix-rebuild module, or \
             set NIX_REBUILD_HOME_CONFIG"
        )
    })?;
    Ok(attribute_name(&declared, "homeConfigurations."))
}

fn attribute_name(value: &str, prefix: &str) -> String {
    let value = value.trim();
    let value = value.strip_prefix(prefix).unwrap_or(value).trim();
    let value = value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(value);
    value.to_owned()
}

/// Whether the machine declares its flake only evaluates impurely. Defaults to no,
/// so the flag is added only where the module's `impure` option declares it.
pub fn impure() -> bool {
    let declared = declared("NIX_REBUILD_IMPURE").unwrap_or_default();
    matches!(declared.trim(), "1" | "true")
}

/// Whether the module declares the flake's `nixConfig` accepted. Same reader
/// contract as `impure`: absent or off means untrusted, the Nix default.
pub fn trust_flake_config() -> bool {
    let declared = declared("NIX_REBUILD_TRUST_FLAKE_CONFIG").unwrap_or_default();
    matches!(declared.trim(), "1" | "true")
}

/// The variable's value; an empty one names nothing and counts as undeclared.
fn declared(variable: &str) -> Option<String> {
    env::var(variable).ok().filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests;
