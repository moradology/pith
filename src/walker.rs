//! Directory traversal with gitignore support.
//!
//! Uses the `ignore` crate to walk directories while respecting
//! .gitignore, .git/info/exclude, global gitignore, and .pithignore.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use thiserror::Error;

use crate::tree::FileNode;

/// Count lines in a file using streaming (8KB buffer) instead of loading entire file.
/// Much more memory-efficient for large files.
fn count_lines_streaming(path: &Path) -> Option<usize> {
    let file = std::fs::File::open(path).ok()?;
    let reader = BufReader::new(file);
    let mut count = 0;
    for line in reader.lines() {
        if line.is_err() {
            // Binary or non-UTF8 file, fall back to byte counting
            return std::fs::read(path)
                .ok()
                .map(|bytes| bytecount::count(&bytes, b'\n'));
        }
        count += 1;
    }
    Some(count)
}

/// Errors that can occur during directory walking.
#[derive(Debug, Error)]
pub enum WalkError {
    #[error("path not found: {path}")]
    NotFound { path: PathBuf },

    #[error("not a directory: {path}")]
    NotADirectory { path: PathBuf },

    #[error("permission denied: {path}")]
    PermissionDenied { path: PathBuf },

    #[error("IO error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("symlink loop detected: {path}")]
    SymlinkLoop { path: PathBuf },
}

/// Options for directory walking.
#[derive(Debug, Clone)]
pub struct WalkOptions {
    /// Maximum depth to recurse (None = unlimited).
    pub max_depth: Option<usize>,
    /// Follow symbolic links.
    pub follow_symlinks: bool,
    /// Include hidden files and directories.
    pub include_hidden: bool,
    /// Respect .gitignore patterns.
    pub respect_gitignore: bool,
    /// Additional ignore file paths (e.g., .pithignore).
    pub custom_ignores: Vec<PathBuf>,
}

impl Default for WalkOptions {
    fn default() -> Self {
        Self {
            max_depth: None,
            follow_symlinks: false,
            include_hidden: false,
            respect_gitignore: true,
            custom_ignores: Vec::new(),
        }
    }
}

impl WalkOptions {
    /// Create options that include hidden files.
    pub fn with_hidden() -> Self {
        Self {
            include_hidden: true,
            ..Default::default()
        }
    }

    /// Set maximum depth.
    pub fn max_depth(mut self, depth: usize) -> Self {
        self.max_depth = Some(depth);
        self
    }
}

/// Entry from directory walk.
#[derive(Debug, Clone)]
pub struct WalkEntry {
    /// Path to the entry.
    pub path: PathBuf,
    /// Depth from root (root = 0).
    pub depth: usize,
    /// Whether this is a file or directory.
    pub is_file: bool,
    /// File size in bytes (only for files).
    pub size: Option<u64>,
}

/// Walk a directory tree, yielding entries.
///
/// Respects .gitignore and .pithignore patterns automatically.
///
/// # Examples
///
/// ```no_run
/// use pith::walker::walk;
/// use std::path::Path;
///
/// for entry in walk(Path::new(".")).flatten() {
///     println!("{}", entry.path.display());
/// }
/// ```
pub fn walk(root: &Path) -> impl Iterator<Item = Result<WalkEntry, WalkError>> {
    walk_with_options(root, &WalkOptions::default())
}

/// Walk a directory tree with custom options.
pub fn walk_with_options(
    root: &Path,
    options: &WalkOptions,
) -> impl Iterator<Item = Result<WalkEntry, WalkError>> {
    let root = root.to_path_buf();

    // Check if root exists
    if !root.exists() {
        return itertools_lite::Either::Left(std::iter::once(Err(WalkError::NotFound {
            path: root,
        })));
    }

    // Build the walker
    let mut builder = WalkBuilder::new(&root);

    builder
        .hidden(!options.include_hidden)
        .git_ignore(options.respect_gitignore)
        .git_global(options.respect_gitignore)
        .git_exclude(options.respect_gitignore)
        .follow_links(options.follow_symlinks);

    if let Some(depth) = options.max_depth {
        builder.max_depth(Some(depth));
    }

    // Add custom ignore files
    for ignore_path in &options.custom_ignores {
        if ignore_path.exists() {
            builder.add_ignore(ignore_path);
        }
    }

    // Look for .pithignore in root
    let pithignore = root.join(".pithignore");
    if pithignore.exists() {
        builder.add_ignore(&pithignore);
    }

    let walker = builder.build();

    itertools_lite::Either::Right(walker.filter_map(move |result| {
        match result {
            Ok(entry) => {
                let path = entry.path().to_path_buf();
                let depth = entry.depth();
                let is_file = entry.file_type().is_some_and(|ft| ft.is_file());

                let size = if is_file {
                    entry.metadata().ok().map(|m| m.len())
                } else {
                    None
                };

                Some(Ok(WalkEntry {
                    path,
                    depth,
                    is_file,
                    size,
                }))
            }
            Err(e) => {
                // Convert ignore errors to our error type
                match e {
                    ignore::Error::Io(io_err) => {
                        let path = PathBuf::from("<walk error>");
                        if io_err.kind() == std::io::ErrorKind::PermissionDenied {
                            Some(Err(WalkError::PermissionDenied { path }))
                        } else {
                            Some(Err(WalkError::Io {
                                path,
                                source: io_err,
                            }))
                        }
                    }
                    // Skip non-IO errors (like gitignore parse errors)
                    _ => None,
                }
            }
        }
    }))
}

