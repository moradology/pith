//! Output formatting for pith.
//!
//! Formats file trees, codemaps, and selected files into
//! XML-style or JSON output suitable for LLM consumption.

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use serde::Serialize;
use thiserror::Error;

/// Errors that can occur during output formatting.
#[derive(Debug, Error)]
pub enum OutputError {
    #[error("serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

use crate::filter::Language;
use crate::tokens::count_tokens;
use crate::tree::{format_number, FileNode, NodeKind, RenderOptions, render_tree};
use crate::codemap::{Codemap, Declaration, Location, Visibility};

/// Output format selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    /// XML-style tags with markdown content (default).
    #[default]
    Xml,
    /// JSON for programmatic access.
    Json,
}

/// Options controlling what to include in output.
#[derive(Debug, Clone)]
pub struct OutputOptions {
    /// Output format.
    pub format: OutputFormat,
    /// Include file tree section.
    pub include_tree: bool,
    /// Include codemaps section.
    pub include_codemaps: bool,
    /// Include selected file contents.
    pub include_selected_files: bool,
    /// Include token summary.
    pub include_summary: bool,
    /// Only show public declarations.
    pub public_only: bool,
}

impl Default for OutputOptions {
    fn default() -> Self {
        Self {
            format: OutputFormat::Xml,
            include_tree: true,
            include_codemaps: true,
            include_selected_files: false,
            include_summary: true,
            public_only: true,
        }
    }
}

impl OutputOptions {
    /// Create options for full context output.
    pub fn full_context() -> Self {
        Self {
            include_selected_files: true,
            ..Default::default()
        }
    }

    /// Create options for tree-only output.
    pub fn tree_only() -> Self {
        Self {
            include_tree: true,
            include_codemaps: false,
            include_selected_files: false,
            include_summary: false,
            ..Default::default()
        }
    }

    /// Create options for codemap-only output.
    pub fn codemap_only() -> Self {
        Self {
            include_tree: false,
            include_codemaps: true,
            include_selected_files: false,
            include_summary: true,
            ..Default::default()
        }
    }
}

/// A selected file with its content.
#[derive(Debug, Clone)]
pub struct SelectedFile {
    pub path: PathBuf,
    pub content: String,
    pub lines: usize,
    pub tokens: usize,
}

/// Token breakdown for a file.
#[derive(Debug, Clone, Serialize)]
pub struct FileTokenInfo {
    pub tokens: usize,
    pub selected: bool,
    pub has_codemap: bool,
}

/// Summary of token usage.
#[derive(Debug, Clone)]
pub struct TokenSummary {
    pub total: usize,
    pub tree_tokens: usize,
    pub codemap_tokens: usize,
    pub selected_tokens: usize,
    pub file_breakdown: BTreeMap<PathBuf, FileTokenInfo>,
}

// ============================================================================
// Main Entry Point
// ============================================================================

/// Format complete output with all requested sections.
pub fn format_output(
    tree: Option<&FileNode>,
    codemaps: &[Codemap],
    selected_files: &[SelectedFile],
    options: &OutputOptions,
) -> String {
    match options.format {
        OutputFormat::Xml => format_output_xml(tree, codemaps, selected_files, options),
        OutputFormat::Json => format_output_json(tree, codemaps, selected_files, options),
    }
}

// ============================================================================
// XML Formatting
// ============================================================================

fn format_output_xml(
    tree: Option<&FileNode>,
    codemaps: &[Codemap],
    selected_files: &[SelectedFile],
    options: &OutputOptions,
) -> String {
    // Pre-allocate for typical output size
    let mut output = String::with_capacity(8192);

    // File tree section
    if options.include_tree {
        if let Some(tree) = tree {
            let selected: HashSet<PathBuf> = selected_files.iter().map(|f| f.path.clone()).collect();
            let has_codemap: HashSet<PathBuf> = codemaps.iter().map(|c| c.path.clone()).collect();

            let render_opts = RenderOptions {
                show_size: true,
                show_lines: true,
                show_language: true,
                selected,
                has_codemap,
            };

            output.push_str("<file_map>\n");
            output.push_str(&render_tree(tree, &render_opts));
            if !selected_files.is_empty() || !codemaps.is_empty() {
                output.push_str("\nLegend: * = selected, + = has codemap\n");
            }
            output.push_str("</file_map>\n\n");
        }
    }

    // Codemaps section
    if options.include_codemaps && !codemaps.is_empty() {
        output.push_str("<codemaps>\n");
        for (i, codemap) in codemaps.iter().enumerate() {
            if i > 0 {
                output.push_str("\n---\n\n");
            }
            output.push_str(&format_codemap_xml(codemap, options.public_only));
        }
        output.push_str("</codemaps>\n\n");
    }

    // Selected files section
    if options.include_selected_files && !selected_files.is_empty() {
        output.push_str("<selected_files>\n");
        for file in selected_files {
            output.push_str(&format!(
                "--- {} ({} lines, {} tokens) ---\n",
                file.path.display(),
                format_number(file.lines),
                format_number(file.tokens)
            ));
            output.push_str(&file.content);
            if !file.content.ends_with('\n') {
                output.push('\n');
            }
            output.push('\n');
        }
        output.push_str("</selected_files>\n\n");
    }

    // Token summary section
    if options.include_summary {
        let summary = calculate_summary(tree, codemaps, selected_files);
        output.push_str("<token_summary>\n");
        output.push_str(&format_summary_xml(&summary));
        output.push_str("</token_summary>\n");
    }

    output
}

