# CLI Specification

## Overview

```
pith - Generate optimized codebase context for LLMs

USAGE:
    pith <COMMAND> [OPTIONS]

COMMANDS:
    tree       Display file tree with metadata
    codemap    Extract API signatures from source files
    context    Generate full context (tree + codemaps)
    tokens     Count tokens for files
    languages  Show supported languages

OPTIONS:
    -h, --help       Print help
    -V, --version    Print version
```

## Commands

### tree

Display file tree with optional metadata.

```
pith tree [PATH] [OPTIONS]

ARGS:
    <PATH>    Root directory to scan [default: .]

OPTIONS:
    --json              Output as JSON
    --no-metadata       Hide file sizes and line counts
    --include-hidden    Include hidden files and directories
    --max-depth <N>     Maximum directory depth
    -h, --help          Print help
```

**Examples:**

```bash
# Basic tree
pith tree ./project

# JSON output
pith tree ./project --json

# Limited depth
pith tree ./project --max-depth 3

# Include hidden files
pith tree ./project --include-hidden
```

**Output (default):**
```
project/
├── src/
│   ├── lib.rs [rust, 245 lines, 2.3KB]
│   └── main.rs [rust, 89 lines, 1.1KB]
└── Cargo.toml [toml, 25 lines, 512B]
```

**Output (JSON):**
```json
{
  "tree": {
    "name": "project",
    "kind": "directory",
    "children": [...]
  }
}
```

### codemap

Extract API signatures from source files.

```
pith codemap [PATH] [OPTIONS]

ARGS:
    <PATH>    Root directory to scan [default: .]

OPTIONS:
    --json               Output as JSON
    --include-docs       Include doc comments
    --include-private    Include private items (default: public only)
    --lang <LANG>        Filter to specific language(s) [possible values: rust, typescript, tsx, javascript, jsx, python, go]
    -h, --help           Print help
```

**Examples:**

```bash
# All codemaps
pith codemap ./project

# Rust only
pith codemap ./project --lang rust

# Include everything
pith codemap ./project --include-docs --include-private

# JSON for programmatic use
pith codemap ./project --json
```

**Output (default):**
```xml
<codemaps>
## src/lib.rs

### Imports
- use std::collections::HashMap

### Declarations

#### pub struct Config (lines 7-10)
Fields:
- pub name: String
- pub timeout: Duration

#### pub fn process(input: &str) -> Result<Output> (line 15)

</codemaps>
```

### context

Generate full context including tree and codemaps.

```
pith context [PATH] [OPTIONS]

ARGS:
    <PATH>    Root directory to scan [default: .]

OPTIONS:
    --json               Output as JSON
    --include-docs       Include doc comments
    --include-private    Include private items
    --select <GLOB>      Select files for full content inclusion
    --lang <LANG>        Filter to specific language(s)
    -h, --help           Print help
```

**Examples:**

```bash
# Full context
pith context ./project

# With selected files
pith context ./project --select "src/**/*.rs"

# JSON output
pith context ./project --json
```

**Output:**
```xml
<file_map>
project/
├── src/
│   ├── lib.rs [rust, 245 lines, 2.3KB] +
│   └── main.rs [rust, 89 lines, 1.1KB] +
└── Cargo.toml [toml, 25 lines, 512B]

Legend: * = selected, + = has codemap
</file_map>

<codemaps>
...
</codemaps>

<token_summary>
Total: 1,456 tokens
</token_summary>
```

### tokens

Count tokens for files.

```
pith tokens [PATH] [OPTIONS]

ARGS:
    <PATH>    Root directory or file to count [default: .]

OPTIONS:
    --json               Output as JSON
    --encoding <ENC>     Token encoding [default: cl100k] [possible values: cl100k, o200k]
    --per-file           Show per-file breakdown
    -h, --help           Print help
```

**Examples:**

```bash
# Total tokens
pith tokens ./project

# Per-file breakdown
pith tokens ./project --per-file

# Different encoding
pith tokens ./project --encoding o200k

# JSON output
pith tokens ./project --json --per-file
```