/// Build a complete file tree from a directory.
///
/// This loads the entire tree into memory. For large directories,
/// prefer using `walk()` with streaming processing.
///
/// # Examples
///
/// ```no_run
/// use pith::walker::build_tree;
/// use std::path::Path;
///
/// let tree = build_tree(Path::new("./project")).unwrap();
/// println!("Files: {}", tree.file_count());
/// ```
pub fn build_tree(root: &Path) -> Result<FileNode, WalkError> {
    build_tree_with_options(root, &WalkOptions::default())
}

/// Build a complete file tree with custom options.
pub fn build_tree_with_options(root: &Path, options: &WalkOptions) -> Result<FileNode, WalkError> {
    if !root.exists() {
        return Err(WalkError::NotFound {
            path: root.to_path_buf(),
        });
    }

    let metadata = root.metadata().map_err(|e| WalkError::Io {
        path: root.to_path_buf(),
        source: e,
    })?;

    let name = root.file_name().map_or_else(
        || root.to_string_lossy().into_owned(),
        |n| n.to_string_lossy().into_owned(),
    );

    if metadata.is_file() {
        let extension = root
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_lowercase());

        // Count lines using streaming (8KB buffer instead of loading entire file)
        let lines = count_lines_streaming(root);

        return Ok(FileNode::file(
            name,
            root.to_path_buf(),
            extension,
            metadata.len(),
            lines,
        ));
    }

    // It's a directory - walk and build tree
    let mut node_map: std::collections::HashMap<PathBuf, FileNode> =
        std::collections::HashMap::new();

    // Create root node
    let root_node = FileNode::directory(&name, root.to_path_buf());
    node_map.insert(root.to_path_buf(), root_node);

    // Collect entries (skipping the root itself)
    let mut entries: Vec<WalkEntry> = walk_with_options(root, options)
        .filter_map(|r| r.ok())
        .filter(|e| e.path != root)
        .collect();

    // Build nodes for all entries
    for entry in &entries {
        let entry_name = entry
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        let node = if entry.is_file {
            let extension = entry
                .path
                .extension()
                .and_then(|e| e.to_str())
                .map(|s| s.to_lowercase());

            // Count lines using streaming (8KB buffer instead of loading entire file)
            let lines = count_lines_streaming(&entry.path);

            FileNode::file(
                &entry_name,
                &entry.path,
                extension,
                entry.size.unwrap_or(0),
                lines,
            )
        } else {
            FileNode::directory(&entry_name, &entry.path)
        };

        node_map.insert(entry.path.clone(), node);
    }

    // Build parent-child relationships
    // Process in order of depth (deepest first) so children are added before parents are moved
    entries.sort_by(|a, b| b.depth.cmp(&a.depth));

    for entry in &entries {
        if let Some(parent_path) = entry.path.parent() {
            let parent_path = parent_path.to_path_buf();
            if let Some(child) = node_map.remove(&entry.path) {
                if let Some(parent) = node_map.get_mut(&parent_path) {
                    parent.add_child(child);
                }
            }
        }
    }

    // Get the root and sort
    let mut result = node_map
        .remove(&root.to_path_buf())
        .ok_or_else(|| WalkError::Io {
            path: root.to_path_buf(),
            source: std::io::Error::other("failed to build tree"),
        })?;

    result.sort_children();
    Ok(result)
}

/// Simple Either type to avoid adding itertools dependency.
mod itertools_lite {
    pub enum Either<L, R> {
        Left(L),
        Right(R),
    }