fn format_codemap_xml(codemap: &Codemap, public_only: bool) -> String {
    let mut output = String::with_capacity(2048);

    // File header
    output.push_str(&format!("## {}\n\n", codemap.path.display()));

    // Parse error warning
    if let Some(ref error) = codemap.parse_error {
        output.push_str(&format!("**Parse error:** {}\n\n", error));
    }

    // Imports section
    if !codemap.imports.is_empty() {
        output.push_str("### Imports\n");
        for import in &codemap.imports {
            if import.items.is_empty() {
                output.push_str(&format!("- use {}\n", import.source));
            } else {
                output.push_str(&format!("- use {}::{{{}}}\n", import.source, import.items.join(", ")));
            }
        }
        output.push('\n');
    }

    // Declarations section
    let decls: Vec<_> = if public_only {
        codemap.declarations.iter().filter(|d| d.is_public()).collect()
    } else {
        codemap.declarations.iter().collect()
    };

    if !decls.is_empty() {
        output.push_str("### Declarations\n\n");
        for decl in decls {
            output.push_str(&format_declaration_xml(decl, public_only, 0));
        }
    }

    output
}

fn format_declaration_xml(decl: &Declaration, public_only: bool, indent: usize) -> String {
    let prefix = "    ".repeat(indent);
    let mut output = String::new();

    match decl {
        Declaration::Function {
            signature,
            location,
            doc,
            ..
        } => {
            output.push_str(&format!(
                "{}#### {} ({})\n",
                prefix,
                signature,
                format_location(location)
            ));
            if let Some(doc) = doc {
                output.push_str(&format!("{}{}\n", prefix, doc));
            }
            output.push('\n');
        }

        Declaration::Struct {
            name,
            fields,
            methods,
            location,
            doc,
            ..
        } => {
            output.push_str(&format!(
                "{}#### struct {} ({})\n",
                prefix,
                name,
                format_location(location)
            ));
            if let Some(doc) = doc {
                output.push_str(&format!("{}{}\n", prefix, doc));
            }

            // Fields
            let visible_fields: Vec<_> = if public_only {
                fields.iter().filter(|f| f.visibility == Visibility::Public).collect()
            } else {
                fields.iter().collect()
            };

            if !visible_fields.is_empty() {
                output.push_str(&format!("{}Fields:\n", prefix));
                for field in visible_fields {
                    let vis = if field.visibility == Visibility::Public { "pub " } else { "" };
                    output.push_str(&format!("{}- {}{}: {}\n", prefix, vis, field.name, field.ty));
                }
            }

            // Methods
            let visible_methods: Vec<_> = if public_only {
                methods.iter().filter(|m| m.is_public()).collect()
            } else {
                methods.iter().collect()
            };

            if !visible_methods.is_empty() {
                output.push_str(&format!("{}Methods:\n", prefix));
                for method in visible_methods {
                    if let Declaration::Function { signature, location, .. } = method {
                        output.push_str(&format!(
                            "{}- {} ({})\n",
                            prefix,
                            signature,
                            format_location(location)
                        ));
                    }
                }
            }
            output.push('\n');
        }

        Declaration::Enum {
            name,
            variants,
            location,
            doc,
            ..
        } => {
            output.push_str(&format!(
                "{}#### enum {} ({})\n",
                prefix,
                name,
                format_location(location)
            ));
            if let Some(doc) = doc {
                output.push_str(&format!("{}{}\n", prefix, doc));
            }
            output.push_str(&format!("{}Variants: {}\n\n", prefix, variants.join(", ")));
        }

        Declaration::Trait {
            name,
            methods,
            location,
            doc,
        } => {
            output.push_str(&format!(
                "{}#### trait {} ({})\n",
                prefix,
                name,
                format_location(location)
            ));
            if let Some(doc) = doc {
                output.push_str(&format!("{}{}\n", prefix, doc));
            }
            if !methods.is_empty() {
                output.push_str(&format!("{}Methods:\n", prefix));
                for method in methods {
                    output.push_str(&format!("{}- {}\n", prefix, method));
                }
            }
            output.push('\n');
        }

        Declaration::TypeAlias {
            name,
            target,
            location,
            ..
        } => {
            output.push_str(&format!(
                "{}#### type {} = {} ({})\n\n",
                prefix,
                name,
                target,
                format_location(location)
            ));
        }

        Declaration::Const {
            name,
            ty,
            location,
            ..
        } => {
            output.push_str(&format!(
                "{}#### const {}: {} ({})\n\n",
                prefix,
                name,
                ty,
                format_location(location)
            ));
        }

        Declaration::Interface {
            name,
            members,
            location,
            doc,
        } => {
            output.push_str(&format!(
                "{}#### interface {} ({})\n",
                prefix,
                name,
                format_location(location)
            ));
            if let Some(doc) = doc {
                output.push_str(&format!("{}{}\n", prefix, doc));
            }
            if !members.is_empty() {
                output.push_str(&format!("{}Members:\n", prefix));
                for member in members {
                    output.push_str(&format!("{}- {}\n", prefix, member));
                }
            }
            output.push('\n');
        }

        Declaration::Class {
            name,
            members,
            location,
            doc,
            ..
        } => {
            output.push_str(&format!(
                "{}#### class {} ({})\n",
                prefix,
                name,
                format_location(location)
            ));
            if let Some(doc) = doc {
                output.push_str(&format!("{}{}\n", prefix, doc));
            }

            let visible_members: Vec<_> = if public_only {
                members.iter().filter(|m| m.is_public()).collect()
            } else {
                members.iter().collect()
            };

            for member in visible_members {
                output.push_str(&format_declaration_xml(member, public_only, indent + 1));
            }
        }
    }

    output
}

