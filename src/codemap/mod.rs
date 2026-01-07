//! Codemap extraction using tree-sitter.
//!
//! Extracts API signatures (functions, structs, types) from source files
//! without implementation bodies.

mod rust;
mod typescript;
mod javascript;
mod python;
mod go;

use std::path::{Path, PathBuf};

use tree_sitter::Node;

/// Find a child node by kind.
pub(crate) fn find_child_by_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    node.children(&mut node.walk())
        .find(|c| c.kind() == kind)
}

/// Extract node text from content.
pub(crate) fn node_text(node: Node, content: &str) -> String {
    content[node.byte_range()].to_string()
}

use thiserror::Error;

use crate::filter::Language;
use crate::tokens::count_tokens;

/// Visibility of a declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Visibility {
    Public,
    #[default]
    Private,
    /// Rust pub(crate)
    Crate,
    /// Python _ prefix convention
    Protected,
}

impl std::fmt::Display for Visibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Visibility::Public => write!(f, "pub"),
            Visibility::Private => write!(f, "private"),
            Visibility::Crate => write!(f, "pub(crate)"),
            Visibility::Protected => write!(f, "protected"),
        }
    }
}

/// Source location of a declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Location {
    /// 1-indexed start line.
    pub start_line: usize,
    /// 1-indexed end line (inclusive).
    pub end_line: usize,
}

impl Location {
    pub fn new(start_line: usize, end_line: usize) -> Self {
        Self { start_line, end_line }
    }

    pub fn single_line(line: usize) -> Self {
        Self { start_line: line, end_line: line }
    }
}

/// A field in a struct or class.
#[derive(Debug, Clone)]
pub struct Field {
    pub name: String,
    pub ty: String,
    pub visibility: Visibility,
}

/// A declaration extracted from source code.
#[derive(Debug, Clone)]
pub enum Declaration {
    Function {
        name: String,
        signature: String,
        visibility: Visibility,
        location: Location,
        is_async: bool,
        doc: Option<String>,
    },
    Struct {
        name: String,
        fields: Vec<Field>,
        visibility: Visibility,
        location: Location,
        methods: Vec<Declaration>,
        doc: Option<String>,
    },
    Enum {
        name: String,
        variants: Vec<String>,
        visibility: Visibility,
        location: Location,
        doc: Option<String>,
    },
    Trait {
        name: String,
        methods: Vec<String>,
        location: Location,
        doc: Option<String>,
    },
    TypeAlias {
        name: String,
        target: String,
        visibility: Visibility,
        location: Location,
    },
    Const {
        name: String,
        ty: String,
        visibility: Visibility,
        location: Location,
    },
    Interface {
        name: String,
        members: Vec<String>,
        location: Location,
        doc: Option<String>,
    },
    Class {
        name: String,
        members: Vec<Declaration>,
        visibility: Visibility,
        location: Location,
        doc: Option<String>,
    },
}

impl Declaration {
    /// Get the name of this declaration.
    pub fn name(&self) -> &str {
        match self {
            Declaration::Function { name, .. } => name,
            Declaration::Struct { name, .. } => name,
            Declaration::Enum { name, .. } => name,
            Declaration::Trait { name, .. } => name,
            Declaration::TypeAlias { name, .. } => name,
            Declaration::Const { name, .. } => name,
            Declaration::Interface { name, .. } => name,
            Declaration::Class { name, .. } => name,
        }
    }

    /// Get the visibility of this declaration.
    pub fn visibility(&self) -> Visibility {
        match self {
            Declaration::Function { visibility, .. } => *visibility,
            Declaration::Struct { visibility, .. } => *visibility,
            Declaration::Enum { visibility, .. } => *visibility,
            Declaration::Trait { .. } => Visibility::Public, // Traits are always public in their context
            Declaration::TypeAlias { visibility, .. } => *visibility,
            Declaration::Const { visibility, .. } => *visibility,
            Declaration::Interface { .. } => Visibility::Public,
            Declaration::Class { visibility, .. } => *visibility,
        }
    }

