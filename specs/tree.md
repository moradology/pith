# Tree Specification

## Purpose

Represent and render directory structures with file metadata and selection markers.

## Types

### NodeKind

```rust
pub enum NodeKind {
    Directory,
    File {
        extension: Option<String>,
        size: u64,
        lines: Option<usize>,
    },
}
```

| Field | Type | Description |
|-------|------|-------------|
| `extension` | `Option<String>` | File extension without dot, e.g., `"rs"`, `"tsx"` |
| `size` | `u64` | File size in bytes |
| `lines` | `Option<usize>` | Line count, computed lazily on first access |

### FileNode

```rust
pub struct FileNode {
    pub name: String,
    pub path: PathBuf,
    pub kind: NodeKind,
    pub children: Vec<FileNode>,
}
```

| Field | Type | Description |
|-------|------|-------------|
| `name` | `String` | File or directory name (not full path) |
| `path` | `PathBuf` | Absolute or relative path from root |
| `kind` | `NodeKind` | Whether this is a file or directory |
| `children` | `Vec<FileNode>` | Child nodes, empty for files |

## Functions

### from_path

```rust
pub fn from_path(path: &Path) -> Result<FileNode, TreeError>
```

**Preconditions:**
- `path` exists and is readable

**Postconditions:**
- Returns `FileNode` with `kind` set appropriately
- `children` is empty (not yet populated)

**Errors:**
- `TreeError::NotFound` if path doesn't exist
- `TreeError::PermissionDenied` if unreadable

### with_children

```rust
pub fn with_children(self, children: Vec<FileNode>) -> Self
```

**Preconditions:**
- `self.kind` is `Directory`

**Postconditions:**
- Returns new `FileNode` with children set

### is_file / is_directory

```rust
pub fn is_file(&self) -> bool
pub fn is_directory(&self) -> bool
```

**Postconditions:**
- Returns `true` if kind matches, `false` otherwise

## Rendering

### render_tree

```rust
pub fn render_tree(
    root: &FileNode,
    options: &RenderOptions,
) -> String
```

### RenderOptions

```rust
pub struct RenderOptions {
    pub show_size: bool,
    pub show_lines: bool,
    pub show_language: bool,
    pub selected: HashSet<PathBuf>,
    pub has_codemap: HashSet<PathBuf>,
}
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `show_size` | `bool` | `true` | Show file sizes |
| `show_lines` | `bool` | `true` | Show line counts |
| `show_language` | `bool` | `true` | Show detected language |
| `selected` | `HashSet<PathBuf>` | empty | Paths to mark with `*` |
| `has_codemap` | `HashSet<PathBuf>` | empty | Paths to mark with `+` |

### Box-Drawing Characters

| Character | Unicode | Usage |
|-----------|---------|-------|
| `├` | U+251C | Non-last child |
| `└` | U+2514 | Last child |
| `│` | U+2502 | Continuation |
| `─` | U+2500 | Horizontal line |

### Rendering Rules

1. **Directories first**: Sort children with directories before files
2. **Alphabetical**: Within each group, sort alphabetically (case-insensitive)
3. **Markers**: Append ` *` for selected, ` +` for has-codemap, ` *+` for both
4. **Metadata**: Show in brackets `[language, lines, size]` if enabled

### Output Format

**Without metadata:**
```
src/
├── lib.rs *+
├── main.rs *
└── utils/
    └── helpers.rs +
```

**With metadata:**
```
src/
├── lib.rs [rust, 245 lines, 2.3KB] *+
├── main.rs [rust, 89 lines, 1.1KB] *
└── utils/
    └── helpers.rs [rust, 42 lines, 512B] +
```

### Size Formatting

| Size | Display |
|------|---------|
| < 1024 | `{n}B` |
| < 1024 * 1024 | `{n}KB` (1 decimal) |
| >= 1024 * 1024 | `{n}MB` (1 decimal) |

## Edge Cases

### Empty Directory
```
empty_dir/
```

### Single File at Root
```
file.rs [rust, 10 lines, 128B]
```

### Deep Nesting
```
a/
└── b/
    └── c/
        └── d/
            └── file.rs
```

### Special Characters in Names
- Spaces: Render as-is
- Unicode: Render as-is
- Control characters: Replace with `?`

## Sorting Algorithm

```rust
fn sort_children(children: &mut [FileNode]) {
    children.sort_by(|a, b| {
        match (&a.kind, &b.kind) {
            (NodeKind::Directory, NodeKind::File { .. }) => Ordering::Less,
            (NodeKind::File { .. }, NodeKind::Directory) => Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        }
    });
}
```

## Examples

### Input
```rust
let root = FileNode {
    name: "project".into(),
    path: "project".into(),
    kind: NodeKind::Directory,
    children: vec![
        FileNode { name: "src".into(), kind: NodeKind::Directory, children: vec![
            FileNode { name: "main.rs".into(), kind: NodeKind::File { extension: Some("rs".into()), size: 1100, lines: Some(89) }, children: vec![] },
            FileNode { name: "lib.rs".into(), kind: NodeKind::File { extension: Some("rs".into()), size: 2300, lines: Some(245) }, children: vec![] },
        ], ..},
        FileNode { name: "Cargo.toml".into(), kind: NodeKind::File { extension: Some("toml".into()), size: 512, lines: Some(25) }, children: vec![] },
    ],
};

let options = RenderOptions {
    show_size: true,
    show_lines: true,
    show_language: true,
    selected: ["project/src/main.rs", "project/src/lib.rs"].into_iter().map(PathBuf::from).collect(),
    has_codemap: ["project/src/lib.rs"].into_iter().map(PathBuf::from).collect(),
};
```

### Output
```
project/
├── src/
│   ├── lib.rs [rust, 245 lines, 2.3KB] *+
│   └── main.rs [rust, 89 lines, 1.1KB] *
└── Cargo.toml [toml, 25 lines, 512B]
```
