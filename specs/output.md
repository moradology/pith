# Output Specification

## Purpose

Format extracted codemaps, file trees, and metadata into human-readable and machine-parseable formats suitable for LLM consumption.

## Output Formats

### XML-style (Default)

Human and LLM readable, uses XML-like tags for structure.

### JSON

Machine-parseable, suitable for programmatic access.

## Types

### OutputFormat

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    #[default]
    Xml,
    Json,
}
```

### OutputOptions

```rust
pub struct OutputOptions {
    pub format: OutputFormat,
    pub include_tree: bool,
    pub include_codemaps: bool,
    pub include_selected_files: bool,
    pub include_summary: bool,
    pub public_only: bool,
}
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `format` | `OutputFormat` | `Xml` | Output format |
| `include_tree` | `bool` | `true` | Include file tree |
| `include_codemaps` | `bool` | `true` | Include codemaps |
| `include_selected_files` | `bool` | `false` | Include full file contents |
| `include_summary` | `bool` | `true` | Include token summary |
| `public_only` | `bool` | `true` | Only show public items |

## XML-Style Format

### File Tree Section

```xml
<file_map>
project/
├── src/
│   ├── lib.rs [rust, 245 lines, 2.3KB] *+
│   ├── main.rs [rust, 89 lines, 1.1KB] *
│   └── utils/
│       └── helpers.rs [rust, 42 lines, 512B] +
├── tests/
│   └── integration.rs [rust, 156 lines, 1.8KB]
└── Cargo.toml [toml, 25 lines, 512B] *

Legend: * = selected, + = has codemap
</file_map>
```

### Codemaps Section

```xml
<codemaps>
## src/lib.rs

### Imports
- use std::collections::HashMap
- use anyhow::Result

### Declarations

#### pub struct Config (lines 7-10)
Fields:
- pub name: String
- pub timeout: Duration

Methods:
- pub fn new() -> Self (line 13)
- pub fn validate(&self) -> bool (line 17)

#### pub async fn process<T: Display>(config: &Config, data: T) -> Result<Output> where T: Clone + Send (lines 22-27)
Process data according to config.

---

## src/utils/helpers.rs

### Imports
- use std::fs

### Declarations

#### pub fn read_file(path: &Path) -> io::Result<String> (lines 5-8)

</codemaps>
```

### Selected Files Section

```xml
<selected_files>
--- src/lib.rs (245 lines, 2,345 tokens) ---
use std::collections::HashMap;
use anyhow::Result;

/// Configuration for the processor
#[derive(Debug, Clone)]
pub struct Config {
    pub name: String,
    pub timeout: Duration,
}

impl Config {
    /// Create a new config with defaults
    pub fn new() -> Self {
        Self { name: "default".into(), timeout: Duration::from_secs(30) }
    }

    fn validate(&self) -> bool {
        !self.name.is_empty()
    }
}

// ... rest of file ...

--- src/main.rs (89 lines, 1,234 tokens) ---
// ... file content ...

</selected_files>
```

### Token Summary Section

```xml
<token_summary>
Component breakdown:
- File tree: 150 tokens
- Codemaps: 1,245 tokens
- Selected files: 3,579 tokens

Total: 4,974 tokens

Per-file breakdown:
- src/lib.rs: 2,345 tokens (selected)
- src/main.rs: 1,234 tokens (selected)
- src/utils/helpers.rs: 312 tokens (codemap only)
- Cargo.toml: 89 tokens (selected)
</token_summary>
```

## JSON Format

### Full Output Structure

```json
{
  "tree": {
    "name": "project",
    "path": "project",
    "kind": "directory",
    "children": [
      {
        "name": "src",
        "path": "project/src",
        "kind": "directory",
        "children": [
          {
            "name": "lib.rs",
            "path": "project/src/lib.rs",
            "kind": "file",
            "extension": "rs",
            "size": 2345,
            "lines": 245,
            "language": "rust",
            "selected": true,
            "has_codemap": true
          }
        ]
      }
    ]
  },
  "codemaps": [
    {
      "path": "project/src/lib.rs",
      "language": "rust",
      "imports": [
        { "source": "std::collections", "items": ["HashMap"] },
        { "source": "anyhow", "items": ["Result"] }
      ],
      "declarations": [
        {
          "kind": "struct",
          "name": "Config",
          "visibility": "public",
          "location": { "start_line": 7, "end_line": 10 },
          "fields": [
            { "name": "name", "type": "String", "visibility": "public" },
            { "name": "timeout", "type": "Duration", "visibility": "public" }
          ],
          "methods": [
            {
              "kind": "function",
              "name": "new",
              "signature": "pub fn new() -> Self",
              "visibility": "public",
              "location": { "start_line": 13, "end_line": 15 },
              "is_async": false,
              "doc": "Create a new config with defaults"
            }
          ],
          "doc": "Configuration for the processor"
        },
        {
          "kind": "function",
          "name": "process",
          "signature": "pub async fn process<T: Display>(config: &Config, data: T) -> Result<Output> where T: Clone + Send",
          "visibility": "public",
          "location": { "start_line": 22, "end_line": 27 },
          "is_async": true,
          "doc": "Process data according to config"
        }
      ],
      "token_count": 312,
      "parse_error": null
    }
  ],
  "selected_files": [
    {
      "path": "project/src/lib.rs",
      "content": "use std::collections::HashMap;\n...",
      "lines": 245,
      "tokens": 2345
    }
  ],
  "summary": {
    "total_tokens": 4974,
    "tree_tokens": 150,
    "codemap_tokens": 1245,
    "selected_tokens": 3579,
    "file_breakdown": {
      "project/src/lib.rs": { "tokens": 2345, "selected": true, "has_codemap": true },
      "project/src/main.rs": { "tokens": 1234, "selected": true, "has_codemap": false },
      "project/src/utils/helpers.rs": { "tokens": 312, "selected": false, "has_codemap": true }
    }
  }
}
```

