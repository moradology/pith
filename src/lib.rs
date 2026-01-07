//! Pith - Generate optimized codebase context for LLMs.
//!
//! Pith walks directory trees, extracts API signatures ("codemaps") from source files,
//! and produces structured output suitable for feeding to language models.
//!
//! # Quick Start
//!
//! ```no_run
//! use pith::builder::Pith;
//! use pith::filter::Language;
//!
//! // Extract codemaps from a project
//! let result = Pith::new("./my-project")
//!     .languages(&[Language::Rust, Language::TypeScript])
//!     .include_docs(true)
//!     .build()
//!     .unwrap();
//!
//! println!("Found {} files with codemaps", result.codemaps.len());
//! println!("Total tokens: {}", result.total_tokens());
//! ```
//!
//! # Modules
//!
//! - [`tokens`] - Token counting for LLM context budgets
//! - [`filter`] - File filtering with blocklist/allowlist/heuristics
//! - [`tree`] - File tree representation and rendering
//! - [`walker`] - Directory traversal with gitignore support
//! - [`codemap`] - Tree-sitter based code extraction
//! - [`builder`] - Fluent API for extraction
//!
//! # Supported Languages
//!
//! - Rust (`.rs`)
//! - TypeScript (`.ts`, `.tsx`)
//! - JavaScript (`.js`, `.jsx`, `.mjs`, `.cjs`)
//! - Python (`.py`, `.pyi`)
//! - Go (`.go`)

pub mod tokens;
pub mod filter;
pub mod errors;
pub mod tree;
pub mod walker;
pub mod codemap;
pub mod output;
pub mod builder;

// Re-export key types at crate root for convenience
pub use builder::{Pith, PithResult};
pub use codemap::{Codemap, CodemapError, Declaration, Visibility, Location};
pub use errors::PithError;
pub use filter::{FilterError, Language};
pub use output::OutputError;
pub use tree::{FileNode, NodeKind, RenderOptions};
pub use tokens::{count_tokens, Encoding, TokenCounter};
pub use walker::WalkError;
