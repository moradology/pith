//! Codemap extraction using tree-sitter.
//!
//! Extracts API signatures (functions, structs, types) from source files
//! without implementation bodies.

mod go;
mod javascript;
mod python;
mod rust;
mod typescript;

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use smallvec::SmallVec;
use tree_sitter::{Node, Parser};

// Thread-local parser caching to avoid re-initialization overhead.
//
// Important: no panics here. Parser initialization can fail (grammar load), and
// per `specs/errors.md` we keep library code panic-free.
thread_local! {
    static RUST_PARSER: RefCell<Option<Parser>> = const { RefCell::new(None) };
    static TS_PARSER: RefCell<Option<Parser>> = const { RefCell::new(None) };
    static TSX_PARSER: RefCell<Option<Parser>> = const { RefCell::new(None) };
    static PYTHON_PARSER: RefCell<Option<Parser>> = const { RefCell::new(None) };
    static GO_PARSER: RefCell<Option<Parser>> = const { RefCell::new(None) };
}

fn init_rust_parser() -> Result<Parser, ()> {
    let mut p = Parser::new();
    p.set_language(&tree_sitter_rust::LANGUAGE.into())
        .map_err(|_| ())?;
    Ok(p)
}

fn init_ts_parser() -> Result<Parser, ()> {
    let mut p = Parser::new();
    p.set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        .map_err(|_| ())?;
    Ok(p)
}

fn init_tsx_parser() -> Result<Parser, ()> {
    let mut p = Parser::new();
    p.set_language(&tree_sitter_typescript::LANGUAGE_TSX.into())
        .map_err(|_| ())?;
    Ok(p)
}

fn init_python_parser() -> Result<Parser, ()> {
    let mut p = Parser::new();
    p.set_language(&tree_sitter_python::LANGUAGE.into())
        .map_err(|_| ())?;
    Ok(p)
}

fn init_go_parser() -> Result<Parser, ()> {
    let mut p = Parser::new();
    p.set_language(&tree_sitter_go::LANGUAGE.into())
        .map_err(|_| ())?;
    Ok(p)
}

fn with_cached_parser<F, R>(
    cell: &'static std::thread::LocalKey<RefCell<Option<Parser>>>,
    init: fn() -> Result<Parser, ()>,
    f: F,
) -> Result<R, String>
where
    F: FnOnce(&mut Parser) -> R,
{
    cell.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            *slot = Some(init().map_err(|()| "failed to initialize parser".to_string())?);
        }

        let parser = slot
            .as_mut()
            .ok_or_else(|| "failed to initialize parser".to_string())?;
        Ok(f(parser))
    })
}

/// Execute a function with a cached Rust parser.
pub(crate) fn with_rust_parser<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&mut Parser) -> R,
{
    with_cached_parser(&RUST_PARSER, init_rust_parser, f)
}

/// Execute a function with a cached TypeScript parser.
pub(crate) fn with_ts_parser<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&mut Parser) -> R,
{
    with_cached_parser(&TS_PARSER, init_ts_parser, f)
}

/// Execute a function with a cached TSX parser.
pub(crate) fn with_tsx_parser<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&mut Parser) -> R,
{
    with_cached_parser(&TSX_PARSER, init_tsx_parser, f)
}

/// Execute a function with a cached Python parser.
pub(crate) fn with_python_parser<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&mut Parser) -> R,
{
    with_cached_parser(&PYTHON_PARSER, init_python_parser, f)
}

/// Execute a function with a cached Go parser.
pub(crate) fn with_go_parser<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&mut Parser) -> R,
{
    with_cached_parser(&GO_PARSER, init_go_parser, f)
}

/// Find a child node by kind.
pub(crate) fn find_child_by_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    node.children(&mut node.walk()).find(|c| c.kind() == kind)
}

/// Extract node text from content.
pub(crate) fn node_text(node: Node, content: &str) -> String {
    content[node.byte_range()].to_string()
}

use thiserror::Error;

use crate::filter::Language;

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
        Self {
            start_line,
            end_line,
        }
    }

    pub fn single_line(line: usize) -> Self {
        Self {
            start_line: line,
            end_line: line,
        }
    }
}

/// A field in a struct or class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub ty: String,
    pub visibility: Visibility,
}

/// A declaration extracted from source code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Declaration {
    /// A function or method declaration (Rust fn, Python def, Go func, JS/TS function).
    Function {
        name: String,
        signature: String,
        visibility: Visibility,
        location: Location,
        is_async: bool,
        doc: Option<String>,
    },
    /// A struct declaration with fields and optional methods (Rust struct, Go struct).
    Struct {
        name: String,
        fields: SmallVec<[Field; 6]>,
        visibility: Visibility,
        location: Location,
        methods: Vec<Declaration>, // Vec needed for recursive type
        doc: Option<String>,
    },
    /// An enum declaration with variants (Rust enum).
    Enum {
        name: String,
        variants: SmallVec<[String; 6]>,
        visibility: Visibility,
        location: Location,
        doc: Option<String>,
    },
    /// A trait declaration with method signatures (Rust trait).
    Trait {
        name: String,
        methods: SmallVec<[String; 8]>,
        visibility: Visibility,
        location: Location,
        doc: Option<String>,
    },
    /// A type alias (Rust type, Go type, TS type).
    TypeAlias {
        name: String,
        target: String,
        visibility: Visibility,
        location: Location,
    },
    /// A constant declaration (Rust const, Go const).
    Const {
        name: String,
        ty: String,
        visibility: Visibility,
        location: Location,
    },
    /// An interface declaration (Go interface, TS interface).
    Interface {
        name: String,
        members: SmallVec<[String; 8]>,
        visibility: Visibility,
        location: Location,
        doc: Option<String>,
    },
    /// A class declaration with members (Python class, JS/TS class).
    Class {
        name: String,
        members: Vec<Declaration>, // Vec needed for recursive type
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
            Declaration::Trait { visibility, .. } => *visibility,
            Declaration::TypeAlias { visibility, .. } => *visibility,
            Declaration::Const { visibility, .. } => *visibility,
            Declaration::Interface { visibility, .. } => *visibility,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    /// Module path (e.g., "std::collections" or "react").
    pub source: String,
    /// Imported items. Empty for wildcard or default imports.
    pub items: SmallVec<[String; 4]>,
}

/// Extracted codemap from a source file.
#[derive(Debug, Clone)]
pub struct Codemap {
    /// Path to the source file.
    pub path: PathBuf,
    /// Detected language.
    pub language: Language,
    /// Import statements.
    pub imports: SmallVec<[Import; 8]>,
    /// Extracted declarations.
    pub declarations: SmallVec<[Declaration; 16]>,
    /// Parse error if extraction failed.
    pub parse_error: Option<String>,
}

impl Codemap {
    /// Create an empty codemap for a file.
    pub fn empty(path: PathBuf, language: Language) -> Self {
        Self {
            path,
            language,
            imports: SmallVec::new(),
            declarations: SmallVec::new(),
            parse_error: None,
        }
    }

    /// Create a codemap with a parse error.
    pub fn with_error(path: PathBuf, language: Language, error: String) -> Self {
        Self {
            path,
            language,
            imports: SmallVec::new(),
            declarations: SmallVec::new(),
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
            codemap.imports = imports.into();
            codemap.declarations = declarations.into();
        }
        Err(e) => {
            codemap.parse_error = Some(e);
        }
    }

    codemap
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
