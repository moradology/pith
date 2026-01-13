//! Fluent builder API for pith.
//!
//! Provides both function composition and builder-style APIs
//! for extracting codemaps from codebases.

use std::path::{Path, PathBuf};

use rayon::prelude::*;

use crate::codemap::{Codemap, ExtractOptions, extract_codemap};
use crate::errors::PithError;
use crate::filter::{Language, FilterResult, should_process, passes_extension_filter};
use crate::tree::{FileNode, RenderOptions};
use crate::walker::{WalkOptions, build_tree, walk_with_options};

/// Builder for extracting codemaps from a codebase.
///
/// # Examples
///
/// ```no_run
/// use pith::builder::Pith;
///
/// let result = Pith::new("./project")
///     .languages(&[pith::filter::Language::Rust])
///     .include_docs(true)
///     .build();
/// ```
pub struct Pith {
    root: PathBuf,
    languages: Option<Vec<Language>>,
    include_docs: bool,
    include_private: bool,
    walk_options: WalkOptions,
}

impl Pith {
    /// Create a new builder for the given root path.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            languages: None,
            include_docs: false,
            include_private: true,
            walk_options: WalkOptions::default(),
        }
    }

    /// Filter to specific languages only.
    pub fn languages(mut self, langs: &[Language]) -> Self {
        self.languages = Some(langs.to_vec());
        self
    }

    /// Include doc comments in extraction.
    pub fn include_docs(mut self, include: bool) -> Self {
        self.include_docs = include;
        self
    }

    /// Include private items (default: true, capture all).
    pub fn include_private(mut self, include: bool) -> Self {
        self.include_private = include;
        self
    }

    /// Include hidden files.
    pub fn include_hidden(mut self, include: bool) -> Self {
        self.walk_options.include_hidden = include;
        self
    }

    /// Set maximum directory depth.
    pub fn max_depth(mut self, depth: usize) -> Self {
        self.walk_options.max_depth = Some(depth);
        self
    }

    /// Build and return extraction results.
    pub fn build(self) -> Result<PithResult, PithError> {
        // Build tree
        let tree = build_tree(&self.root)
            .map_err(PithError::Walk)?;

        // Extract codemaps in parallel
        let extract_options = ExtractOptions {
            include_docs: self.include_docs,
            include_private: self.include_private,
        };

        let codemaps = extract_codemaps_parallel(
            &self.root,
            &self.walk_options,
            &extract_options,
            self.languages.as_deref(),
        )?;

        Ok(PithResult { tree, codemaps })
    }

    /// Extract codemaps only (no tree).
    pub fn extract(self) -> Result<Vec<Codemap>, PithError> {
        let extract_options = ExtractOptions {
            include_docs: self.include_docs,
            include_private: self.include_private,
        };

        extract_codemaps_parallel(
            &self.root,
            &self.walk_options,
            &extract_options,
            self.languages.as_deref(),
        )
    }

    /// Build tree only (no codemaps).
    pub fn tree(self) -> Result<FileNode, PithError> {
        build_tree(&self.root).map_err(PithError::Walk)
    }
}

/// Result of a pith extraction.
#[derive(Debug)]
pub struct PithResult {
    /// File tree of the codebase.
    pub tree: FileNode,
    /// Extracted codemaps.
    pub codemaps: Vec<Codemap>,
}

impl PithResult {
    /// Get paths that have codemaps.
    pub fn codemap_paths(&self) -> impl Iterator<Item = &Path> {
        self.codemaps.iter().map(|c| c.path.as_path())
    }

    /// Get codemap for a specific path.
    pub fn codemap_for(&self, path: &Path) -> Option<&Codemap> {
        self.codemaps.iter().find(|c| c.path == path)
    }

    /// Total token count across all codemaps.
    pub fn total_tokens(&self) -> usize {
        self.codemaps.iter().map(|c| c.token_count).sum()
    }

    /// Build render options with codemap markers.
    pub fn render_options(&self) -> RenderOptions<'_> {
        RenderOptions {
            has_codemap: self.codemaps.iter().map(|c| &c.path).collect(),
            ..Default::default()
        }
    }
}

