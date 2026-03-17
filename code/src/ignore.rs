use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::path::Path;
use std::sync::Arc;
use walkdir::DirEntry;

#[derive(Clone)]
pub struct IgnoreMatcher {
    root: std::path::PathBuf,
    // We store the root ignore matcher and let it cascade
    root_ignorer: Option<Arc<Gitignore>>,
}

impl IgnoreMatcher {
    pub fn new(root: &Path) -> Self {
        let syncignore_path = root.join(".syncignore");
        let root_ignorer = if syncignore_path.exists() {
            let mut builder = GitignoreBuilder::new(root);
            builder.add(&syncignore_path);
            builder.build().ok().map(Arc::new)
        } else {
            None
        };

        Self {
            root: root.to_path_buf(),
            root_ignorer,
        }
    }

    /// Check if a path should be ignored.
    /// Returns true if the path should be skipped.
    pub fn is_ignored(&self, path: &Path, is_dir: bool) -> bool {
        // Get relative path
        let rel_path = match path.strip_prefix(&self.root) {
            Ok(p) => p,
            Err(_) => return false,
        };

        let rel_str = rel_path.to_string_lossy();

        // Built-in excludes that cannot be overridden
        // .kitchensync/ directories
        if rel_str.starts_with(".kitchensync") || rel_str.contains("/.kitchensync") || rel_str.contains("\\.kitchensync") {
            return true;
        }

        // Check for .git/ (can be overridden)
        let is_git = rel_str == ".git" || rel_str.starts_with(".git/") || rel_str.starts_with(".git\\")
            || rel_str.contains("/.git/") || rel_str.contains("\\.git\\")
            || rel_str.contains("/.git") || rel_str.contains("\\.git");

        // Check .syncignore from root
        if let Some(ref ignorer) = self.root_ignorer {
            let matched = ignorer.matched_path_or_any_parents(path, is_dir);
            if matched.is_ignore() {
                return true;
            }
            // Check if .git/ is negated
            if is_git && matched.is_whitelist() {
                return false;
            }
        }

        // Default .git/ exclusion
        if is_git {
            return true;
        }

        // Check for .syncignore in parent directories
        let mut current = rel_path.parent();
        while let Some(parent) = current {
            let parent_full = self.root.join(parent);
            let syncignore = parent_full.join(".syncignore");
            if syncignore.exists() {
                let mut builder = GitignoreBuilder::new(&parent_full);
                builder.add(&syncignore);
                if let Ok(ignorer) = builder.build() {
                    let matched = ignorer.matched_path_or_any_parents(path, is_dir);
                    if matched.is_ignore() {
                        return true;
                    }
                }
            }
            current = parent.parent();
        }

        false
    }

    /// Check if a directory entry should be ignored.
    pub fn is_entry_ignored(&self, entry: &DirEntry) -> bool {
        let path = entry.path();
        let is_dir = entry.file_type().is_dir();

        // Skip symlinks
        if entry.file_type().is_symlink() {
            return true;
        }

        self.is_ignored(path, is_dir)
    }
}

pub fn build_ignore_matcher(root: &Path) -> Arc<IgnoreMatcher> {
    Arc::new(IgnoreMatcher::new(root))
}

/// Check if a path is a special file (device, FIFO, socket).
#[cfg(unix)]
pub fn is_special_file(path: &Path) -> bool {
    use std::os::unix::fs::FileTypeExt;
    if let Ok(metadata) = path.symlink_metadata() {
        let ft = metadata.file_type();
        return ft.is_block_device() || ft.is_char_device() || ft.is_fifo() || ft.is_socket();
    }
    false
}

#[cfg(windows)]
pub fn is_special_file(_path: &Path) -> bool {
    // Windows doesn't have the same special file types
    false
}
