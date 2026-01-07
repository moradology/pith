# Token Counting Specification

## Purpose

Estimate token counts for text content to manage LLM context budgets. Uses tiktoken-rs for accurate counts with fallback to heuristics.

## Types

### Encoding

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Encoding {
    #[default]
    Cl100kBase,  // GPT-4, ChatGPT
    O200kBase,   // GPT-4o
}
```

| Encoding | Model | Description |
|----------|-------|-------------|
| `Cl100kBase` | GPT-4, GPT-3.5 | Standard ChatGPT tokenizer |
| `O200kBase` | GPT-4o | Newer tokenizer, slightly different |

### TokenCounter

```rust
pub struct TokenCounter {
    encoding: Encoding,
    tokenizer: Option<CoreBPE>,  // Lazily initialized
}
```

## Functions

### count_tokens

```rust
pub fn count_tokens(text: &str) -> usize
```

**Preconditions:**
- `text` is valid UTF-8

**Postconditions:**
- Returns approximate token count
- Uses default encoding (Cl100kBase)

### count_tokens_with_encoding

```rust
pub fn count_tokens_with_encoding(text: &str, encoding: Encoding) -> usize
```

**Preconditions:**
- `text` is valid UTF-8

**Postconditions:**
- Returns token count using specified encoding
- Falls back to heuristic if tiktoken unavailable

### TokenCounter Methods

```rust
impl TokenCounter {
    pub fn new(encoding: Encoding) -> Self;
    pub fn count(&self, text: &str) -> usize;
    pub fn count_many(&self, texts: &[&str]) -> Vec<usize>;
}
```

**Note:** `TokenCounter` caches the tokenizer for repeated use.

## Implementation

### Primary: tiktoken-rs

```rust
use tiktoken_rs::CoreBPE;

fn tiktoken_count(text: &str, encoding: Encoding) -> Option<usize> {
    let bpe = match encoding {
        Encoding::Cl100kBase => tiktoken_rs::cl100k_base()?,
        Encoding::O200kBase => tiktoken_rs::o200k_base()?,
    };
    Some(bpe.encode_ordinary(text).len())
}
```

### Fallback: Heuristic

If tiktoken fails to initialize (missing data files, etc.), use character-based estimate:

```rust
fn fallback_count(text: &str) -> usize {
    // Rough approximation: ~4 characters per token
    (text.len() + 3) / 4
}
```

### Selection Logic

```rust
pub fn count_tokens_with_encoding(text: &str, encoding: Encoding) -> usize {
    tiktoken_count(text, encoding).unwrap_or_else(|| fallback_count(text))
}
```

## Accuracy

### tiktoken (Primary)

- Exact token count for OpenAI models
- Matches what the API would charge
- Handles special tokens correctly

### Heuristic (Fallback)

| Content Type | Heuristic | Actual | Error |
|--------------|-----------|--------|-------|
| English prose | ~4 chars/token | ~4.2 | ~5% |
| Code | ~4 chars/token | ~3.5 | ~15% |
| JSON/XML | ~4 chars/token | ~3.0 | ~25% |
| CJK text | ~4 chars/token | ~1.5 | ~60% |

**Recommendation:** Use tiktoken for accuracy. Heuristic is only for graceful degradation.

## Special Cases

### Empty String
```rust
count_tokens("") == 0
```

### Whitespace Only
```rust
count_tokens("   \n\t") == 1  // Single whitespace token (varies by encoding)
```

### Unicode

```rust
count_tokens("Hello") == 1      // Single word
count_tokens("Héllo") == 1      // Accented characters
count_tokens("こんにちは") == 3    // Japanese (more tokens per char)
```

### Code Tokens

```rust
// Code typically has more tokens per character
count_tokens("fn main() { println!(\"Hello\"); }") == 12
```

### Special Characters

```rust
count_tokens("```rust\nfn main() {}\n```") == 9
```

## Batch Counting

For efficiency when counting many strings:

```rust
impl TokenCounter {
    pub fn count_many(&self, texts: &[&str]) -> Vec<usize> {
        texts.iter().map(|t| self.count(t)).collect()
    }
}
```

**Note:** tiktoken's `encode_ordinary` is thread-safe, so this can be parallelized with rayon.

## Caching

### Tokenizer Initialization

tiktoken initialization is expensive (~50ms). Cache the tokenizer:

```rust
use std::sync::OnceLock;

static CL100K: OnceLock<Option<CoreBPE>> = OnceLock::new();

fn get_cl100k() -> Option<&'static CoreBPE> {
    CL100K.get_or_init(|| tiktoken_rs::cl100k_base().ok()).as_ref()
}
```

### Per-File Results

Don't cache token counts per file - they're cheap to recompute and caching adds complexity.

## Integration

### With Codemap

```rust
let codemap = extract_codemap(path, content, language, options);
let rendered = render_codemap(&codemap);
codemap.token_count = count_tokens(&rendered);
```

### With Output

```rust
let output = format_output(&tree, &codemaps, &selection);
let total_tokens = count_tokens(&output.file_map)
    + count_tokens(&output.codemaps)
    + count_tokens(&output.selected_files);
```

## CLI Integration

### tokens Subcommand

```
pith tokens <PATH> [OPTIONS]

Options:
  --encoding <ENCODING>  Token encoding [default: cl100k] [possible values: cl100k, o200k]
  --per-file             Show tokens per file instead of total
  --json                 Output as JSON
```

### Example Output

**Default (total):**
```
Total tokens: 12,345
```

**Per-file:**
```
src/lib.rs: 2,345
src/main.rs: 1,234
src/utils/helpers.rs: 567
Total: 4,146
```

**JSON:**
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

## Error Handling

Token counting should never fail - always return a value:

```rust
pub fn count_tokens(text: &str) -> usize {
    // Try tiktoken first
    if let Some(count) = tiktoken_count(text, Encoding::Cl100kBase) {
        return count;
    }

    // Fall back to heuristic
    fallback_count(text)
}
```

No `Result` type - always succeeds with best-effort count.

## Performance

| Operation | Time | Notes |
|-----------|------|-------|
| Tokenizer init | ~50ms | Once per encoding |
| Count 1KB | ~0.1ms | After init |
| Count 1MB | ~10ms | After init |
| Fallback 1KB | ~0.01ms | Character count only |

**Recommendation:** Initialize tokenizer once at startup, reuse for all counts.

## Examples

### Basic Usage

```rust
let tokens = count_tokens("Hello, world!");
assert_eq!(tokens, 4);
```

### With Encoding

```rust
let tokens = count_tokens_with_encoding(
    "Hello, world!",
    Encoding::O200kBase
);
```

### Reusable Counter

```rust
let counter = TokenCounter::new(Encoding::Cl100kBase);
let count1 = counter.count("First text");
let count2 = counter.count("Second text");
let counts = counter.count_many(&["a", "b", "c"]);
```

### In Pipeline

```rust
walk(path)
    .filter_map(|e| e.ok())
    .filter(|e| e.file_type == FileType::File)
    .map(|e| {
        let content = fs::read_to_string(&e.path).ok()?;
        let tokens = count_tokens(&content);
        Some((e.path, tokens))
    })
    .filter_map(|x| x)
    .collect::<Vec<_>>()
```