    /// Get the location of this declaration.
    pub fn location(&self) -> Location {
        match self {
            Declaration::Function { location, .. } => *location,
            Declaration::Struct { location, .. } => *location,
            Declaration::Enum { location, .. } => *location,
            Declaration::Trait { location, .. } => *location,
            Declaration::TypeAlias { location, .. } => *location,
            Declaration::Const { location, .. } => *location,
            Declaration::Interface { location, .. } => *location,
            Declaration::Class { location, .. } => *location,
        }
    }

    /// Check if this is public.
    pub fn is_public(&self) -> bool {
        matches!(self.visibility(), Visibility::Public)
    }
}

/// An import statement.
#[derive(Debug, Clone)]
pub struct Import {
    /// Module path (e.g., "std::collections" or "react").
    pub source: String,
    /// Imported items. Empty for wildcard or default imports.
    pub items: Vec<String>,
}

/// Extracted codemap from a source file.
#[derive(Debug, Clone)]
pub struct Codemap {
    /// Path to the source file.
    pub path: PathBuf,
    /// Detected language.
    pub language: Language,
    /// Import statements.
    pub imports: Vec<Import>,
    /// Extracted declarations.
    pub declarations: Vec<Declaration>,
    /// Token count of rendered codemap.
    pub token_count: usize,
    /// Parse error if extraction failed.
    pub parse_error: Option<String>,
}

impl Codemap {
    /// Create an empty codemap for a file.
    pub fn empty(path: PathBuf, language: Language) -> Self {
        Self {
            path,
            language,
            imports: Vec::new(),
            declarations: Vec::new(),
            token_count: 0,
            parse_error: None,
        }
    }

    /// Create a codemap with a parse error.
    pub fn with_error(path: PathBuf, language: Language, error: String) -> Self {
        Self {
            path,
            language,
            imports: Vec::new(),
            declarations: Vec::new(),
            token_count: 0,
            parse_error: Some(error),
        }
    }

    /// Filter to only public declarations.
    pub fn public_only(&self) -> impl Iterator<Item = &Declaration> {
        self.declarations.iter().filter(|d| d.is_public())
    }

    /// Count total declarations including nested.
    pub fn declaration_count(&self) -> usize {
        fn count_nested(decl: &Declaration) -> usize {
            let nested = match decl {
                Declaration::Struct { methods, .. } => methods.iter().map(count_nested).sum(),
                Declaration::Class { members, .. } => members.iter().map(count_nested).sum(),
                _ => 0,
            };
            1 + nested
        }
        self.declarations.iter().map(count_nested).sum()
    }
}

/// Options for codemap extraction.
#[derive(Debug, Clone, Default)]
pub struct ExtractOptions {
    /// Include doc comments.
    pub include_docs: bool,
    /// Include private items.
    pub include_private: bool,
}

impl ExtractOptions {
    pub fn with_docs() -> Self {
        Self {
            include_docs: true,
            include_private: true,
        }
    }

    pub fn public_only() -> Self {
        Self {
            include_docs: false,
            include_private: false,
        }
    }
}

/// Errors during codemap extraction.
#[derive(Debug, Error)]
pub enum CodemapError {
    #[error("failed to initialize {language} parser")]
    ParserInit { language: Language },

    #[error("parse error in {path}: {message}")]
    Parse { path: PathBuf, message: String },

    #[error("unsupported language for file: {path}")]
    UnsupportedLanguage { path: PathBuf },