fn format_location(loc: &Location) -> String {
    if loc.start_line == loc.end_line {
        format!("line {}", loc.start_line)
    } else {
        format!("lines {}-{}", loc.start_line, loc.end_line)
    }
}

fn format_summary_xml(summary: &TokenSummary) -> String {
    let mut output = String::new();

    output.push_str(&format!("Total: {} tokens\n", format_number(summary.total)));

    if summary.tree_tokens > 0 || summary.codemap_tokens > 0 || summary.selected_tokens > 0 {
        output.push_str("\nComponent breakdown:\n");
        if summary.tree_tokens > 0 {
            output.push_str(&format!("- File tree: {} tokens\n", format_number(summary.tree_tokens)));
        }
        if summary.codemap_tokens > 0 {
            output.push_str(&format!("- Codemaps: {} tokens\n", format_number(summary.codemap_tokens)));
        }
        if summary.selected_tokens > 0 {
            output.push_str(&format!("- Selected files: {} tokens\n", format_number(summary.selected_tokens)));
        }
    }

    if !summary.file_breakdown.is_empty() {
        output.push_str("\nPer-file breakdown:\n");
        for (path, info) in &summary.file_breakdown {
            let markers = match (info.selected, info.has_codemap) {
                (true, true) => " (selected, codemap)",
                (true, false) => " (selected)",
                (false, true) => " (codemap only)",
                (false, false) => "",
            };
            output.push_str(&format!(
                "- {}: {} tokens{}\n",
                path.display(),
                format_number(info.tokens),
                markers
            ));
        }
    }

    output
}

// ============================================================================
// JSON Formatting
// ============================================================================

#[derive(Serialize)]
struct JsonOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    tree: Option<JsonTree>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    codemaps: Vec<JsonCodemap>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    selected_files: Vec<JsonSelectedFile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<JsonSummary>,
}

#[derive(Serialize)]
struct JsonTree {
    name: String,
    path: String,
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    extension: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lines: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    selected: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    has_codemap: Option<bool>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    children: Vec<JsonTree>,
}

