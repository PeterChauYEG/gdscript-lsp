use std::path::Path;

/// Parsed contents of a `project.godot` file.
#[derive(Debug, Default)]
pub struct ProjectConfig {
    /// Autoload singletons: name → res:// path (without leading `*`)
    pub autoloads: Vec<(String, String)>,
    pub godot_version: Option<String>,
}

/// Parse a `project.godot` file from its text content.
#[must_use]
pub fn parse(content: &str) -> ProjectConfig {
    let mut config = ProjectConfig::default();
    let mut in_autoload = false;

    for line in content.lines() {
        let line = line.trim();

        if line.starts_with('[') && line.ends_with(']') {
            in_autoload = line == "[autoload]";
            continue;
        }

        if line.starts_with(';') || line.is_empty() {
            continue;
        }

        // Extract Godot version from config/features=PackedStringArray("4.x")
        if line.starts_with("config/features=") {
            if let Some(ver) = extract_version(line) {
                config.godot_version = Some(ver);
            }
            continue;
        }

        if in_autoload {
            if let Some((name, path)) = parse_autoload_line(line) {
                config.autoloads.push((name, path));
            }
        }
    }

    config
}

/// Find `project.godot` by walking up from `start`.
#[must_use]
pub fn find(start: &Path) -> Option<std::path::PathBuf> {
    let mut dir = start;
    loop {
        let candidate = dir.join("project.godot");
        if candidate.exists() {
            return Some(candidate);
        }
        dir = dir.parent()?;
    }
}

fn parse_autoload_line(line: &str) -> Option<(String, String)> {
    let (name, value) = line.split_once('=')?;
    let name = name.trim().to_owned();
    // Value is a quoted string, possibly prefixed with `*` (singleton marker)
    let value = value.trim().trim_matches('"');
    let path = value.trim_start_matches('*').to_owned();
    Some((name, path))
}

fn extract_version(line: &str) -> Option<String> {
    // config/features=PackedStringArray("4.3", ...)
    let start = line.find('"')? + 1;
    let rest = &line[start..];
    let end = rest.find('"')?;
    let version_str = &rest[..end];
    // Keep only major.minor
    let parts: Vec<&str> = version_str.split('.').take(2).collect();
    Some(parts.join("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"[application]
config/features=PackedStringArray("4.3", "Forward Plus")

[autoload]
GameManager="*res://autoloads/game_manager.gd"
AudioBus="res://autoloads/audio_bus.gd"
"#;

    #[test]
    fn parses_godot_version() {
        let cfg = parse(SAMPLE);
        assert_eq!(cfg.godot_version.as_deref(), Some("4.3"));
    }

    #[test]
    fn parses_autoloads_with_singleton_marker() {
        let cfg = parse(SAMPLE);
        let gm = cfg.autoloads.iter().find(|(n, _)| n == "GameManager");
        assert!(gm.is_some());
        // Strips the leading `*`
        assert_eq!(gm.unwrap().1, "res://autoloads/game_manager.gd");
    }

    #[test]
    fn parses_autoloads_without_singleton_marker() {
        let cfg = parse(SAMPLE);
        let ab = cfg.autoloads.iter().find(|(n, _)| n == "AudioBus");
        assert!(ab.is_some());
        assert_eq!(ab.unwrap().1, "res://autoloads/audio_bus.gd");
    }

    #[test]
    fn ignores_non_autoload_sections() {
        let src = "[application]\nsome_key=\"value\"\n";
        let cfg = parse(src);
        assert!(cfg.autoloads.is_empty());
    }

    #[test]
    fn version_minor_only() {
        let src = "config/features=PackedStringArray(\"4.2.2\")\n";
        let cfg = parse(src);
        // Keeps only major.minor
        assert_eq!(cfg.godot_version.as_deref(), Some("4.2"));
    }

    #[test]
    fn empty_content_gives_defaults() {
        let cfg = parse("");
        assert!(cfg.autoloads.is_empty());
        assert!(cfg.godot_version.is_none());
    }
}
