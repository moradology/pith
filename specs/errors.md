# Error Handling Specification

## Principles

1. **No panics in library code** - All fallible operations return `Result`
2. **Propagate with `?`** - No try-catch patterns, no `.unwrap()` except in tests
3. **Module-specific errors** - Each module defines its own error type
4. **Graceful degradation** - Parse failures produce placeholders, not errors
5. **User-friendly messages** - Errors should be actionable

## Error Types

### Top-Level

```rust
#[derive(Debug, thiserror::Error)]
pub enum PithError {
    #[error("Path not found: {path}")]
    PathNotFound { path: PathBuf },

    #[error("Permission denied: {path}")]
    PermissionDenied { path: PathBuf },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Walk error: {0}")]
    Walk(#[from] WalkError),

    #[error("Filter error: {0}")]
    Filter(#[from] FilterError),

    #[error("Codemap error: {0}")]
    Codemap(#[from] CodemapError),

    #[error("Output error: {0}")]
    Output(#[from] OutputError),
}
```

### Walker Module

```rust
#[derive(Debug, thiserror::Error)]
pub enum WalkError {
    #[error("Root path not found: {path}")]
    RootNotFound { path: PathBuf },

    #[error("Not a directory: {path}")]
    NotADirectory { path: PathBuf },

    #[error("Permission denied: {path}")]
    PermissionDenied { path: PathBuf },

    #[error("IO error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Symlink loop detected: {path}")]
    SymlinkLoop { path: PathBuf },
}
```

### Filter Module

```rust
#[derive(Debug, thiserror::Error)]
pub enum FilterError {
    #[error("Failed to read file for heuristics: {path}")]
    ReadFailed { path: PathBuf },
}
```

### Codemap Module

```rust
#[derive(Debug, thiserror::Error)]
pub enum CodemapError {
    #[error("Failed to initialize {language} parser")]
    ParserInit { language: Language },

    #[error("Parse error in {path}: {message}")]
    Parse { path: PathBuf, message: String },

    #[error("Unsupported language for file: {path}")]
    UnsupportedLanguage { path: PathBuf },

    #[error("Failed to read file: {path}")]
    ReadFailed {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}
```

### Output Module

```rust
#[derive(Debug, thiserror::Error)]
pub enum OutputError {
    #[error("Serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
```

## Error Recovery Strategy

### Fatal Errors

These abort the operation:

| Error | Action |
|-------|--------|
| Root path not found | Return error immediately |
| Not a directory (when directory expected) | Return error immediately |
| Parser initialization failure | Return error immediately |

### Recoverable Errors

These produce warnings but continue processing:

| Error | Action |
|-------|--------|
| Permission denied on single file | Skip file, log warning |
| Parse error in file | Include file with `parse_error` set |
| Symlink loop | Skip, log warning |
| Heuristic read failure | Process file anyway |

### Example: Walk with Recovery

```rust
pub fn walk(root: &Path) -> impl Iterator<Item = Result<WalkEntry, WalkError>> {
    // Fatal check at start
    if !root.exists() {
        return std::iter::once(Err(WalkError::RootNotFound { path: root.to_owned() }));
    }

    // Iterator yields results, caller decides how to handle errors
    WalkBuilder::new(root)
        .build()
        .map(|result| match result {
            Ok(entry) => Ok(WalkEntry::from(entry)),
            Err(e) => Err(WalkError::from(e)),
        })
}

// Caller can filter or collect errors
let entries: Vec<WalkEntry> = walk(path)
    .filter_map(|r| match r {
        Ok(e) => Some(e),
        Err(e) => {
            eprintln!("Warning: {}", e);
            None
        }
    })
    .collect();
```

### Example: Codemap with Placeholder

