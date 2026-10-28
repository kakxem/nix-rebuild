use std::sync::LazyLock;

use regex::Regex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogKind {
    Info,
    Evaluation,
    Download,
    Build,
    Activation,
    Cleanup,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NixEvent {
    raw: String,
    summary: String,
    pub level: LogLevel,
}

impl NixEvent {
    pub fn from_output(line: &str) -> Self {
        let raw = strip_ansi(line.trim());
        let lower = raw.to_ascii_lowercase();
        let level = if lower.starts_with("error:") {
            LogLevel::Error
        } else if lower.starts_with("warning:") || is_degraded_session(&lower) {
            LogLevel::Warning
        } else {
            LogLevel::Info
        };
        let summary = normalize(&raw);
        Self {
            raw,
            summary,
            level,
        }
    }

    pub fn from_message(line: &str, level: i64) -> Self {
        let mut event = Self::from_output(line);
        event.level = match level {
            0 => LogLevel::Error,
            1 => LogLevel::Warning,
            _ => LogLevel::Info,
        };
        event
    }

    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// The same line at another severity, for the continuation lines of a Nix
    /// error block: only its first line carries the `error:` prefix.
    pub fn with_level(mut self, level: LogLevel) -> Self {
        self.level = level;
        self
    }

    pub fn summary(&self) -> &str {
        &self.summary
    }

    pub fn kind(&self) -> LogKind {
        match self.level {
            LogLevel::Error => LogKind::Error,
            LogLevel::Warning => LogKind::Warning,
            LogLevel::Info => {
                let message = self.summary.to_ascii_lowercase();
                if message.starts_with("evaluating")
                    || message.starts_with("querying info")
                    || message.starts_with("these derivations")
                {
                    LogKind::Evaluation
                } else if message.starts_with("downloading") || message.starts_with("copying") {
                    LogKind::Download
                } else if message.starts_with("building") {
                    LogKind::Build
                } else if message.starts_with("activating")
                    || message.contains("activation package")
                {
                    LogKind::Activation
                } else if message.starts_with("deleting")
                    || message.starts_with("cleanup:")
                    || message.starts_with("removing generation")
                    || message.starts_with("removing stale")
                    || message.starts_with("removing old generations")
                    || message.starts_with("finding garbage collector roots")
                    || cleanup_tally(&self.summary).is_some()
                {
                    LogKind::Cleanup
                } else {
                    LogKind::Info
                }
            }
        }
    }
}

/// Failed systemd unit during activation: Home Manager prints the degraded
/// block with no severity. `line` is pre-lowercased; `●` marks a failed unit.
fn is_degraded_session(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.contains("degraded")
        || trimmed.starts_with("failed services:")
        || (trimmed.starts_with('●') && trimmed.contains("failed"))
}

fn normalize(line: &str) -> String {
    if let Some(installable) = line
        .strip_prefix("evaluating derivation '")
        .and_then(|value| value.strip_suffix('\''))
        && let Some((_, attribute)) = installable.split_once('#')
    {
        return format!("evaluating  {}", attribute.replace('"', ""));
    }
    let copying = &*COPYING;
    if let Some(captures) = copying.captures(line) {
        let path = captures.get(1).map(|m| m.as_str()).unwrap_or_default();
        // A `«…»` argument is a flake input reference, not a path.
        let label = if path.starts_with('«') {
            flake_input_name(path).unwrap_or_else(|| compact_path(path))
        } else {
            compact_path(path)
        };
        return format!("copying  {label} → store");
    }
    if let Some(rest) = line.strip_prefix("copying path '")
        && let Some((path, _)) = rest.split_once("' from '")
    {
        return format!("downloading  {}", store_name(path));
    }
    if let Some(rest) = line.strip_prefix("building '")
        && let Some(path) = rest.strip_suffix("'...")
    {
        return format!("building  {}", store_name(path));
    }
    line.to_owned()
}

/// Short log name for a flake input, from Nix's `«…»` copy reference: owner/repo,
/// dropping the query and pin. Keeps a `»/sub-path` so copies of one input stay distinct.
fn flake_input_name(reference: &str) -> Option<String> {
    let (head, sub_path) = reference.split_once('»')?;
    let name = flake_reference_name(head.trim_start_matches('«'))?;
    match compact_sub_path(sub_path) {
        Some(sub_path) => Some(format!("{name}/{sub_path}")),
        None => Some(name),
    }
}

/// Owner and repo for a flake reference, the file name for a tarball.
fn flake_reference_name(reference: &str) -> Option<String> {
    let without_query = reference.split(['?', '#']).next().unwrap_or(reference);
    let mut segments: Vec<&str> = without_query
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    // A URL leads with its host; a shorthand reference starts at the owner.
    if segments.len() > 1 && segments[0].contains('.') {
        segments.remove(0);
    }
    // A pinned git input ends in the commit the lock file resolved.
    if segments.len() > 1 && is_commit(segments[segments.len() - 1]) {
        segments.pop();
    }
    let name = *segments.last()?;
    if segments.len() == 1 || is_archive(name) {
        return Some(name.to_owned());
    }
    Some(format!("{}/{}", segments[segments.len() - 2], name))
}

/// Deep sub-paths keep their last two segments so the file still reads.
fn compact_sub_path(sub_path: &str) -> Option<String> {
    let parts: Vec<&str> = sub_path
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    match parts.as_slice() {
        [] => None,
        [only] => Some((*only).to_owned()),
        _ => Some(format!(
            "…/{}/{}",
            parts[parts.len() - 2],
            parts[parts.len() - 1]
        )),
    }
}

fn is_commit(segment: &str) -> bool {
    segment.len() == 40 && segment.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_archive(name: &str) -> bool {
    [".tar.zst", ".tar.gz", ".tar.xz", ".tar.bz2", ".tgz", ".zip"]
        .iter()
        .any(|extension| name.ends_with(extension))
}

fn compact_path(path: &str) -> String {
    if path.contains("nixpkgs/") {
        let clean = path.split('?').next().unwrap_or(path);
        let original_tail = path.rsplit("/pkgs/").next().unwrap_or(clean);
        let parts: Vec<_> = original_tail
            .split('/')
            .filter(|part| !part.is_empty())
            .collect();
        if parts.len() >= 2 {
            return format!(
                "nixpkgs/…/{}/{}",
                parts[parts.len() - 2],
                parts[parts.len() - 1]
            );
        }
        return "nixpkgs source".into();
    }
    if path.starts_with("/nix/store/") {
        return store_name(path);
    }
    let parts: Vec<_> = path.split('/').filter(|part| !part.is_empty()).collect();
    match parts.as_slice() {
        [] => path.to_owned(),
        [only] => (*only).to_owned(),
        _ => format!("…/{}/{}", parts[parts.len() - 2], parts[parts.len() - 1]),
    }
}

pub fn store_name(path: &str) -> String {
    let base = path
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .trim_end_matches(".drv");
    if base.len() > 33 && base.as_bytes().get(32) == Some(&b'-') {
        base[33..].to_owned()
    } else {
        base.to_owned()
    }
}

/// The collector's closing tally: `847 store paths deleted, 3.9 GiB freed`.
/// Its numbers feed the Activity card.
pub fn cleanup_tally(line: &str) -> Option<(u64, String)> {
    let (count, rest) = line.split_once(" store path")?;
    if count.is_empty() || !count.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    // Nix always prints the plural, but a singular reads the same way.
    let rest = rest.strip_prefix('s').unwrap_or(rest);
    let freed = rest.strip_prefix(" deleted, ")?;
    let freed = freed.trim_end_matches('.').strip_suffix(" freed")?;
    Some((count.parse().ok()?, freed.to_owned()))
}

fn strip_ansi(text: &str) -> String {
    ANSI.replace_all(text, "").into_owned()
}

/// One compile per pattern, not one per line: a rebuild prints thousands.
static COPYING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^copying ['"](.+?)['"] to the store$"#).expect("copy regex"));

static ANSI: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\x1b\[[0-9;]*[A-Za-z]").expect("ansi regex"));

#[cfg(test)]
mod tests;
