//! File tree representation and rendering.
//!
//! Provides types for representing directory structures and
//! functions for rendering them with box-drawing characters.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::filter::Language;

/// The type of a filesystem node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeKind {
    Directory,
    File {
        extension: Option<String>,
        size: u64,
        lines: Option<usize>,
    },
}

impl NodeKind {
    /// Check if this is a directory.
    pub fn is_directory(&self) -> bool {
        matches!(self, NodeKind::Directory)
    }

    /// Check if this is a file.
    pub fn is_file(&self) -> bool {
        matches!(self, NodeKind::File { .. })
    }
}

/// A node in the file tree.
#[derive(Debug, Clone)]
pub struct FileNode {
    /// File or directory name (not full path).
    pub name: String,
    /// Full path from root.
    pub path: PathBuf,
    /// Type of node (file or directory).
    pub kind: NodeKind,
    /// Child nodes (empty for files).
    children: Vec<FileNode>,
}

impl FileNode {
    /// Create a new directory node.
    pub fn directory(name: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
            kind: NodeKind::Directory,
            children: Vec::new(),
        }
    }

    /// Create a new file node.
    pub fn file(
        name: impl Into<String>,
        path: impl Into<PathBuf>,
        extension: Option<String>,
        size: u64,
        lines: Option<usize>,
    ) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
            kind: NodeKind::File {
                extension,
                size,
                lines,
            },
            children: Vec::new(),
        }
    }

    /// Check if this is a directory.
    pub fn is_directory(&self) -> bool {
        self.kind.is_directory()
    }

    /// Check if this is a file.
    pub fn is_file(&self) -> bool {
        self.kind.is_file()
    }

    /// Add a child node. Only valid for directories.
    pub fn add_child(&mut self, child: FileNode) {
        self.children.push(child);
    }

    /// Get child nodes.
    pub fn children(&self) -> &[FileNode] {
        &self.children
    }

    /// Sort children: directories first, then alphabetically.
    pub fn sort_children(&mut self) {
        self.children.sort_by(|a, b| {
            match (&a.kind, &b.kind) {
                (NodeKind::Directory, NodeKind::File { .. }) => Ordering::Less,
                (NodeKind::File { .. }, NodeKind::Directory) => Ordering::Greater,
                _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            }
        });

        // Recursively sort children's children
        for child in &mut self.children {
            child.sort_children();
        }
    }

    /// Get file extension if this is a file.
    pub fn extension(&self) -> Option<&str> {
        match &self.kind {
            NodeKind::File { extension, .. } => extension.as_deref(),
            NodeKind::Directory => None,
        }
    }

    /// Get file size if this is a file.
    pub fn size(&self) -> Option<u64> {
        match &self.kind {
            NodeKind::File { size, .. } => Some(*size),
            NodeKind::Directory => None,
        }
    }

    /// Count total files in this tree.
    pub fn file_count(&self) -> usize {
        match &self.kind {
            NodeKind::File { .. } => 1,
            NodeKind::Directory => self.children.iter().map(|c| c.file_count()).sum(),
        }
    }

    /// Count total directories in this tree.
    pub fn directory_count(&self) -> usize {
        match &self.kind {
            NodeKind::File { .. } => 0,
            NodeKind::Directory => {
                1 + self.children.iter().map(|c| c.directory_count()).sum::<usize>()
            }
        }
    }
}

/// Options for rendering the tree.
#[derive(Debug, Clone, Default)]
pub struct RenderOptions {
    /// Show file sizes.
    pub show_size: bool,
    /// Show line counts.
    pub show_lines: bool,
    /// Show detected language.
    pub show_language: bool,
    /// Paths that are selected (marked with *).
    pub selected: HashSet<PathBuf>,
    /// Paths that have codemaps (marked with +).
    pub has_codemap: HashSet<PathBuf>,
}

impl RenderOptions {
    /// Create options with all metadata enabled.
    pub fn with_metadata() -> Self {
        Self {
            show_size: true,
            show_lines: true,
            show_language: true,
            ..Default::default()
        }
    }

    /// Create minimal options (no metadata).
    pub fn minimal() -> Self {
        Self::default()
    }
}

/// Box-drawing characters for tree rendering.
const BRANCH: &str = "├── ";
const LAST_BRANCH: &str = "└── ";
const VERTICAL: &str = "│   ";
const SPACE: &str = "    ";

