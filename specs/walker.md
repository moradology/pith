# Walker Specification

## Purpose

Stream directory contents as an iterator, respecting ignore patterns and providing file metadata.

## Types

### WalkEntry

```rust
pub struct WalkEntry {
    pub path: PathBuf,
    pub depth: usize,
    pub file_type: FileType,
    pub metadata: Option<Metadata>,
}

pub enum FileType {
    File,
    Directory,
    Symlink,
}

pub struct Metadata {
    pub size: u64,
    pub modified: Option<SystemTime>,
}
```

| Field | Type | Description |
|-------|------|-------------|
| `path` | `PathBuf` | Absolute path to the entry |
| `depth` | `usize` | Depth from root (root = 0) |
| `file_type` | `FileType` | Type of filesystem entry |
| `metadata` | `Option<Metadata>` | File metadata, None if unavailable |

### WalkOptions

```rust
pub struct WalkOptions {
    pub max_depth: Option<usize>,
    pub follow_symlinks: bool,
    pub include_hidden: bool,
    pub respect_gitignore: bool,
    pub custom_ignores: Vec<PathBuf>,
}
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `max_depth` | `Option<usize>` | `None` | Maximum recursion depth |
| `follow_symlinks` | `bool` | `false` | Follow symbolic links |
| `include_hidden` | `bool` | `false` | Include hidden files/dirs |
| `respect_gitignore` | `bool` | `true` | Respect .gitignore patterns |
| `custom_ignores` | `Vec<PathBuf>` | `[]` | Additional ignore files |

## Functions

### walk

```rust
pub fn walk(root: &Path) -> impl Iterator<Item = Result<WalkEntry, WalkError>>
```

**Preconditions:**
- `root` exists and is a directory

**Postconditions:**
- Returns streaming iterator over all entries
- Entries are yielded depth-first
- Respects default ignore patterns

**Errors per item:**
- `WalkError::PermissionDenied` if entry unreadable
- `WalkError::IoError` for other I/O failures

### walk_with_options

```rust
pub fn walk_with_options(
    root: &Path,
    options: WalkOptions
) -> impl Iterator<Item = Result<WalkEntry, WalkError>>
```

**Preconditions:**
- `root` exists and is a directory

**Postconditions:**
- Honors all options
- Custom ignore files loaded from `custom_ignores`

### build_tree

```rust
pub fn build_tree(root: &Path) -> Result<FileNode, WalkError>
```

**Preconditions:**
- `root` exists

**Postconditions:**
- Returns complete tree structure
- All children populated recursively

**Note:** This loads full tree into memory. For large directories, prefer `walk()`.

## Ignore Pattern Handling

### Priority Order (highest to lowest)

1. Command-line excludes (not handled here, passed as options)
2. `.pithignore` in current or parent directories
3. `.gitignore` in current or parent directories
4. `.git/info/exclude`
5. Global gitignore (`~/.config/git/ignore`)

### Pattern Syntax

Uses gitignore syntax via the `ignore` crate:

| Pattern | Matches |
|---------|---------|
| `*.log` | All .log files |
| `/build` | build/ at root only |
| `build/` | Any build/ directory |
| `!important.log` | Negation (include despite previous rules) |
| `**/temp` | temp in any directory |

### .pithignore

Additional ignore file specific to pith. Same syntax as .gitignore.

Example `.pithignore`:
```
# Ignore vendored dependencies
vendor/

# Ignore large generated files
*.generated.ts

# But include this specific one
!important.generated.ts
```

## Traversal Order

### Depth-First
Entries are yielded in depth-first order:

```
project/
├── src/
│   ├── main.rs
│   └── lib.rs
└── tests/
    └── test.rs
```

Yields: `project/`, `project/src/`, `project/src/main.rs`, `project/src/lib.rs`, `project/tests/`, `project/tests/test.rs`

### No Guaranteed Order Within Directory
Files within a single directory may be in any order. Use sorting at render time.

## Symlink Handling

### Default (follow_symlinks = false)
- Symlinks are reported as `FileType::Symlink`
- Target is not followed
- Avoids infinite loops

### With follow_symlinks = true
- Symlinks resolved to their target
- Loop detection prevents infinite recursion
- Broken symlinks reported as errors

## Hidden Files

### Definition
- Files/directories starting with `.`
- On Windows, files with hidden attribute

### Default (include_hidden = false)
- `.git`, `.gitignore`, etc. are skipped
- `.pithignore` is read but not yielded

### With include_hidden = true
- All hidden files included
- Still respects ignore patterns

## Error Handling

### Per-Entry Errors
Errors are yielded as `Err(WalkError)` items, not propagated up. This allows partial traversal:

```rust
for entry in walk(path) {
    match entry {
        Ok(e) => process(e),
        Err(e) => eprintln!("Warning: {}", e),
    }
}
```

### Fatal Errors
Only the root path being invalid is fatal:

```rust
pub fn walk(root: &Path) -> Result<impl Iterator<...>, WalkError>
//                          ^^^^^^ Fatal error returned here
```

Actually, prefer returning an iterator that yields errors:

```rust
pub fn walk(root: &Path) -> impl Iterator<Item = Result<WalkEntry, WalkError>>
```

If root is invalid, first item is an error.

## Implementation Notes

### Using the `ignore` Crate

```rust
use ignore::WalkBuilder;

pub fn walk(root: &Path) -> impl Iterator<Item = Result<WalkEntry, WalkError>> {
    WalkBuilder::new(root)
        .hidden(false)      // We handle hidden ourselves
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .build()
        .filter_map(|result| {
            match result {
                Ok(entry) => Some(Ok(WalkEntry::from(entry))),
                Err(e) => Some(Err(WalkError::from(e))),
            }
        })
}
```

### Adding .pithignore Support

```rust
let mut builder = WalkBuilder::new(root);

// Add .pithignore if it exists
if let Some(pithignore) = find_pithignore(root) {
    builder.add_ignore(pithignore);
}
```

## Examples

### Basic Walk

```rust
for entry in walk("./project") {
    let entry = entry?;
    if entry.file_type == FileType::File {
        println!("{}", entry.path.display());
    }
}
```

### With Options

```rust
let options = WalkOptions {
    max_depth: Some(3),
    follow_symlinks: false,
    include_hidden: false,
    respect_gitignore: true,
    custom_ignores: vec![PathBuf::from(".pithignore")],
};

for entry in walk_with_options("./project", options) {
    // ...
}
```

### Building Full Tree

```rust
let tree = build_tree("./project")?;
let rendered = render_tree(&tree, &options);
println!("{}", rendered);
```

## Edge Cases

### Empty Directory
- `walk()` yields only the root directory entry
- `build_tree()` returns a `FileNode` with empty `children`

### Single File
- If `root` is a file, yield only that file
- `build_tree()` returns a `FileNode` with `kind = File`

### Permission Denied
- Skip the entry, yield error
- Continue with siblings

### Circular Symlinks
- Detected via path tracking
- Yield error, don't follow

### Very Deep Directories
- No stack overflow (use iterative, not recursive)
- If `max_depth` set, stop at that level