/// Extract codemaps from a directory in parallel.
fn extract_codemaps_parallel(
    root: &Path,
    walk_options: &WalkOptions,
    extract_options: &ExtractOptions,
    language_filter: Option<&[Language]>,
) -> Result<Vec<Codemap>, PithError> {
    // Collect files that pass filtering
    let files: Vec<(PathBuf, Language)> = walk_with_options(root, walk_options)
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.is_file)
        .filter_map(|entry| {
            // Check extension filter
            let lang = passes_extension_filter(&entry.path)?;

            // Apply language filter if specified
            if let Some(langs) = language_filter {
                if !langs.contains(&lang) {
                    return None;
                }
            }

            Some((entry.path, lang))
        })
        .collect();

    // Threshold for memory-mapping large files (5MB)
    const MMAP_THRESHOLD: u64 = 5_000_000;

    // Extract codemaps in parallel
    let codemaps: Vec<Codemap> = files
        .into_par_iter()
        .filter_map(|(path, lang)| {
            use std::io::Read;

            // Get file metadata for size-based optimizations
            let metadata = std::fs::metadata(&path).ok()?;
            let file_size = metadata.len();

            // Read first 1KB for heuristics check
            let mut first_kb = [0u8; 1024];
            let n = {
                let mut file = std::fs::File::open(&path).ok()?;
                file.read(&mut first_kb).ok()?
            };

            // Apply content heuristics on first 1KB only
            match should_process(&path, Some(&first_kb[..n])) {
                FilterResult::Accept(_) => {}
                FilterResult::Reject(_) => return None,
            }

            // Read file content with size-based optimization
            let content = if file_size as usize <= n {
                // Small file: we already have it in the buffer
                String::from_utf8(first_kb[..n].to_vec()).ok()?
            } else if file_size > MMAP_THRESHOLD {
                // Large file: use memory mapping to avoid heap allocation
                use memmap2::Mmap;
                let file = std::fs::File::open(&path).ok()?;
                let mmap = unsafe { Mmap::map(&file).ok()? };
                std::str::from_utf8(&mmap).ok()?.to_string()
            } else {
                // Medium file: pre-allocate capacity to avoid reallocs
                let mut content = String::with_capacity(file_size as usize);
                let mut file = std::fs::File::open(&path).ok()?;
                file.read_to_string(&mut content).ok()?;
                content
            };

            // Extract codemap
            Some(extract_codemap(&path, &content, lang, extract_options))
        })
        .collect();

    Ok(codemaps)
}

// ============================================================================
// Functional API
// ============================================================================

/// Extract codemaps from a path using functional composition.
///
/// # Examples
///
/// ```no_run
/// use pith::builder::extract_from_path;
/// use pith::codemap::ExtractOptions;
///
/// let codemaps = extract_from_path("./project", &ExtractOptions::default()).unwrap();
/// for codemap in codemaps {
///     println!("{}: {} declarations", codemap.path.display(), codemap.declarations.len());
/// }
/// ```
pub fn extract_from_path(
    root: impl AsRef<Path>,
    options: &ExtractOptions,
) -> Result<Vec<Codemap>, PithError> {
    extract_codemaps_parallel(
        root.as_ref(),
        &WalkOptions::default(),
        options,
        None,
    )
}

/// Extract codemaps for specific languages.
pub fn extract_languages(
    root: impl AsRef<Path>,
    languages: &[Language],
    options: &ExtractOptions,
) -> Result<Vec<Codemap>, PithError> {
    extract_codemaps_parallel(
        root.as_ref(),
        &WalkOptions::default(),
        options,
        Some(languages),
    )
}

/// Build a file tree from a path.
pub fn tree_from_path(root: impl AsRef<Path>) -> Result<FileNode, PithError> {
    build_tree(root.as_ref()).map_err(PithError::Walk)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs;

    fn create_test_project() -> TempDir {
        let dir = TempDir::new().unwrap();

        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("src/main.rs"),
            r#"
pub fn main() {
    println!("Hello");
}
"#,
        ).unwrap();

        fs::write(
            dir.path().join("src/lib.rs"),
            r#"
pub struct Config {
    pub name: String,
}

pub fn process(config: &Config) -> String {
    config.name.clone()
}
"#,
        ).unwrap();

        dir
    }

    #[test]
    fn test_pith_builder() {
        let dir = create_test_project();

        let result = Pith::new(dir.path())
            .languages(&[Language::Rust])
            .build()
            .unwrap();

        assert!(result.tree.is_directory());
        assert!(!result.codemaps.is_empty());
    }

    #[test]
    fn test_extract_only() {
        let dir = create_test_project();

        let codemaps = Pith::new(dir.path())
            .extract()
            .unwrap();

        assert!(!codemaps.is_empty());
    }

    #[test]
    fn test_tree_only() {
        let dir = create_test_project();

        let tree = Pith::new(dir.path())
            .tree()
            .unwrap();

        assert!(tree.is_directory());
        assert!(tree.file_count() >= 2);
    }

    #[test]
    fn test_functional_api() {
        let dir = create_test_project();

        let codemaps = extract_from_path(dir.path(), &ExtractOptions::default()).unwrap();
        assert!(!codemaps.is_empty());

        let tree = tree_from_path(dir.path()).unwrap();
        assert!(tree.is_directory());
    }

    #[test]
    fn test_language_filter() {
        let dir = TempDir::new().unwrap();

        fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
        fs::write(dir.path().join("index.ts"), "export function foo() {}").unwrap();

        let rust_only = extract_languages(
            dir.path(),
            &[Language::Rust],
            &ExtractOptions::default(),
        ).unwrap();

        assert_eq!(rust_only.len(), 1);
        assert!(rust_only[0].path.ends_with("main.rs"));
    }
}