/// Render a file tree to a string with box-drawing characters.
///
/// # Examples
///
/// ```
/// use pith::tree::{FileNode, NodeKind, RenderOptions, render_tree};
///
/// let mut root = FileNode::directory("project", "project");
/// root.add_child(FileNode::file("main.rs", "project/main.rs", Some("rs".into()), 1024, Some(50)));
/// root.sort_children();
///
/// let output = render_tree(&root, &RenderOptions::minimal());
/// assert!(output.contains("main.rs"));
/// ```
pub fn render_tree(root: &FileNode, options: &RenderOptions) -> String {
    // Pre-allocate for typical tree size
    let mut output = String::with_capacity(4096);
    render_node(&mut output, root, "", true, true, options);
    output
}

fn render_node(
    output: &mut String,
    node: &FileNode,
    prefix: &str,
    is_last: bool,
    is_root: bool,
    options: &RenderOptions,
) {
    // Render this node
    let branch = if is_root {
        "" // Root node has no branch
    } else if is_last {
        LAST_BRANCH
    } else {
        BRANCH
    };

    output.push_str(prefix);
    output.push_str(branch);
    output.push_str(&node.name);

    // Add trailing slash for directories
    if node.is_directory() {
        output.push('/');
    }

    // Add metadata for files
    if let NodeKind::File {
        extension,
        size,
        lines,
    } = &node.kind
    {
        let mut metadata = Vec::new();

        if options.show_language {
            if let Some(ext) = extension {
                if let Ok(lang) = ext.parse::<Language>() {
                    metadata.push(lang.to_string());
                }
            }
        }

        if options.show_lines {
            if let Some(line_count) = lines {
                metadata.push(format!("{} lines", line_count));
            }
        }

        if options.show_size {
            metadata.push(format_size(*size));
        }

        if !metadata.is_empty() {
            output.push_str(" [");
            output.push_str(&metadata.join(", "));
            output.push(']');
        }
    }

    // Add markers
    let is_selected = options.selected.contains(&node.path);
    let has_codemap = options.has_codemap.contains(&node.path);

    if is_selected || has_codemap {
        output.push(' ');
        if is_selected {
            output.push('*');
        }
        if has_codemap {
            output.push('+');
        }
    }

    output.push('\n');

    // Render children
    let child_count = node.children.len();
    for (i, child) in node.children.iter().enumerate() {
        let is_last_child = i == child_count - 1;

        // Build new prefix for children
        let new_prefix = if is_root {
            // Root's children have no prefix before their branch
            String::new()
        } else {
            // Non-root: add continuation (vertical line or space) based on whether this node is last
            let continuation = if is_last { SPACE } else { VERTICAL };
            format!("{}{}", prefix, continuation)
        };

        render_node(output, child, &new_prefix, is_last_child, false, options);
    }
}

/// Format file size for display.
fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;

    if bytes < KB {
        format!("{}B", bytes)
    } else if bytes < MB {
        format!("{:.1}KB", bytes as f64 / KB as f64)
    } else {
        format!("{:.1}MB", bytes as f64 / MB as f64)
    }
}

/// Format number with thousands separators.
pub fn format_number(n: usize) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