**Output (default):**
```
Total tokens: 12,345
```

**Output (per-file):**
```
src/lib.rs: 2,345 tokens
src/main.rs: 1,234 tokens
src/utils/helpers.rs: 567 tokens
Total: 4,146 tokens
```

**Output (JSON):**
```json
{
  "total": 4146,
  "files": {
    "src/lib.rs": 2345,
    "src/main.rs": 1234,
    "src/utils/helpers.rs": 567
  }
}
```

### languages

Show supported languages and their extensions.

```
pith languages [OPTIONS]

OPTIONS:
    --json    Output as JSON
    -h, --help    Print help
```

**Output (default):**
```
Supported languages:
  rust        .rs
  typescript  .ts
  tsx         .tsx
  javascript  .js, .mjs, .cjs
  jsx         .jsx
  python      .py, .pyi
  go          .go
```

**Output (JSON):**
```json
{
  "languages": [
    { "name": "rust", "extensions": [".rs"] },
    { "name": "typescript", "extensions": [".ts"] },
    { "name": "tsx", "extensions": [".tsx"] },
    { "name": "javascript", "extensions": [".js", ".mjs", ".cjs"] },
    { "name": "jsx", "extensions": [".jsx"] },
    { "name": "python", "extensions": [".py", ".pyi"] },
    { "name": "go", "extensions": [".go"] }
  ]
}
```

## Global Options

These options apply to all commands:

| Flag | Description |
|------|-------------|
| `-h, --help` | Print help information |
| `-V, --version` | Print version information |
| `--json` | Output as JSON (available on most commands) |

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | General error |
| 2 | Invalid arguments |
| 3 | Path not found |
| 4 | Permission denied |
| 5 | No supported files found |

## Piping and Redirection

Primary use case is stdout piping to LLMs:

```bash
# Pipe to LLM CLI
pith context ./project | llm "Analyze this codebase"

# Save to file
pith context ./project > context.txt

# Combine with other tools
pith codemap ./project --json | jq '.codemaps[].path'
```

## Glob Patterns

The `--select` flag accepts glob patterns:

| Pattern | Matches |
|---------|---------|
| `*.rs` | All .rs files in root |
| `**/*.rs` | All .rs files recursively |
| `src/**/*.rs` | All .rs files under src/ |
| `src/*.{rs,ts}` | .rs and .ts files in src/ |
| `!**/test_*.rs` | Exclude test files |

## Implementation with clap

```rust
use clap::{Parser, Subcommand};

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
        lang: Vec<Language>,
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
        lang: Vec<Language>,
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
        encoding: Encoding,

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
}
```

## Error Output

Errors go to stderr, allowing stdout to be piped:

```bash
pith codemap ./nonexistent 2>/dev/null | llm "..."
```

Error format:
```
error: path not found: ./nonexistent
```

With `--json`, errors are also JSON:
```json
{ "error": "path not found: ./nonexistent" }
```

## Shell Completion

Generate shell completions:

```bash
# Bash
pith --generate-completion bash > /etc/bash_completion.d/pith

# Zsh
pith --generate-completion zsh > ~/.zfunc/_pith

# Fish
pith --generate-completion fish > ~/.config/fish/completions/pith.fish
```

## Examples

### Quick Codebase Overview

```bash
pith tree . --no-metadata
```

### Get API Surface

```bash
pith codemap . --lang rust,typescript
```

### Full Context for LLM

```bash
pith context . | pbcopy  # macOS
pith context . | xclip   # Linux
```

### Token Budget Check

```bash
pith tokens . --per-file | sort -t: -k2 -n -r | head -10
```

### JSON Processing

```bash
# Get all function names
pith codemap . --json | jq '.codemaps[].declarations[] | select(.kind == "function") | .name'

# Count declarations per file
pith codemap . --json | jq '.codemaps | map({path: .path, count: (.declarations | length)})'
```