### Codemap-Only JSON

When only codemaps requested (`pith codemap --json`):

```json
{
  "codemaps": [
    {
      "path": "src/lib.rs",
      "language": "rust",
      "imports": [...],
      "declarations": [...],
      "token_count": 312
    }
  ],
  "total_tokens": 312
}
```

### Tree-Only JSON

When only tree requested (`pith tree --json`):

```json
{
  "tree": {
    "name": "project",
    "kind": "directory",
    "children": [...]
  }
}
```

## Functions

### format_output

```rust
pub fn format_output(
    tree: &FileNode,
    codemaps: &[Codemap],
    selected_files: &[(PathBuf, String)],
    options: &OutputOptions,
) -> String
```

**Preconditions:**
- All paths in codemaps and selected_files exist in tree

**Postconditions:**
- Returns formatted string in requested format
- Sections omitted if options disable them

### format_tree

```rust
pub fn format_tree(
    tree: &FileNode,
    options: &TreeRenderOptions,
    format: OutputFormat,
) -> String
```

### format_codemaps

```rust
pub fn format_codemaps(
    codemaps: &[Codemap],
    options: &OutputOptions,
    format: OutputFormat,
) -> String
```

### format_summary

```rust
pub fn format_summary(
    tree_tokens: usize,
    codemap_tokens: usize,
    selected_tokens: usize,
    file_breakdown: &HashMap<PathBuf, FileTokens>,
    format: OutputFormat,
) -> String
```

## Rendering Rules

### Visibility Filtering

When `public_only = true`:

- Only show public declarations
- Private methods in impl blocks omitted
- Private fields shown with `(private)` marker

### Line Numbers

Always include in both formats:
- XML: `(lines 7-10)` or `(line 13)`
- JSON: `{ "start_line": 7, "end_line": 10 }`

### Doc Comments

When included:
- XML: Rendered below signature, indented
- JSON: `"doc": "..."` field

### Signatures

Full signatures preserved:
- Generics: `<T: Display + Clone>`
- Lifetimes: `<'a, 'b>`
- Where clauses on separate line in XML

### Import Grouping

Group imports by source module in XML:
```
### Imports
- std::collections::{HashMap, HashSet}
- anyhow::{Result, Context}
```

## Token Counting for Output

Each section contributes to total:

```rust
let tree_tokens = count_tokens(&format_tree(...));
let codemap_tokens = codemaps.iter()
    .map(|c| c.token_count)
    .sum();
let selected_tokens = selected_files.iter()
    .map(|(_, content)| count_tokens(content))
    .sum();
```

## Formatting Details

### Number Formatting

| Value | Display |
|-------|---------|
| 1234 | 1,234 |
| 1234567 | 1,234,567 |

### Size Formatting

| Bytes | Display |
|-------|---------|
| 512 | 512B |
| 1536 | 1.5KB |
| 2097152 | 2.0MB |

### Path Display

- Relative to root in output
- Forward slashes on all platforms
- No leading `./`

## Examples

### Minimal Output (Tree Only)

```xml
<file_map>
src/
├── lib.rs [rust, 245 lines]
└── main.rs [rust, 89 lines]
</file_map>
```

### Codemap Only

```xml
<codemaps>
## src/lib.rs

### Declarations

#### pub fn process(input: &str) -> Result<Output> (line 15)

</codemaps>
```

### Full Context

```xml
<file_map>
src/
├── lib.rs [rust, 245 lines, 2.3KB] *+
└── main.rs [rust, 89 lines, 1.1KB] *

Legend: * = selected, + = has codemap
</file_map>

<codemaps>
## src/lib.rs

### Imports
- use std::collections::HashMap

### Declarations

#### pub struct Config (lines 7-10)
Fields:
- pub name: String

#### pub fn new() -> Self (line 13)

</codemaps>

<selected_files>
--- src/lib.rs (245 lines, 2,345 tokens) ---
// Full file content here...

--- src/main.rs (89 lines, 1,234 tokens) ---
// Full file content here...
</selected_files>

<token_summary>
Total: 4,974 tokens
- File tree: 150 tokens
- Codemaps: 1,245 tokens
- Selected files: 3,579 tokens
</token_summary>
```

## Serde Serialization

For JSON output, use serde with appropriate derives:

```rust
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct JsonOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tree: Option<JsonTree>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub codemaps: Vec<JsonCodemap>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub selected_files: Vec<JsonSelectedFile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<JsonSummary>,
}
```

Use `serde_json::to_string_pretty` for human-readable JSON.
