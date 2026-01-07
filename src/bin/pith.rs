//! Pith CLI - Generate optimized codebase context for LLMs.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{generate, Shell};
use glob::Pattern;
use ignore::WalkBuilder;
use pith::codemap::{extract_codemap, ExtractOptions};
use pith::errors::{exit_code, PithError};
use pith::filter::{detect_language, passes_extension_filter, Language};
use pith::output::{format_output, OutputFormat, OutputOptions, SelectedFile};
use pith::tokens::{count_tokens, count_tokens_with_encoding, Encoding};
use pith::tree::{render_tree, RenderOptions};
use pith::walker::{build_tree_with_options, WalkOptions};
use serde::Serialize;

#[derive(Parser)]
#[command(name = "pith")]
#[command(about = "Generate optimized codebase context for LLMs")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Display file tree with metadata
    Tree {
        /// Root directory to scan
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Output as JSON
        #[arg(long)]
        json: bool,

        /// Hide file sizes and line counts
        #[arg(long)]
        no_metadata: bool,

        /// Include hidden files and directories
        #[arg(long)]
        include_hidden: bool,

        /// Maximum directory depth
        #[arg(long)]
        max_depth: Option<usize>,
    },

    /// Extract API signatures from source files
    Codemap {
        /// Root directory to scan
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Output as JSON
        #[arg(long)]
        json: bool,

        /// Include doc comments
        #[arg(long)]
        include_docs: bool,

        /// Include private items
        #[arg(long)]
        include_private: bool,

        /// Filter to specific language(s)
        #[arg(long, value_delimiter = ',')]
        lang: Vec<LanguageArg>,
    },

    /// Generate full context (tree + codemaps)
    Context {
        /// Root directory to scan
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Output as JSON
        #[arg(long)]
        json: bool,

        /// Include doc comments
        #[arg(long)]
        include_docs: bool,

        /// Include private items
        #[arg(long)]
        include_private: bool,

        /// Select files for full content inclusion
        #[arg(long)]
        select: Vec<String>,

        /// Filter to specific language(s)
        #[arg(long, value_delimiter = ',')]
        lang: Vec<LanguageArg>,
    },

    /// Count tokens for files
    Tokens {
        /// Root directory or file to count
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Output as JSON
        #[arg(long)]
        json: bool,

        /// Token encoding
        #[arg(long, default_value = "cl100k")]
        encoding: EncodingArg,

        /// Show per-file breakdown
        #[arg(long)]
        per_file: bool,
    },

    /// Show supported languages
    Languages {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },
}

#[derive(Clone, ValueEnum)]
enum LanguageArg {
    Rust,
    Typescript,
    Tsx,
    Javascript,
    Jsx,
    Python,
    Go,
}

#[derive(Clone, ValueEnum)]
enum EncodingArg {
    Cl100k,
    O200k,
}

impl From<EncodingArg> for Encoding {
    fn from(arg: EncodingArg) -> Self {
        match arg {
            EncodingArg::Cl100k => Encoding::Cl100kBase,
            EncodingArg::O200k => Encoding::O200kBase,
        }
    }
}

impl From<LanguageArg> for Language {
    fn from(arg: LanguageArg) -> Self {
        match arg {
            LanguageArg::Rust => Language::Rust,
            LanguageArg::Typescript => Language::TypeScript,
            LanguageArg::Tsx => Language::Tsx,
            LanguageArg::Javascript => Language::JavaScript,
            LanguageArg::Jsx => Language::Jsx,
            LanguageArg::Python => Language::Python,
            LanguageArg::Go => Language::Go,
        }
    }
}