#[derive(Serialize)]
struct JsonCodemap {
    path: String,
    language: String,
    imports: Vec<JsonImport>,
    declarations: Vec<JsonDeclaration>,
    token_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    parse_error: Option<String>,
}

#[derive(Serialize)]
struct JsonImport {
    source: String,
    items: Vec<String>,
}

#[derive(Serialize)]
struct JsonDeclaration {
    kind: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    signature: Option<String>,
    visibility: String,
    location: JsonLocation,
    #[serde(skip_serializing_if = "Option::is_none")]
    is_async: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    doc: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    fields: Vec<JsonField>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    methods: Vec<JsonDeclaration>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    variants: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    members: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    ty: Option<String>,
}

#[derive(Serialize)]
struct JsonField {
    name: String,
    #[serde(rename = "type")]
    ty: String,
    visibility: String,
}

#[derive(Serialize)]
struct JsonLocation {
    start_line: usize,
    end_line: usize,
}

#[derive(Serialize)]
struct JsonSelectedFile {
    path: String,
    content: String,
    lines: usize,
    tokens: usize,
}

#[derive(Serialize)]
struct JsonSummary {
    total_tokens: usize,
    tree_tokens: usize,
    codemap_tokens: usize,
    selected_tokens: usize,
    file_breakdown: BTreeMap<String, FileTokenInfo>,
}

fn format_output_json(
    tree: Option<&FileNode>,
    codemaps: &[Codemap],
    selected_files: &[SelectedFile],
    options: &OutputOptions,
) -> String {
    let selected_set: HashSet<PathBuf> = selected_files.iter().map(|f| f.path.clone()).collect();
    let codemap_set: HashSet<PathBuf> = codemaps.iter().map(|c| c.path.clone()).collect();

    let json_tree = if options.include_tree {
        tree.map(|t| file_node_to_json(t, &selected_set, &codemap_set))
    } else {
        None
    };

    let json_codemaps: Vec<JsonCodemap> = if options.include_codemaps {
        codemaps.iter().map(|c| codemap_to_json(c, options.public_only)).collect()
    } else {
        Vec::new()
    };

    let json_selected: Vec<JsonSelectedFile> = if options.include_selected_files {
        selected_files
            .iter()
            .map(|f| JsonSelectedFile {
                path: f.path.display().to_string(),
                content: f.content.clone(),
                lines: f.lines,
                tokens: f.tokens,
            })
            .collect()
    } else {
        Vec::new()
    };

    let json_summary = if options.include_summary {
        let summary = calculate_summary(tree, codemaps, selected_files);
        Some(JsonSummary {
            total_tokens: summary.total,
            tree_tokens: summary.tree_tokens,
            codemap_tokens: summary.codemap_tokens,
            selected_tokens: summary.selected_tokens,
            file_breakdown: summary
                .file_breakdown
                .into_iter()
                .map(|(k, v)| (k.display().to_string(), v))
                .collect(),
        })
    } else {
        None
    };

    let output = JsonOutput {
        tree: json_tree,
        codemaps: json_codemaps,
        selected_files: json_selected,
        summary: json_summary,
    };

    serde_json::to_string_pretty(&output).unwrap_or_else(|e| format!("{{\"error\": \"{}\"}}", e))
}

fn file_node_to_json(
    node: &FileNode,
    selected: &HashSet<PathBuf>,
    has_codemap: &HashSet<PathBuf>,
) -> JsonTree {
    let (kind, extension, size, lines, language) = match &node.kind {
        NodeKind::Directory => ("directory".to_string(), None, None, None, None),
        NodeKind::File { extension, size, lines } => {
            let lang = extension.as_ref().and_then(|ext| {
                ext.parse::<Language>().ok().map(|l| l.to_string())
            });
            ("file".to_string(), extension.clone(), Some(*size), *lines, lang)
        }
    };

    let is_selected = selected.contains(&node.path);
    let has_map = has_codemap.contains(&node.path);

    JsonTree {
        name: node.name.clone(),
        path: node.path.display().to_string(),
        kind,
        extension,
        size,
        lines,
        language,
        selected: if is_selected { Some(true) } else { None },
        has_codemap: if has_map { Some(true) } else { None },
        children: node
            .children()
            .iter()
            .map(|c| file_node_to_json(c, selected, has_codemap))
            .collect(),
    }
}