/// Detect language from a path (convenience wrapper).
pub fn detect_language_from_path(path: &Path) -> Option<Language> {
    crate::filter::detect_language(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_directory_node() {
        let node = FileNode::directory("src", "project/src");
        assert!(node.is_directory());
        assert!(!node.is_file());
        assert_eq!(node.name, "src");
    }

    #[test]
    fn test_file_node() {
        let node = FileNode::file("main.rs", "project/main.rs", Some("rs".into()), 1024, Some(50));
        assert!(node.is_file());
        assert!(!node.is_directory());
        assert_eq!(node.extension(), Some("rs"));
        assert_eq!(node.size(), Some(1024));
    }

    #[test]
    fn test_add_child() {
        let mut dir = FileNode::directory("src", "src");
        dir.add_child(FileNode::file("lib.rs", "src/lib.rs", Some("rs".into()), 512, Some(25)));
        assert_eq!(dir.children.len(), 1);
    }

    #[test]
    fn test_sort_children() {
        let mut dir = FileNode::directory("src", "src");
        dir.add_child(FileNode::file("z.rs", "src/z.rs", Some("rs".into()), 100, Some(5)));
        dir.add_child(FileNode::directory("utils", "src/utils"));
        dir.add_child(FileNode::file("a.rs", "src/a.rs", Some("rs".into()), 100, Some(5)));

        dir.sort_children();

        // Directory should come first
        assert!(dir.children[0].is_directory());
        assert_eq!(dir.children[0].name, "utils");
        // Then files alphabetically
        assert_eq!(dir.children[1].name, "a.rs");
        assert_eq!(dir.children[2].name, "z.rs");
    }

    #[test]
    fn test_file_count() {
        let mut root = FileNode::directory("root", "root");
        root.add_child(FileNode::file("a.rs", "root/a.rs", Some("rs".into()), 100, Some(5)));

        let mut sub = FileNode::directory("sub", "root/sub");
        sub.add_child(FileNode::file("b.rs", "root/sub/b.rs", Some("rs".into()), 100, Some(5)));
        sub.add_child(FileNode::file("c.rs", "root/sub/c.rs", Some("rs".into()), 100, Some(5)));
        root.add_child(sub);

        assert_eq!(root.file_count(), 3);
    }

    #[test]
    fn test_render_simple() {
        let mut root = FileNode::directory("project", "project");
        root.add_child(FileNode::file(
            "main.rs",
            "project/main.rs",
            Some("rs".into()),
            1024,
            Some(50),
        ));
        root.sort_children();

        let output = render_tree(&root, &RenderOptions::minimal());
        assert!(output.contains("project/"));
        assert!(output.contains("main.rs"));
    }

    #[test]
    fn test_render_with_metadata() {
        let mut root = FileNode::directory("project", "project");
        root.add_child(FileNode::file(
            "main.rs",
            "project/main.rs",
            Some("rs".into()),
            2048,
            Some(100),
        ));
        root.sort_children();

        let options = RenderOptions {
            show_size: true,
            show_language: false,
            show_lines: false,
            ..Default::default()
        };

        let output = render_tree(&root, &options);
        assert!(output.contains("2.0KB"));
    }

    #[test]
    fn test_render_with_markers() {
        let mut root = FileNode::directory("project", "project");
        root.add_child(FileNode::file(
            "main.rs",
            "project/main.rs",
            Some("rs".into()),
            1024,
            Some(50),
        ));
        root.sort_children();

        let options = RenderOptions {
            selected: [PathBuf::from("project/main.rs")].into_iter().collect(),
            has_codemap: [PathBuf::from("project/main.rs")].into_iter().collect(),
            ..Default::default()
        };

        let output = render_tree(&root, &options);
        assert!(output.contains("*+"));
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0B");
        assert_eq!(format_size(512), "512B");
        assert_eq!(format_size(1024), "1.0KB");
        assert_eq!(format_size(1536), "1.5KB");
        assert_eq!(format_size(1024 * 1024), "1.0MB");
    }

    #[test]
    fn test_format_number() {
        assert_eq!(format_number(0), "0");
        assert_eq!(format_number(999), "999");
        assert_eq!(format_number(1000), "1,000");
        assert_eq!(format_number(1234567), "1,234,567");
    }

    #[test]
    fn test_render_nested() {
        let mut root = FileNode::directory("project", "project");

        let mut src = FileNode::directory("src", "project/src");
        src.add_child(FileNode::file(
            "main.rs",
            "project/src/main.rs",
            Some("rs".into()),
            100,
            Some(5),
        ));
        src.add_child(FileNode::file(
            "lib.rs",
            "project/src/lib.rs",
            Some("rs".into()),
            200,
            Some(10),
        ));

        root.add_child(src);
        root.add_child(FileNode::file(
            "Cargo.toml",
            "project/Cargo.toml",
            Some("toml".into()),
            50,
            Some(3),
        ));
        root.sort_children();

        let output = render_tree(&root, &RenderOptions::minimal());

        // Verify structure
        assert!(output.contains("project/"));
        assert!(output.contains("src/"));
        assert!(output.contains("main.rs"));
        assert!(output.contains("lib.rs"));
        assert!(output.contains("Cargo.toml"));

        // Verify box drawing characters
        assert!(output.contains("├──") || output.contains("└──"));
    }
}
