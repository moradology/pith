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

use crate::codemap::{Codemap, Declaration, Location, Visibility};
use crate::filter::Language;
use crate::tokens::{Encoding, TokenCounter};
use crate::tree::{format_number, render_tree, FileNode, NodeKind, RenderOptions};

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
    encoding: Encoding,
) -> String {
    let counter = TokenCounter::new(encoding);

    match options.format {
        OutputFormat::Xml => format_output_xml(tree, codemaps, selected_files, options, &counter),
        OutputFormat::Json => format_output_json(tree, codemaps, selected_files, options, &counter),
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
    counter: &TokenCounter,
) -> String {
    // Build each section as a standalone string and count tokens from the
    // exact bytes that will be emitted.

    let (tree_section, tree_tokens) = if options.include_tree {
        if let Some(tree) = tree {
            let selected: HashSet<&PathBuf> = selected_files.iter().map(|f| &f.path).collect();
            let has_codemap: HashSet<&PathBuf> = codemaps.iter().map(|c| &c.path).collect();

            let render_opts = RenderOptions {
                show_size: true,
                show_lines: true,
                show_language: true,
                selected,
                has_codemap,
            };

            let rendered_tree = render_tree(tree, &render_opts);

            let mut section = String::new();
            section.push_str("<file_map>\n");
            section.push_str(&rendered_tree);
            if !selected_files.is_empty() || !codemaps.is_empty() {
                section.push_str("\nLegend: * = selected, + = has codemap\n");
            }
            section.push_str("</file_map>\n\n");

            let tokens = counter.count(&section);
            (section, tokens)
        } else {
            (String::new(), 0)
        }
    } else {
        (String::new(), 0)
    };

    let (codemap_section, codemap_tokens) = if options.include_codemaps && !codemaps.is_empty() {
        let mut section = String::new();
        section.push_str("<codemaps>\n");
        for (i, codemap) in codemaps.iter().enumerate() {
            if i > 0 {
                section.push_str("\n---\n\n");
            }
            section.push_str(&format_codemap_xml(codemap, options.public_only));
        }
        section.push_str("</codemaps>\n\n");

        let tokens = counter.count(&section);
        (section, tokens)
    } else {
        (String::new(), 0)
    };

    let (selected_section, selected_tokens) =
        if options.include_selected_files && !selected_files.is_empty() {
            let mut section = String::new();
            section.push_str("<selected_files>\n");
            for file in selected_files {
                section.push_str(&format!(
                    "--- {} ({} lines, {} tokens) ---\n",
                    file.path.display(),
                    format_number(file.lines),
                    format_number(file.tokens)
                ));
                section.push_str(&file.content);
                if !file.content.ends_with('\n') {
                    section.push('\n');
                }
                section.push('\n');
            }
            section.push_str("</selected_files>\n\n");

            let tokens = counter.count(&section);
            (section, tokens)
        } else {
            (String::new(), 0)
        };

    let file_breakdown =
        build_file_breakdown(selected_files, codemaps, options.public_only, counter);

    let summary_section = if options.include_summary {
        build_summary_section_fixed_point(
            tree_tokens,
            codemap_tokens,
            selected_tokens,
            file_breakdown,
            counter,
        )
    } else {
        String::new()
    };

    let mut output = String::with_capacity(
        tree_section.len() + codemap_section.len() + selected_section.len() + summary_section.len(),
    );

    output.push_str(&tree_section);
    output.push_str(&codemap_section);
    output.push_str(&selected_section);
    output.push_str(&summary_section);
    output
}

fn build_file_breakdown(
    selected_files: &[SelectedFile],
    codemaps: &[Codemap],
    public_only: bool,
    counter: &TokenCounter,
) -> BTreeMap<PathBuf, FileTokenInfo> {
    let mut breakdown = BTreeMap::new();

    // Per-file breakdown is defined in terms of exact emitted output tokens.
    // For a selected file, this includes the entire per-file block under
    // <selected_files>. For a codemap-only file, this includes the codemap's
    // contribution under <codemaps>.

    // Selected file blocks
    for file in selected_files {
        let mut block = String::new();
        block.push_str(&format!(
            "--- {} ({} lines, {} tokens) ---\n",
            file.path.display(),
            format_number(file.lines),
            format_number(file.tokens)
        ));
        block.push_str(&file.content);
        if !file.content.ends_with('\n') {
            block.push('\n');
        }
        block.push('\n');

        let tokens = counter.count(&block);

        breakdown.insert(
            file.path.clone(),
            FileTokenInfo {
                tokens,
                selected: true,
                has_codemap: codemaps.iter().any(|c| c.path == file.path),
            },
        );
    }

    // Codemap-only blocks: include only codemap contribution within <codemaps>.
    for codemap in codemaps {
        // Skip codemaps that are already selected; those are handled by selected blocks.
        if breakdown.contains_key(&codemap.path) {
            continue;
        }

        let section = format_codemap_xml(codemap, public_only);
        let tokens = counter.count(&section);

        breakdown.insert(
            codemap.path.clone(),
            FileTokenInfo {
                tokens,
                selected: false,
                has_codemap: true,
            },
        );
    }

    breakdown
}

fn build_summary_section_fixed_point(
    tree_tokens: usize,
    codemap_tokens: usize,
    selected_tokens: usize,
    file_breakdown: BTreeMap<PathBuf, FileTokenInfo>,
    counter: &TokenCounter,
) -> String {
    // Fixed-point iteration: the summary includes numbers that affect tokenization.
    let mut summary_tokens = 0usize;

    for _ in 0..10 {
        let summary = calculate_summary(
            tree_tokens,
            codemap_tokens,
            selected_tokens,
            file_breakdown.clone(),
            summary_tokens,
        );

        let mut section = String::new();
        section.push_str("<token_summary>\n");
        section.push_str(&format_summary_xml(&summary));
        section.push_str("</token_summary>\n");

        let next_summary_tokens = counter.count(&section);
        if next_summary_tokens == summary_tokens {
            return section;
        }

        summary_tokens = next_summary_tokens;
    }

    // If not converged, return last attempt.
    let summary = calculate_summary(
        tree_tokens,
        codemap_tokens,
        selected_tokens,
        file_breakdown,
        summary_tokens,
    );

    let mut section = String::new();
    section.push_str("<token_summary>\n");
    section.push_str(&format_summary_xml(&summary));
    section.push_str("</token_summary>\n");
    section
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
                output.push_str(&format!(
                    "- use {}::{{{}}}\n",
                    import.source,
                    import.items.join(", ")
                ));
            }
        }
        output.push('\n');
    }

    // Declarations section
    let decls: Vec<_> = if public_only {
        codemap
            .declarations
            .iter()
            .filter(|d| d.is_public())
            .collect()
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
                fields
                    .iter()
                    .filter(|f| f.visibility == Visibility::Public)
                    .collect()
            } else {
                fields.iter().collect()
            };

            if !visible_fields.is_empty() {
                output.push_str(&format!("{}Fields:\n", prefix));
                for field in visible_fields {
                    let vis = if field.visibility == Visibility::Public {
                        "pub "
                    } else {
                        ""
                    };
                    output.push_str(&format!(
                        "{}- {}{}: {}\n",
                        prefix, vis, field.name, field.ty
                    ));
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
                    if let Declaration::Function {
                        signature,
                        location,
                        ..
                    } = method
                    {
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
            ..
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
            name, ty, location, ..
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
            ..
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
            output.push_str(&format!(
                "- File tree: {} tokens\n",
                format_number(summary.tree_tokens)
            ));
        }
        if summary.codemap_tokens > 0 {
            output.push_str(&format!(
                "- Codemaps: {} tokens\n",
                format_number(summary.codemap_tokens)
            ));
        }
        if summary.selected_tokens > 0 {
            output.push_str(&format!(
                "- Selected files: {} tokens\n",
                format_number(summary.selected_tokens)
            ));
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

#[derive(Serialize, Clone)]
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

#[derive(Serialize, Clone)]
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

#[derive(Serialize, Clone)]
struct JsonCodemap {
    path: String,
    language: String,
    imports: Vec<JsonImport>,
    declarations: Vec<JsonDeclaration>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parse_error: Option<String>,
}

#[derive(Serialize, Clone)]
struct JsonImport {
    source: String,
    items: Vec<String>,
}

#[derive(Serialize, Clone)]
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

#[derive(Serialize, Clone)]
struct JsonField {
    name: String,
    #[serde(rename = "type")]
    ty: String,
    visibility: String,
}

#[derive(Serialize, Clone)]
struct JsonLocation {
    start_line: usize,
    end_line: usize,
}

#[derive(Serialize, Clone)]
struct JsonSelectedFile {
    path: String,
    content: String,
    lines: usize,
    tokens: usize,
}

#[derive(Serialize, Clone)]
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
    counter: &TokenCounter,
) -> String {
    let selected_set: HashSet<&PathBuf> = selected_files.iter().map(|f| &f.path).collect();
    let codemap_set: HashSet<&PathBuf> = codemaps.iter().map(|c| &c.path).collect();

    let json_tree = if options.include_tree {
        tree.map(|t| file_node_to_json(t, &selected_set, &codemap_set))
    } else {
        None
    };

    let json_codemaps: Vec<JsonCodemap> = if options.include_codemaps {
        codemaps
            .iter()
            .map(|c| codemap_to_json(c, options.public_only))
            .collect()
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
        let output_without_summary = {
            let tmp = JsonOutput {
                tree: json_tree.clone(),
                codemaps: json_codemaps.clone(),
                selected_files: json_selected.clone(),
                summary: None,
            };
            serde_json::to_string_pretty(&tmp).unwrap_or_default()
        };

        let tree_tokens = if options.include_tree {
            json_tree.as_ref().map_or(0, |t| {
                let s = serde_json::to_string_pretty(t).unwrap_or_default();
                counter.count(&s)
            })
        } else {
            0
        };

        let codemap_tokens = if options.include_codemaps {
            let s = serde_json::to_string_pretty(&json_codemaps).unwrap_or_default();
            counter.count(&s)
        } else {
            0
        };

        let selected_tokens = if options.include_selected_files {
            let s = serde_json::to_string_pretty(&json_selected).unwrap_or_default();
            counter.count(&s)
        } else {
            0
        };

        // Fixed-point: summary includes total_tokens, which affects token count.
        let mut summary_tokens = 0usize;
        let mut total_tokens = counter.count(&output_without_summary);

        for _ in 0..10 {
            let summary = JsonSummary {
                total_tokens,
                tree_tokens,
                codemap_tokens,
                selected_tokens,
                file_breakdown: BTreeMap::new(),
            };

            let tmp = JsonOutput {
                tree: json_tree.clone(),
                codemaps: json_codemaps.clone(),
                selected_files: json_selected.clone(),
                summary: Some(summary),
            };

            let full = serde_json::to_string_pretty(&tmp).unwrap_or_default();
            let next_total = counter.count(&full);

            // Keep summary_tokens in sync for completeness (not currently exported).
            summary_tokens = next_total.saturating_sub(counter.count(&output_without_summary));

            if next_total == total_tokens {
                break;
            }

            total_tokens = next_total;
        }

        let _ = summary_tokens;

        Some(JsonSummary {
            total_tokens,
            tree_tokens,
            codemap_tokens,
            selected_tokens,
            file_breakdown: BTreeMap::new(),
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

    serde_json::to_string_pretty(&output).unwrap_or_else(|e| {
        #[derive(Serialize)]
        struct JsonError {
            error: String,
        }

        serde_json::to_string_pretty(&JsonError {
            error: e.to_string(),
        })
        .unwrap_or_else(|_| "{\"error\":\"serialization failed\"}".to_string())
    })
}

fn file_node_to_json(
    node: &FileNode,
    selected: &HashSet<&PathBuf>,
    has_codemap: &HashSet<&PathBuf>,
) -> JsonTree {
    let (kind, extension, size, lines, language) = match &node.kind {
        NodeKind::Directory => ("directory".to_string(), None, None, None, None),
        NodeKind::File {
            extension,
            size,
            lines,
        } => {
            let lang = extension
                .as_ref()
                .and_then(|ext| ext.parse::<Language>().ok().map(|l| l.to_string()));
            (
                "file".to_string(),
                extension.clone(),
                Some(*size),
                *lines,
                lang,
            )
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
            items: i.items.to_vec(),
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
            variants: variants.to_vec(),
            members: Vec::new(),
            target: None,
            ty: None,
        },

        Declaration::Trait {
            name,
            methods,
            visibility,
            location,
            doc,
        } => JsonDeclaration {
            kind: "trait".to_string(),
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
            variants: Vec::new(),
            members: methods.to_vec(),
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
            visibility,
            location,
            doc,
        } => JsonDeclaration {
            kind: "interface".to_string(),
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
            variants: Vec::new(),
            members: members.to_vec(),
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
    tree_tokens: usize,
    codemap_tokens: usize,
    selected_tokens: usize,
    file_breakdown: BTreeMap<PathBuf, FileTokenInfo>,
    summary_tokens: usize,
) -> TokenSummary {
    TokenSummary {
        total: tree_tokens + codemap_tokens + selected_tokens + summary_tokens,
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
        let output = format_output(None, &[], &[], &opts, Encoding::default());
        assert!(output.is_empty());
    }

    #[test]
    fn test_xml_summary_total_matches_exact_output_tokens() {
        use crate::codemap::{Codemap, Declaration, Location, Visibility};
        use crate::filter::Language;
        use crate::tokens::count_tokens_with_encoding;

        let tree = FileNode::directory("project", "project");

        let codemap = Codemap {
            path: PathBuf::from("project/a.rs"),
            language: Language::Rust,
            imports: smallvec::smallvec![],
            declarations: smallvec::smallvec![Declaration::Function {
                name: "a".into(),
                signature: "pub fn a()".into(),
                visibility: Visibility::Public,
                location: Location::single_line(1),
                is_async: false,
                doc: None,
            }],
            parse_error: None,
        };

        let opts = OutputOptions {
            format: OutputFormat::Xml,
            include_tree: true,
            include_codemaps: true,
            include_selected_files: false,
            include_summary: true,
            public_only: true,
        };

        let out = format_output(Some(&tree), &[codemap], &[], &opts, Encoding::Cl100kBase);
        let actual = count_tokens_with_encoding(&out, Encoding::Cl100kBase);

        // Parse the reported total out of the output.
        let line = out
            .lines()
            .find(|l| l.starts_with("Total: "))
            .expect("missing Total line");
        let reported: usize = line
            .trim_start_matches("Total: ")
            .trim_end_matches(" tokens")
            .replace(',', "")
            .parse()
            .expect("invalid total number");

        assert_eq!(reported, actual);
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
        let output = format_output(None, &[], &[], &opts, Encoding::default());
        assert!(output.contains('{'));
        assert!(output.contains('}'));
    }

    #[test]
    fn test_json_summary_total_matches_exact_output_tokens() {
        use crate::codemap::{Codemap, Declaration, Location, Visibility};
        use crate::filter::Language;
        use crate::tokens::count_tokens_with_encoding;

        let tree = FileNode::directory("project", "project");

        let codemap = Codemap {
            path: PathBuf::from("project/a.rs"),
            language: Language::Rust,
            imports: smallvec::smallvec![],
            declarations: smallvec::smallvec![Declaration::Function {
                name: "a".into(),
                signature: "pub fn a()".into(),
                visibility: Visibility::Public,
                location: Location::single_line(1),
                is_async: false,
                doc: None,
            }],
            parse_error: None,
        };

        let opts = OutputOptions {
            format: OutputFormat::Json,
            include_tree: true,
            include_codemaps: true,
            include_selected_files: false,
            include_summary: true,
            public_only: true,
        };

        let out = format_output(Some(&tree), &[codemap], &[], &opts, Encoding::Cl100kBase);
        let actual = count_tokens_with_encoding(&out, Encoding::Cl100kBase);

        let v: serde_json::Value = serde_json::from_str(&out).expect("invalid json output");
        let reported = v
            .get("summary")
            .and_then(|s| s.get("total_tokens"))
            .and_then(|t| t.as_u64())
            .expect("missing total_tokens") as usize;

        assert_eq!(reported, actual);
    }
}
