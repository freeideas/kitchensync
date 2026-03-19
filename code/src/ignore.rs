use globset::{Glob, GlobSet, GlobSetBuilder};

pub struct IgnoreRules {
    exclude_patterns: Vec<String>,
    include_patterns: Vec<String>,
    excludes: GlobSet,
    includes: GlobSet, // negation patterns (!)
}

const BUILTIN_EXCLUDES: &[&str] = &[".kitchensync"];
const DEFAULT_EXCLUDES: &[&str] = &[".git"];

impl IgnoreRules {
    pub fn new() -> Self {
        let empty: Vec<String> = Vec::new();
        Self {
            exclude_patterns: Vec::new(),
            include_patterns: Vec::new(),
            excludes: build_globset(&empty),
            includes: build_globset(&empty),
        }
    }

    pub fn from_content(content: &str, parent_rules: Option<&IgnoreRules>) -> Self {
        let mut exclude_patterns = Vec::new();
        let mut include_patterns = Vec::new();

        // Inherit parent patterns
        if let Some(parent) = parent_rules {
            exclude_patterns.extend(parent.exclude_patterns.iter().cloned());
            include_patterns.extend(parent.include_patterns.iter().cloned());
        } else {
            // Start with default excludes only if no parent
            for pat in DEFAULT_EXCLUDES {
                exclude_patterns.push(pat.to_string());
            }
        }

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(rest) = line.strip_prefix('!') {
                include_patterns.push(rest.to_string());
            } else {
                exclude_patterns.push(line.to_string());
            }
        }

        Self {
            excludes: build_globset(&exclude_patterns),
            includes: build_globset(&include_patterns),
            exclude_patterns,
            include_patterns,
        }
    }

    pub fn is_ignored(&self, name: &str, is_dir: bool) -> bool {
        // Built-in excludes: always ignored, cannot override
        for builtin in BUILTIN_EXCLUDES {
            if name == *builtin {
                return true;
            }
        }

        let check_name = if is_dir {
            format!("{}/", name)
        } else {
            name.to_string()
        };

        // Check negation first (! patterns override excludes)
        if self.includes.is_match(&check_name) || self.includes.is_match(name) {
            return false;
        }

        // Check excludes
        if self.excludes.is_match(&check_name) || self.excludes.is_match(name) {
            return true;
        }

        false
    }

    pub fn default_rules() -> Self {
        let mut exclude_patterns: Vec<String> = Vec::new();
        let empty: Vec<String> = Vec::new();
        for pat in DEFAULT_EXCLUDES {
            exclude_patterns.push(pat.to_string());
        }
        Self {
            excludes: build_globset(&exclude_patterns),
            includes: build_globset(&empty),
            exclude_patterns,
            include_patterns: Vec::new(),
        }
    }

    pub fn clone_rules(&self) -> IgnoreRules {
        IgnoreRules {
            exclude_patterns: self.exclude_patterns.clone(),
            include_patterns: self.include_patterns.clone(),
            excludes: build_globset(&self.exclude_patterns),
            includes: build_globset(&self.include_patterns),
        }
    }
}

fn build_globset(patterns: &[impl AsRef<str>]) -> GlobSet {
    let mut builder = GlobSetBuilder::new();
    for pat in patterns {
        let pat = pat.as_ref();
        // Handle directory patterns (trailing /)
        let pat_clean = pat.trim_end_matches('/');
        if let Ok(glob) = Glob::new(pat_clean) {
            builder.add(glob);
        }
        // Also match with **/ prefix for patterns without it
        if !pat_clean.starts_with("**/") {
            if let Ok(glob) = Glob::new(&format!("**/{}", pat_clean)) {
                builder.add(glob);
            }
        }
    }
    builder.build().unwrap_or_else(|_| GlobSetBuilder::new().build().unwrap())
}
