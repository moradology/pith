# Pith: Architecture Overview

## Purpose

Pith generates optimized context representations of codebases for LLM consumption. It walks directory trees, extracts API signatures ("codemaps") from source files, and produces structured output suitable for feeding to language models.

## Core Principles

1. **Functional first**: Data pipelines, streaming iterators, pure functions
2. **Lazy evaluation**: Parse files only when needed
3. **Parallel processing**: Use rayon for multi-core extraction
4. **Token-aware**: All output includes token counts for budget management

## Data Flow

```
Directory Path
      │
      ▼
┌─────────────┐
│   Walker    │  Streaming iterator over files
│  (ignore)   │  Respects .gitignore, .pithignore
└─────────────┘
      │
      ▼
┌─────────────┐
│   Filter    │  Blocklist → Allowlist → Heuristics
│             │  Skip binaries, minified, generated
└─────────────┘
      │
      ▼
┌─────────────┐
│  Codemap    │  tree-sitter extraction per language
│  Extractor  │  Rust, TypeScript, JavaScript, Python, Go
└─────────────┘
      │
      ▼
┌─────────────┐
│   Output    │  XML-style or JSON
│  Formatter  │  Tree, codemaps, token counts
└─────────────┘
      │
      ▼
    stdout
```

## Module Dependency Graph

```
tokens.rs ──────────────────────────────────┐
    │                                       │
filter.rs ◄─────────────────────────────────┤
    │                                       │
tree.rs ────────────────────────────────────┤
    │                                       │
walker.rs ◄── tree.rs                       │
    │                                       │
codemap/                                    │
├── mod.rs ◄── tokens.rs                    │
├── rust.rs                                 │
├── typescript.rs                           │
├── javascript.rs                           │
├── python.rs                               │
└── go.rs                                   │
    │                                       │
output.rs ◄── tree.rs, codemap/mod.rs       │
    │                                       │
builder.rs ◄── walker.rs, filter.rs,        │
              codemap/, output.rs ──────────┘
    │
lib.rs (re-exports)
    │
bin/pith.rs (CLI)
```

## Key Types

### FileNode (tree.rs)
Represents a file or directory in the tree structure.

### Codemap (codemap/mod.rs)
Extracted API surface of a source file: imports, declarations with signatures, visibility, line locations.

### Declaration (codemap/mod.rs)
A single extracted item: function, struct, enum, trait, type alias, or const.

## Processing Model

### Streaming
- `walk()` returns `impl Iterator<Item = WalkEntry>`
- Files processed one at a time, memory-efficient
- No full tree held in memory

### Parallelism
- Use `par_bridge()` from rayon for parallel codemap extraction
- Each file parsed independently
- Results collected into final output

### Error Handling
- Parse failures become placeholders, not errors
- Propagate with `?`, no catch-all handlers
- Module-specific error types via `thiserror`

## API Styles

### Function Composition
```rust
walk(path)
    .filter(is_supported)
    .filter(passes_heuristics)
    .par_bridge()
    .filter_map(|f| extract_codemap(&f).ok())
    .collect::<Vec<_>>()
```

### Builder (convenience wrapper)
```rust
Pith::new(path)
    .languages(&["rust", "typescript"])
    .include_docs(true)
    .extract()
    .to_json()
```

## Output Modes

| Command | Output |
|---------|--------|
| `pith tree <PATH>` | File tree with metadata |
| `pith codemap <PATH>` | Extracted API signatures |
| `pith context <PATH>` | Full context (tree + codemaps) |
| `pith tokens <PATH>` | Token counts per file |
| `pith languages` | Supported extractors |

## Token Counting

Uses tiktoken-rs with configurable encoding:
- `cl100k_base` (default): GPT-4, ChatGPT
- `o200k_base`: GPT-4o

Fallback to `len / 4` if tiktoken unavailable.

## Ignore Patterns

Priority order:
1. `.gitignore` (handled by `ignore` crate)
2. `.git/info/exclude`
3. Global gitignore
4. `.pithignore` (pith-specific exclusions)
