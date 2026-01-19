//! Pith CLI - Generate optimized codebase context for LLMs.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::PathBuf;

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{generate, Shell};
use glob::Pattern;
use pith::codemap::{extract_codemap, ExtractOptions};
use pith::errors::{exit_code, PithError};
use pith::filter::{detect_language, should_process, FilterResult, Language};
use pith::output::{format_output, OutputFormat, OutputOptions, SelectedFile};
use pith::tokens::{count_tokens_with_encoding, Encoding};
use pith::tree::{render_tree, RenderOptions};
use pith::walker::{build_tree_with_options, walk, WalkOptions};
use rayon::prelude::*;
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

        /// Token encoding for token summary
        #[arg(long, default_value = "cl100k")]
        encoding: EncodingArg,

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

        /// Token encoding for token summary
        #[arg(long, default_value = "cl100k")]
        encoding: EncodingArg,

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
            encoding,
            lang,
        } => run_codemap(
            path,
            json,
            include_docs,
            include_private,
            encoding.into(),
            lang,
        ),
        Commands::Context {
            path,
            json,
            include_docs,
            include_private,
            encoding,
            select,
            lang,
        } => run_context(
            path,
            json,
            include_docs,
            include_private,
            encoding.into(),
            select,
            lang,
        ),
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
            #[derive(Serialize)]
            struct ErrorOutput {
                error: String,
            }

            let payload = ErrorOutput {
                error: e.to_string(),
            };

            let json = serde_json::to_string(&payload)
                .unwrap_or_else(|_| "{\"error\":\"serialization failed\"}".to_string());
            eprintln!("{json}");
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
            extensions: lang
                .extensions()
                .iter()
                .map(|e| format!(".{}", e))
                .collect(),
        })
        .collect();

    if json {
        #[derive(Serialize)]
        struct Output {
            languages: Vec<LanguageInfo>,
        }
        let output = Output { languages };
        let json = serde_json::to_string_pretty(&output)
            .map_err(|e| PithError::Io(std::io::Error::other(e.to_string())))?;
        println!("{json}");
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
        // Collect file paths for parallel processing
        let paths: Vec<PathBuf> = walk(&path)
            .filter_map(|e| e.ok())
            .filter(|e| e.is_file)
            .map(|e| e.path)
            .collect();

        file_tokens = paths
            .par_iter()
            .filter_map(|entry_path| {
                use std::io::Read;

                let mut file = std::fs::File::open(entry_path).ok()?;

                let mut first_kb = [0u8; 1024];
                let n = file.read(&mut first_kb).ok()?;

                match should_process(entry_path, Some(&first_kb[..n])) {
                    FilterResult::Accept(_) => {}
                    FilterResult::Reject(_) => return None,
                }

                let mut content = String::new();
                content.push_str(std::str::from_utf8(&first_kb[..n]).ok()?);
                file.read_to_string(&mut content).ok()?;

                let count = count_tokens_with_encoding(&content, encoding);

                let relative = entry_path
                    .strip_prefix(&path)
                    .unwrap_or(entry_path)
                    .to_path_buf();
                Some((relative, count))
            })
            .collect();
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
        let json = serde_json::to_string_pretty(&output)
            .map_err(|e| PithError::Io(std::io::Error::other(e.to_string())))?;
        println!("{json}");
    } else {
        use std::io::{BufWriter, Write};
        let stdout = std::io::stdout();
        let mut out = BufWriter::new(stdout.lock());
        if per_file {
            for (file, count) in &file_tokens {
                writeln!(out, "{}: {} tokens", file.display(), count).ok();
            }
        }
        writeln!(out, "Total: {} tokens", total).ok();
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

    let tree = build_tree_with_options(&path, &walk_opts)
        .map_err(|e| PithError::Io(std::io::Error::other(e.to_string())))?;

    if json {
        // Use serde to serialize the tree
        let json = serde_json::to_string_pretty(&tree_to_json(&tree))
            .map_err(|e| PithError::Io(std::io::Error::other(e.to_string())))?;
        println!("{json}");
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
        NodeKind::File {
            extension,
            size,
            lines,
        } => ("file".to_string(), extension.clone(), Some(*size), *lines),
    };

    JsonTreeNode {
        name: node.name.clone(),
        path: node.path.display().to_string(),
        kind,
        extension,
        size,
        lines,
        children: node.children().iter().map(tree_to_json).collect(),
    }
}

