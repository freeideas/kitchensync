use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::path::Path;

/// Accumulated ignore rules from .syncignore files, with default exclusions.
#[derive(Clone)]
pub struct IgnoreRules {
    /// (source_dir, pattern) pairs, in order
    rules: Vec<(String, String)>,
}

impl IgnoreRules {
    /// Create default rules: .git/ is excluded by default.
    pub fn default_rules() -> Self {
        Self {
            rules: vec![(".".to_string(), ".git/".to_string())],
        }
    }

    /// Add rules from a .syncignore file at the given directory.
    pub fn with_syncignore(&self, dir: &str, content: &str) -> Self {
        let mut rules = self.rules.clone();
        let source = if dir.is_empty() { "." } else { dir };
        for line in content.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() && !trimmed.starts_with('#') {
                rules.push((source.to_string(), trimmed.to_string()));
            }
        }
        Self { rules }
    }

    /// Build a Gitignore matcher from accumulated rules.
    pub fn build(&self) -> Gitignore {
        let mut builder = GitignoreBuilder::new(".");
        for (source, line) in &self.rules {
            let _ = builder.add_line(Some(Path::new(source).to_path_buf()), line);
        }
        builder
            .build()
            .unwrap_or_else(|_| GitignoreBuilder::new(".").build().unwrap())
    }

    /// Check if a relative path should be ignored.
    pub fn is_ignored(&self, gitignore: &Gitignore, rel_path: &str, is_dir: bool) -> bool {
        gitignore
            .matched_path_or_any_parents(Path::new(rel_path), is_dir)
            .is_ignore()
    }
}