```rust
pub fn extract_codemap(
    path: &Path,
    content: &str,
    language: Language,
    options: &ExtractOptions,
) -> Codemap {
    let mut codemap = Codemap {
        path: path.to_owned(),
        language,
        imports: vec![],
        declarations: vec![],
        token_count: 0,
        parse_error: None,
    };

    let parser_result = create_parser(language);
    let parser = match parser_result {
        Ok(p) => p,
        Err(e) => {
            codemap.parse_error = Some(format!("Parser init failed: {}", e));
            return codemap;
        }
    };

    match parser.parse(content) {
        Ok(tree) => {
            codemap.imports = extract_imports(&tree, content);
            codemap.declarations = extract_declarations(&tree, content, options);
        }
        Err(e) => {
            codemap.parse_error = Some(format!("Parse failed: {}", e));
        }
    }

    codemap.token_count = count_tokens(&render_codemap(&codemap));
    codemap
}
```

## Error Display

### Human-Readable (Default)

```
error: path not found: ./nonexistent

error: permission denied while reading: ./secret/file.rs
  Skipped 3 files due to permission errors.

warning: parse error in src/broken.rs: unexpected token at line 42
  Continuing with partial extraction.
```

### JSON Format

When `--json` is specified:

```json
{
  "error": {
    "type": "path_not_found",
    "path": "./nonexistent",
    "message": "Path not found: ./nonexistent"
  }
}
```

With warnings:

```json
{
  "codemaps": [...],
  "warnings": [
    {
      "type": "parse_error",
      "path": "src/broken.rs",
      "message": "unexpected token at line 42"
    }
  ]
}
```

## CLI Error Handling

```rust
fn main() {
    if let Err(e) = run() {
        eprintln!("error: {}", e);
        std::process::exit(exit_code_for(&e));
    }
}

fn exit_code_for(e: &PithError) -> i32 {
    match e {
        PithError::PathNotFound { .. } => 3,
        PithError::PermissionDenied { .. } => 4,
        PithError::Io(_) => 1,
        _ => 1,
    }
}
```

## Context and Cause Chains

Use `#[source]` for error chains:

```rust
#[derive(Debug, thiserror::Error)]
pub enum WalkError {
    #[error("IO error at {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

// Usage
let file = File::open(&path).map_err(|e| WalkError::Io {
    path: path.clone(),
    source: e,
})?;
```

Display with cause chain:

```
error: IO error at ./foo/bar.rs
  Caused by: No such file or directory (os error 2)
```

## Logging vs Errors

### Use Errors For:
- Conditions that prevent the requested operation
- Invalid input
- Required resources not available

### Use Logging/Warnings For:
- Skipped files (permission, filter)
- Partial results (parse errors with recovery)
- Performance concerns

### Stderr vs Return Value

- **stderr**: Warnings during processing
- **Return value (exit code)**: Final success/failure status

## Testing Errors

```rust
#[test]
fn test_walk_nonexistent_path() {
    let result = walk(Path::new("/nonexistent"));
    let first = result.next().unwrap();
    assert!(matches!(first, Err(WalkError::RootNotFound { .. })));
}

#[test]
fn test_codemap_parse_error_recovery() {
    let content = "fn broken( { }";  // Invalid Rust
    let codemap = extract_codemap(
        Path::new("test.rs"),
        content,
        Language::Rust,
        &ExtractOptions::default(),
    );

    assert!(codemap.parse_error.is_some());
    assert!(codemap.declarations.is_empty());
}
```

## Error Messages

### Good:
```
error: path not found: ./src/missing.rs
error: permission denied: ./private/secret.rs
error: parse error in lib.rs at line 42: unexpected '}'
```

### Bad:
```
error: Error(Kind(NotFound))
error: failed
error: something went wrong
```

### Guidelines:
1. Include the path when relevant
2. Include line numbers for parse errors
3. Suggest remediation when possible
4. Be specific about what failed

## Summary

| Module | Error Type | Recovery |
|--------|------------|----------|
| walker | `WalkError` | Skip file, continue |
| filter | `FilterError` | Process anyway |
| codemap | `CodemapError` | Placeholder with `parse_error` |
| output | `OutputError` | Fatal |
| tokens | N/A | Always succeeds with fallback |