fn codemap_to_json(codemap: &Codemap, public_only: bool) -> JsonCodemap {
    let imports: Vec<JsonImport> = codemap
        .imports
        .iter()
        .map(|i| JsonImport {
            source: i.source.clone(),
            items: i.items.clone(),
        })
        .collect();

    let declarations: Vec<JsonDeclaration> = codemap
        .declarations
        .iter()
        .filter(|d| !public_only || d.is_public())
        .map(|d| declaration_to_json(d, public_only))
        .collect();

    JsonCodemap {
        path: codemap.path.display().to_string(),
        language: codemap.language.to_string(),
        imports,
        declarations,
        token_count: codemap.token_count,
        parse_error: codemap.parse_error.clone(),
    }
}

fn declaration_to_json(decl: &Declaration, public_only: bool) -> JsonDeclaration {
    match decl {
        Declaration::Function {
            name,
            signature,
            visibility,
            location,
            is_async,
            doc,
        } => JsonDeclaration {
            kind: "function".to_string(),
            name: name.clone(),
            signature: Some(signature.clone()),
            visibility: visibility.to_string(),
            location: JsonLocation {
                start_line: location.start_line,
                end_line: location.end_line,
            },
            is_async: Some(*is_async),
            doc: doc.clone(),
            fields: Vec::new(),
            methods: Vec::new(),
            variants: Vec::new(),
            members: Vec::new(),
            target: None,
            ty: None,
        },

        Declaration::Struct {
            name,
            fields,
            visibility,
            location,
            methods,
            doc,
        } => {
            let json_fields: Vec<JsonField> = fields
                .iter()
                .filter(|f| !public_only || f.visibility == Visibility::Public)
                .map(|f| JsonField {
                    name: f.name.clone(),
                    ty: f.ty.clone(),
                    visibility: f.visibility.to_string(),
                })
                .collect();

            let json_methods: Vec<JsonDeclaration> = methods
                .iter()
                .filter(|m| !public_only || m.is_public())
                .map(|m| declaration_to_json(m, public_only))
                .collect();

            JsonDeclaration {
                kind: "struct".to_string(),
                name: name.clone(),
                signature: None,
                visibility: visibility.to_string(),
                location: JsonLocation {
                    start_line: location.start_line,
                    end_line: location.end_line,
                },
                is_async: None,
                doc: doc.clone(),
                fields: json_fields,
                methods: json_methods,
                variants: Vec::new(),
                members: Vec::new(),
                target: None,
                ty: None,
            }
        }

        Declaration::Enum {
            name,
            variants,
            visibility,
            location,
            doc,
        } => JsonDeclaration {
            kind: "enum".to_string(),
            name: name.clone(),
            signature: None,
            visibility: visibility.to_string(),
            location: JsonLocation {
                start_line: location.start_line,
                end_line: location.end_line,
            },
            is_async: None,
            doc: doc.clone(),
            fields: Vec::new(),
            methods: Vec::new(),
            variants: variants.clone(),
            members: Vec::new(),
            target: None,
            ty: None,
        },

        Declaration::Trait {
            name,
            methods,
            location,
            doc,
        } => JsonDeclaration {
            kind: "trait".to_string(),
            name: name.clone(),
            signature: None,
            visibility: "public".to_string(),
            location: JsonLocation {
                start_line: location.start_line,
                end_line: location.end_line,
            },
            is_async: None,
            doc: doc.clone(),
            fields: Vec::new(),
            methods: Vec::new(),
            variants: Vec::new(),
            members: methods.clone(),
            target: None,
            ty: None,
        },

        Declaration::TypeAlias {
            name,
            target,
            visibility,
            location,
        } => JsonDeclaration {
            kind: "type_alias".to_string(),
            name: name.clone(),
            signature: None,
            visibility: visibility.to_string(),
            location: JsonLocation {
                start_line: location.start_line,
                end_line: location.end_line,
            },
            is_async: None,
            doc: None,
            fields: Vec::new(),
            methods: Vec::new(),
            variants: Vec::new(),
            members: Vec::new(),
            target: Some(target.clone()),
            ty: None,
        },

        Declaration::Const {
            name,
            ty,
            visibility,
            location,
        } => JsonDeclaration {
            kind: "const".to_string(),
            name: name.clone(),
            signature: None,
            visibility: visibility.to_string(),
            location: JsonLocation {
                start_line: location.start_line,
                end_line: location.end_line,
            },
            is_async: None,
            doc: None,
            fields: Vec::new(),
            methods: Vec::new(),
            variants: Vec::new(),
            members: Vec::new(),
            target: None,
            ty: Some(ty.clone()),
        },

        Declaration::Interface {
            name,
            members,
            location,
            doc,
        } => JsonDeclaration {
            kind: "interface".to_string(),
            name: name.clone(),
            signature: None,
            visibility: "public".to_string(),
            location: JsonLocation {
                start_line: location.start_line,
                end_line: location.end_line,
            },
            is_async: None,
            doc: doc.clone(),
            fields: Vec::new(),
            methods: Vec::new(),
            variants: Vec::new(),
            members: members.clone(),
            target: None,
            ty: None,
        },

        Declaration::Class {
            name,
            members,
            visibility,
            location,
            doc,
        } => {
            let json_members: Vec<JsonDeclaration> = members
                .iter()
                .filter(|m| !public_only || m.is_public())
                .map(|m| declaration_to_json(m, public_only))
                .collect();

            JsonDeclaration {
                kind: "class".to_string(),
                name: name.clone(),
                signature: None,
                visibility: visibility.to_string(),
                location: JsonLocation {
                    start_line: location.start_line,
                    end_line: location.end_line,
                },
                is_async: None,
                doc: doc.clone(),
                fields: Vec::new(),
                methods: json_members,
                variants: Vec::new(),
                members: Vec::new(),
                target: None,
                ty: None,
            }
        }
    }
}

