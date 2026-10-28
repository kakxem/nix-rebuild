//! Parses `key = value` lines; the settings file and the recorded runs both use them.

use std::collections::BTreeMap;

/// Every `key = value` pair. Comments, blanks and lines without `=` are dropped; a quoted
/// value loses its quotes and keeps any `#` inside, an unquoted one is cut at `#`.
pub fn pairs(text: &str) -> BTreeMap<String, String> {
    let mut pairs = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim();
        let value = match value.strip_prefix('"') {
            // The last quote closes the value: a home configuration may carry its own.
            Some(rest) => rest.rsplit_once('"').map_or(rest, |(value, _)| value),
            None => value.split('#').next().unwrap_or_default().trim(),
        };
        pairs.insert(key, value.to_owned());
    }
    pairs
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_quoted_value_keeps_the_quotes_inside_it() {
        let values = super::pairs(
            "flake = \"/repo#homeConfigurations.\"sample\"\"\nstatus = \"ok\"\ncommented = value # note\n",
        );

        assert_eq!(
            values["flake"], "/repo#homeConfigurations.\"sample\"",
            "the last quote closes the value, not the first"
        );
        assert_eq!(values["status"], "ok");
        assert_eq!(
            values["commented"], "value",
            "an unquoted value is still cut at a comment"
        );
    }
}