    #[error("failed to read file: {path}")]
    ReadFailed {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Extract a codemap from a source file.
///
/// # Arguments
///
/// * `path` - Path to the source file
/// * `content` - File content as string
/// * `language` - Programming language of the file
/// * `options` - Extraction options
///
/// # Returns
///
/// A `Codemap` with extracted declarations. If parsing fails,
/// the codemap will have `parse_error` set but may still contain
/// partial results.
pub fn extract_codemap(
    path: &Path,
    content: &str,
    language: Language,
    options: &ExtractOptions,
) -> Codemap {
    let mut codemap = Codemap::empty(path.to_path_buf(), language);

    let result = match language {
        Language::Rust => rust::extract(content, options),
        Language::TypeScript | Language::Tsx => typescript::extract(content, language, options),
        Language::JavaScript | Language::Jsx => javascript::extract(content, language, options),
        Language::Python => python::extract(content, options),
        Language::Go => go::extract(content, options),
    };

    match result {
        Ok((imports, declarations)) => {
            codemap.imports = imports;
            codemap.declarations = declarations;
        }
        Err(e) => {
            codemap.parse_error = Some(e);
        }
    }

    // Calculate token count from a simple render
    let rendered = render_codemap_simple(&codemap);
    codemap.token_count = count_tokens(&rendered);

    codemap
}

/// Simple render for token counting.
fn render_codemap_simple(codemap: &Codemap) -> String {
    let mut output = String::new();

    // Imports
    for import in &codemap.imports {
        output.push_str(&import.source);
        output.push('\n');
    }

    // Declarations
    for decl in &codemap.declarations {
        render_declaration_simple(&mut output, decl);
    }

    output
}

fn render_declaration_simple(output: &mut String, decl: &Declaration) {
    match decl {
        Declaration::Function { signature, .. } => {
            output.push_str(signature);
            output.push('\n');
        }
        Declaration::Struct { name, fields, methods, .. } => {
            output.push_str("struct ");
            output.push_str(name);
            output.push_str(" { ");
            for field in fields {
                output.push_str(&field.name);
                output.push_str(": ");
                output.push_str(&field.ty);
                output.push_str(", ");
            }
            output.push_str("}\n");
            for method in methods {
                render_declaration_simple(output, method);
            }
        }
        Declaration::Enum { name, variants, .. } => {
            output.push_str("enum ");
            output.push_str(name);
            output.push_str(" { ");
            output.push_str(&variants.join(", "));
            output.push_str(" }\n");
        }
        Declaration::Trait { name, methods, .. } => {
            output.push_str("trait ");
            output.push_str(name);
            output.push_str(" { ");
            output.push_str(&methods.join("; "));
            output.push_str(" }\n");
        }
        Declaration::TypeAlias { name, target, .. } => {
            output.push_str("type ");
            output.push_str(name);
            output.push_str(" = ");
            output.push_str(target);
            output.push('\n');
        }
        Declaration::Const { name, ty, .. } => {
            output.push_str("const ");
            output.push_str(name);
            output.push_str(": ");
            output.push_str(ty);
            output.push('\n');
        }
        Declaration::Interface { name, members, .. } => {
            output.push_str("interface ");
            output.push_str(name);
            output.push_str(" { ");
            output.push_str(&members.join("; "));
            output.push_str(" }\n");
        }
        Declaration::Class { name, members, .. } => {
            output.push_str("class ");
            output.push_str(name);
            output.push_str(" {\n");
            for member in members {
                render_declaration_simple(output, member);
            }
            output.push_str("}\n");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_codemap() {
        let codemap = Codemap::empty(PathBuf::from("test.rs"), Language::Rust);
        assert!(codemap.imports.is_empty());
        assert!(codemap.declarations.is_empty());
        assert!(codemap.parse_error.is_none());
    }

    #[test]
    fn test_codemap_with_error() {
        let codemap = Codemap::with_error(
            PathBuf::from("test.rs"),
            Language::Rust,
            "parse error".into(),
        );
        assert!(codemap.parse_error.is_some());
    }

    #[test]
    fn test_declaration_name() {
        let func = Declaration::Function {
            name: "test".into(),
            signature: "fn test()".into(),
            visibility: Visibility::Public,
            location: Location::single_line(1),
            is_async: false,
            doc: None,
        };
        assert_eq!(func.name(), "test");
    }

    #[test]
    fn test_visibility_display() {
        assert_eq!(Visibility::Public.to_string(), "pub");
        assert_eq!(Visibility::Private.to_string(), "private");
        assert_eq!(Visibility::Crate.to_string(), "pub(crate)");
    }
}
