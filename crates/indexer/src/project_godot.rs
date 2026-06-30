use std::path::Path;

/// Parsed contents of a `project.godot` file.
#[derive(Debug, Default)]
pub struct ProjectConfig {
    pub autoloads: Vec<(String, String)>,
    pub godot_version: Option<String>,
}

/// Parse a `project.godot` file from its text content.
#[must_use]
pub fn parse(content: &str) -> ProjectConfig {
    // TODO(LAB-662): implement INI-style parser for autoloads and version
    let _ = content;
    ProjectConfig::default()
}

/// Find `project.godot` by walking up from the given directory.
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