// ============================================================================
// Token Calculation
// ============================================================================

fn calculate_summary(
    tree: Option<&FileNode>,
    codemaps: &[Codemap],
    selected_files: &[SelectedFile],
) -> TokenSummary {
    // Calculate tree tokens (by rendering it)
    let tree_tokens = tree.map_or(0, |t| {
        let rendered = render_tree(t, &RenderOptions::with_metadata());
        count_tokens(&rendered)
    });

    // Sum codemap tokens
    let codemap_tokens: usize = codemaps.iter().map(|c| c.token_count).sum();

    // Sum selected file tokens
    let selected_tokens: usize = selected_files.iter().map(|f| f.tokens).sum();

    // Build file breakdown
    let mut file_breakdown = BTreeMap::new();

    let selected_set: HashSet<PathBuf> = selected_files.iter().map(|f| f.path.clone()).collect();
    let codemap_map: BTreeMap<PathBuf, usize> = codemaps
        .iter()
        .map(|c| (c.path.clone(), c.token_count))
        .collect();

    for file in selected_files {
        let has_codemap = codemap_map.contains_key(&file.path);
        let tokens = file.tokens + codemap_map.get(&file.path).copied().unwrap_or(0);
        file_breakdown.insert(
            file.path.clone(),
            FileTokenInfo {
                tokens,
                selected: true,
                has_codemap,
            },
        );
    }

    for (path, tokens) in &codemap_map {
        if !selected_set.contains(path) {
            file_breakdown.insert(
                path.clone(),
                FileTokenInfo {
                    tokens: *tokens,
                    selected: false,
                    has_codemap: true,
                },
            );
        }
    }

    TokenSummary {
        total: tree_tokens + codemap_tokens + selected_tokens,
        tree_tokens,
        codemap_tokens,
        selected_tokens,
        file_breakdown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_output_options_default() {
        let opts = OutputOptions::default();
        assert_eq!(opts.format, OutputFormat::Xml);
        assert!(opts.include_tree);
        assert!(opts.include_codemaps);
        assert!(!opts.include_selected_files);
        assert!(opts.include_summary);
        assert!(opts.public_only);
    }

    #[test]
    fn test_format_location_single() {
        let loc = Location::new(5, 5);
        assert_eq!(format_location(&loc), "line 5");
    }

    #[test]
    fn test_format_location_range() {
        let loc = Location::new(5, 10);
        assert_eq!(format_location(&loc), "lines 5-10");
    }

    #[test]
    fn test_empty_output_xml() {
        let opts = OutputOptions {
            include_tree: false,
            include_codemaps: false,
            include_selected_files: false,
            include_summary: false,
            ..Default::default()
        };
        let output = format_output(None, &[], &[], &opts);
        assert!(output.is_empty());
    }

    #[test]
    fn test_json_output_empty() {
        let opts = OutputOptions {
            format: OutputFormat::Json,
            include_tree: false,
            include_codemaps: false,
            include_selected_files: false,
            include_summary: false,
            ..Default::default()
        };
        let output = format_output(None, &[], &[], &opts);
        assert!(output.contains('{'));
        assert!(output.contains('}'));
    }
}