// --- Codemap command ---

fn run_codemap(
    path: PathBuf,
    json: bool,
    include_docs: bool,
    include_private: bool,
    encoding: Encoding,
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

    for entry in walk(&path).flatten() {
        if !entry.is_file {
            continue;
        }

        let entry_path = entry.path.as_path();

        let lang = match detect_language(entry_path) {
            Some(l) => l,
            None => continue,
        };

        // Apply language filter if specified
        if !lang_set.is_empty() && !lang_set.contains(&lang) {
            continue;
        }

        // Check heuristics on first 1KB
        let mut file = match std::fs::File::open(entry_path) {
            Ok(f) => f,
            Err(_) => continue,
        };

        let mut first_kb = [0u8; 1024];
        let n = {
            use std::io::Read;
            match file.read(&mut first_kb) {
                Ok(n) => n,
                Err(_) => continue,
            }
        };

        match should_process(entry_path, Some(&first_kb[..n])) {
            FilterResult::Accept(_) => {}
            FilterResult::Reject(_) => continue,
        }

        let mut content = String::new();
        let prefix = match std::str::from_utf8(&first_kb[..n]) {
            Ok(s) => s,
            Err(_) => continue,
        };
        content.push_str(prefix);
        if file.read_to_string(&mut content).is_err() {
            continue;
        }

        let codemap = extract_codemap(entry_path, &content, lang, &extract_opts);
        codemaps.push(codemap);
    }

    if codemaps.is_empty() {
        return Err(PithError::NoFilesFound(path));
    }

    let output_opts = OutputOptions {
        format: if json {
            OutputFormat::Json
        } else {
            OutputFormat::Xml
        },
        include_tree: false,
        include_codemaps: true,
        include_selected_files: false,
        include_summary: true,
        public_only: !include_private,
    };

    let output = format_output(None, &codemaps, &[], &output_opts, encoding);
    print!("{}", output);

    Ok(())
}

// --- Context command ---

fn run_context(
    path: PathBuf,
    json: bool,
    include_docs: bool,
    include_private: bool,
    encoding: Encoding,
    select_patterns: Vec<String>,
    lang_filter: Vec<LanguageArg>,
) -> Result<(), PithError> {
    if !path.exists() {
        return Err(PithError::PathNotFound(path));
    }

    let lang_set: Vec<Language> = lang_filter.into_iter().map(|l| l.into()).collect();

    // Build the file tree
    let tree = build_tree_with_options(&path, &WalkOptions::default())
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

    for entry in walk(&path).flatten() {
        if !entry.is_file {
            continue;
        }

        let entry_path = entry.path.as_path();

        let relative = entry_path.strip_prefix(&path).unwrap_or(entry_path);
        let relative_str = relative.to_string_lossy();

        // Check if file matches any select pattern
        let is_selected = patterns.iter().any(|p| p.matches(&relative_str));

        // Check language
        let lang = detect_language(entry_path);

        // Check heuristics on first 1KB (binary/minified/generated)
        let mut file = match std::fs::File::open(entry_path) {
            Ok(f) => f,
            Err(_) => continue,
        };

        let mut first_kb = [0u8; 1024];
        let n = {
            use std::io::Read;
            match file.read(&mut first_kb) {
                Ok(n) => n,
                Err(_) => continue,
            }
        };

        match should_process(entry_path, Some(&first_kb[..n])) {
            FilterResult::Accept(_) => {}
            FilterResult::Reject(_) => continue,
        }

        // Read full content (reuse already-read prefix)
        let mut content = String::new();
        let prefix = match std::str::from_utf8(&first_kb[..n]) {
            Ok(s) => s,
            Err(_) => continue,
        };
        content.push_str(prefix);
        if file.read_to_string(&mut content).is_err() {
            continue;
        }

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
            let tokens = count_tokens_with_encoding(&content, encoding);
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
        format: if json {
            OutputFormat::Json
        } else {
            OutputFormat::Xml
        },
        include_tree: true,
        include_codemaps: true,
        include_selected_files: !selected_files.is_empty(),
        include_summary: true,
        public_only: !include_private,
    };

    let output = format_output(
        Some(&tree),
        &codemaps,
        &selected_files,
        &output_opts,
        encoding,
    );
    print!("{}", output);

    Ok(())
}