fn main() {
    let cli = Cli::parse();
    let json_output = json_flag(&cli.command);

    let result = match cli.command {
        Commands::Tree {
            path,
            json,
            no_metadata,
            include_hidden,
            max_depth,
        } => run_tree(path, json, no_metadata, include_hidden, max_depth),
        Commands::Codemap {
            path,
            json,
            include_docs,
            include_private,
            lang,
        } => run_codemap(path, json, include_docs, include_private, lang),
        Commands::Context {
            path,
            json,
            include_docs,
            include_private,
            select,
            lang,
        } => run_context(path, json, include_docs, include_private, select, lang),
        Commands::Tokens {
            path,
            json,
            encoding,
            per_file,
        } => run_tokens(path, json, encoding.into(), per_file),
        Commands::Languages { json } => run_languages(json),
        Commands::Completions { shell } => {
            generate(shell, &mut Cli::command(), "pith", &mut std::io::stdout());
            Ok(())
        }
    };

    if let Err(e) = result {
        if json_output {
            eprintln!(r#"{{"error": "{}"}}"#, e);
        } else {
            eprintln!("error: {}", e);
        }
        std::process::exit(exit_code(&e));
    }
}

fn json_flag(cmd: &Commands) -> bool {
    match cmd {
        Commands::Tree { json, .. } => *json,
        Commands::Codemap { json, .. } => *json,
        Commands::Context { json, .. } => *json,
        Commands::Tokens { json, .. } => *json,
        Commands::Languages { json } => *json,
        Commands::Completions { .. } => false,
    }
}

// --- Languages command ---

#[derive(Serialize)]
struct LanguageInfo {
    name: String,
    extensions: Vec<String>,
}

fn run_languages(json: bool) -> Result<(), PithError> {
    let languages: Vec<LanguageInfo> = Language::all()
        .iter()
        .map(|lang| LanguageInfo {
            name: lang.to_string(),
            extensions: lang.extensions().iter().map(|e| format!(".{}", e)).collect(),
        })
        .collect();

    if json {
        #[derive(Serialize)]
        struct Output {
            languages: Vec<LanguageInfo>,
        }
        let output = Output { languages };
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else {
        println!("Supported languages:");
        for lang in &languages {
            println!("  {:12} {}", lang.name, lang.extensions.join(", "));
        }
    }

    Ok(())
}

// --- Tokens command ---

fn run_tokens(
    path: PathBuf,
    json: bool,
    encoding: Encoding,
    per_file: bool,
) -> Result<(), PithError> {
    if !path.exists() {
        return Err(PithError::PathNotFound(path));
    }

    let mut file_tokens: BTreeMap<PathBuf, usize> = BTreeMap::new();

    if path.is_file() {
        let content = fs::read_to_string(&path)?;
        let count = count_tokens_with_encoding(&content, encoding);
        file_tokens.insert(path.clone(), count);
    } else {
        let walker = WalkBuilder::new(&path)
            .hidden(false)
            .git_ignore(true)
            .build();

        for entry in walker.flatten() {
            let entry_path = entry.path();
            if !entry_path.is_file() {
                continue;
            }

            if passes_extension_filter(entry_path).is_none() {
                continue;
            }

            let content = match fs::read_to_string(entry_path) {
                Ok(c) => c,
                Err(_) => continue, // Skip unreadable files
            };

            let count = count_tokens_with_encoding(&content, encoding);
            let relative = entry_path
                .strip_prefix(&path)
                .unwrap_or(entry_path)
                .to_path_buf();
            file_tokens.insert(relative, count);
        }
    }

    let total: usize = file_tokens.values().sum();

    if json {
        #[derive(Serialize)]
        struct Output {
            total: usize,
            encoding: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            files: Option<BTreeMap<String, usize>>,
        }

        let files = if per_file {
            Some(
                file_tokens
                    .into_iter()
                    .map(|(k, v)| (k.display().to_string(), v))
                    .collect(),
            )
        } else {
            None
        };

        let output = Output {
            total,
            encoding: encoding.to_string(),
            files,
        };
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else {
        if per_file {
            for (file, count) in &file_tokens {
                println!("{}: {} tokens", file.display(), count);
            }
        }
        println!("Total: {} tokens", total);
    }

    Ok(())
}

// --- Tree command ---

fn run_tree(
    path: PathBuf,
    json: bool,
    no_metadata: bool,
    include_hidden: bool,
    max_depth: Option<usize>,
) -> Result<(), PithError> {
    if !path.exists() {
        return Err(PithError::PathNotFound(path));
    }

    let walk_opts = WalkOptions {
        max_depth,
        include_hidden,
        ..Default::default()
    };

    let tree = build_tree_with_options(&path, walk_opts)
        .map_err(|e| PithError::Io(std::io::Error::other(e.to_string())))?;

    if json {
        // Use serde to serialize the tree
        println!("{}", serde_json::to_string_pretty(&tree_to_json(&tree)).unwrap());
    } else {
        let render_opts = RenderOptions {
            show_size: !no_metadata,
            show_lines: !no_metadata,
            show_language: !no_metadata,
            ..Default::default()
        };
        print!("{}", render_tree(&tree, &render_opts));
    }

    Ok(())
}

#[derive(Serialize)]
struct JsonTreeNode {
    name: String,
    path: String,
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    extension: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lines: Option<usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    children: Vec<JsonTreeNode>,
}

fn tree_to_json(node: &pith::tree::FileNode) -> JsonTreeNode {
    use pith::tree::NodeKind;

    let (kind, extension, size, lines) = match &node.kind {
        NodeKind::Directory => ("directory".to_string(), None, None, None),
        NodeKind::File { extension, size, lines } => {
            ("file".to_string(), extension.clone(), Some(*size), *lines)
        }
    };

    JsonTreeNode {
        name: node.name.clone(),
        path: node.path.display().to_string(),
        kind,
        extension,
        size,
        lines,
        children: node.children.iter().map(tree_to_json).collect(),
    }
}

// --- Codemap command ---

fn run_codemap(
    path: PathBuf,
    json: bool,
    include_docs: bool,
    include_private: bool,
    lang_filter: Vec<LanguageArg>,
) -> Result<(), PithError> {
    if !path.exists() {
        return Err(PithError::PathNotFound(path));
    }

    let lang_set: Vec<Language> = lang_filter.into_iter().map(|l| l.into()).collect();

    let extract_opts = ExtractOptions {
        include_docs,
        include_private,
    };

    let mut codemaps = Vec::new();

    let walker = WalkBuilder::new(&path)
        .hidden(false)
        .git_ignore(true)
        .build();

    for entry in walker.flatten() {
        let entry_path = entry.path();
        if !entry_path.is_file() {
            continue;
        }

        let lang = match detect_language(entry_path) {
            Some(l) => l,
            None => continue,
        };

        // Apply language filter if specified
        if !lang_set.is_empty() && !lang_set.contains(&lang) {
            continue;
        }

        let content = match fs::read_to_string(entry_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let codemap = extract_codemap(entry_path, &content, lang, &extract_opts);
        codemaps.push(codemap);
    }

    if codemaps.is_empty() {
        return Err(PithError::NoFilesFound(path));
    }

    let output_opts = OutputOptions {
        format: if json { OutputFormat::Json } else { OutputFormat::Xml },
        include_tree: false,
        include_codemaps: true,
        include_selected_files: false,
        include_summary: true,
        public_only: !include_private,
    };

    let output = format_output(None, &codemaps, &[], &output_opts);
    print!("{}", output);

    Ok(())
}

// --- Context command ---

fn run_context(
    path: PathBuf,
    json: bool,
    include_docs: bool,
    include_private: bool,
    select_patterns: Vec<String>,
    lang_filter: Vec<LanguageArg>,
) -> Result<(), PithError> {
    if !path.exists() {
        return Err(PithError::PathNotFound(path));
    }

    let lang_set: Vec<Language> = lang_filter.into_iter().map(|l| l.into()).collect();

    // Build the file tree
    let tree = build_tree_with_options(&path, WalkOptions::default())
        .map_err(|e| PithError::Io(std::io::Error::other(e.to_string())))?;

    let extract_opts = ExtractOptions {
        include_docs,
        include_private,
    };

    // Compile glob patterns
    let patterns: Vec<Pattern> = select_patterns
        .iter()
        .filter_map(|p| Pattern::new(p).ok())
        .collect();

    let mut codemaps = Vec::new();
    let mut selected_files = Vec::new();

    let walker = WalkBuilder::new(&path)
        .hidden(false)
        .git_ignore(true)
        .build();

    for entry in walker.flatten() {
        let entry_path = entry.path();
        if !entry_path.is_file() {
            continue;
        }

        let relative = entry_path
            .strip_prefix(&path)
            .unwrap_or(entry_path);
        let relative_str = relative.to_string_lossy();

        // Check if file matches any select pattern
        let is_selected = patterns.iter().any(|p| p.matches(&relative_str));

        // Check language
        let lang = detect_language(entry_path);

        // Read content
        let content = match fs::read_to_string(entry_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Extract codemap if it's a supported language
        if let Some(lang) = lang {
            // Apply language filter if specified
            if lang_set.is_empty() || lang_set.contains(&lang) {
                let codemap = extract_codemap(entry_path, &content, lang, &extract_opts);
                codemaps.push(codemap);
            }
        }

        // Add to selected files if it matches patterns
        if is_selected {
            let lines = content.lines().count();
            let tokens = count_tokens(&content);
            selected_files.push(SelectedFile {
                path: entry_path.to_path_buf(),
                content,
                lines,
                tokens,
            });
        }
    }

    if codemaps.is_empty() {
        return Err(PithError::NoFilesFound(path));
    }

    let output_opts = OutputOptions {
        format: if json { OutputFormat::Json } else { OutputFormat::Xml },
        include_tree: true,
        include_codemaps: true,
        include_selected_files: !selected_files.is_empty(),
        include_summary: true,
        public_only: !include_private,
    };

    let output = format_output(Some(&tree), &codemaps, &selected_files, &output_opts);
    print!("{}", output);

    Ok(())
}