    impl<L, R, T> Iterator for Either<L, R>
    where
        L: Iterator<Item = T>,
        R: Iterator<Item = T>,
    {
        type Item = T;

        fn next(&mut self) -> Option<Self::Item> {
            match self {
                Either::Left(l) => l.next(),
                Either::Right(r) => r.next(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_dir() -> TempDir {
        let dir = TempDir::new().unwrap();

        // Create structure
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(dir.path().join("src/lib.rs"), "pub fn hello() {}").unwrap();
        fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();

        dir
    }

    #[test]
    fn test_walk_basic() {
        let dir = create_test_dir();

        let entries: Vec<_> = walk(dir.path()).filter_map(|r| r.ok()).collect();

        // Should have root, src dir, and 3 files
        assert!(entries.len() >= 4);

        // Check we got the expected files
        let paths: Vec<_> = entries.iter().map(|e| e.path.clone()).collect();
        assert!(paths.iter().any(|p| p.ends_with("main.rs")));
        assert!(paths.iter().any(|p| p.ends_with("lib.rs")));
        assert!(paths.iter().any(|p| p.ends_with("Cargo.toml")));
    }

    #[test]
    fn test_walk_nonexistent() {
        let result: Vec<_> = walk(Path::new("/nonexistent/path")).collect();
        assert_eq!(result.len(), 1);
        assert!(matches!(result[0], Err(WalkError::NotFound { .. })));
    }

    #[test]
    fn test_walk_respects_gitignore() {
        let dir = TempDir::new().unwrap();

        // Initialize git repo (ignore crate needs this to respect .gitignore)
        fs::create_dir(dir.path().join(".git")).unwrap();

        // Create files
        fs::write(dir.path().join("visible.rs"), "// visible").unwrap();
        fs::write(dir.path().join("hidden.log"), "// hidden").unwrap();
        fs::write(dir.path().join(".gitignore"), "*.log").unwrap();

        let entries: Vec<_> = walk(dir.path()).filter_map(|r| r.ok()).collect();
        let paths: Vec<_> = entries.iter().map(|e| &e.path).collect();

        assert!(paths.iter().any(|p| p.ends_with("visible.rs")));
        assert!(!paths.iter().any(|p| p.ends_with("hidden.log")));
    }

    #[test]
    fn test_walk_respects_pithignore() {
        let dir = TempDir::new().unwrap();

        // Create files
        fs::write(dir.path().join("keep.rs"), "// keep").unwrap();
        fs::write(dir.path().join("skip.rs"), "// skip").unwrap();
        fs::write(dir.path().join(".pithignore"), "skip.rs").unwrap();

        let entries: Vec<_> = walk(dir.path()).filter_map(|r| r.ok()).collect();
        let paths: Vec<_> = entries.iter().map(|e| &e.path).collect();

        assert!(paths.iter().any(|p| p.ends_with("keep.rs")));
        assert!(!paths.iter().any(|p| p.ends_with("skip.rs")));
    }

    #[test]
    fn test_walk_hidden_files() {
        let dir = TempDir::new().unwrap();

        fs::write(dir.path().join("visible.rs"), "// visible").unwrap();
        fs::write(dir.path().join(".hidden.rs"), "// hidden").unwrap();

        // Default: exclude hidden
        let entries: Vec<_> = walk(dir.path()).filter_map(|r| r.ok()).collect();
        let paths: Vec<_> = entries.iter().map(|e| &e.path).collect();
        assert!(!paths.iter().any(|p| p.ends_with(".hidden.rs")));

        // With hidden
        let entries: Vec<_> = walk_with_options(dir.path(), &WalkOptions::with_hidden())
            .filter_map(|r| r.ok())
            .collect();
        let paths: Vec<_> = entries.iter().map(|e| &e.path).collect();
        assert!(paths.iter().any(|p| p.ends_with(".hidden.rs")));
    }

    #[test]
    fn test_build_tree() {
        let dir = create_test_dir();

        let tree = build_tree(dir.path()).unwrap();

        assert!(tree.is_directory());
        assert_eq!(tree.file_count(), 3);
        assert!(tree.directory_count() >= 2); // root + src
    }

    #[test]
    fn test_build_tree_sorted() {
        let dir = TempDir::new().unwrap();

        // Create in non-alphabetical order
        fs::write(dir.path().join("z.rs"), "").unwrap();
        fs::create_dir(dir.path().join("a_dir")).unwrap();
        fs::write(dir.path().join("a.rs"), "").unwrap();

        let tree = build_tree(dir.path()).unwrap();

        // Directory should come first
        assert!(tree.children()[0].is_directory());
        assert_eq!(tree.children()[0].name, "a_dir");
        // Then files alphabetically
        assert_eq!(tree.children()[1].name, "a.rs");
        assert_eq!(tree.children()[2].name, "z.rs");
    }

    #[test]
    fn test_walk_max_depth() {
        let dir = TempDir::new().unwrap();

        fs::create_dir_all(dir.path().join("a/b/c")).unwrap();
        fs::write(dir.path().join("a/b/c/deep.rs"), "").unwrap();
        fs::write(dir.path().join("a/shallow.rs"), "").unwrap();

        let options = WalkOptions::default().max_depth(2);
        let entries: Vec<_> = walk_with_options(dir.path(), &options)
            .filter_map(|r| r.ok())
            .collect();

        let paths: Vec<_> = entries.iter().map(|e| &e.path).collect();
        assert!(paths.iter().any(|p| p.ends_with("shallow.rs")));
        assert!(!paths.iter().any(|p| p.ends_with("deep.rs")));
    }
}
