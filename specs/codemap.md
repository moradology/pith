# Codemap Specification

## Purpose

Extract API signatures (function/struct/type declarations) from source files using tree-sitter. Captures the public interface without implementation bodies.

## Types

### Language

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    Rust,
    TypeScript,
    Tsx,
    JavaScript,
    Jsx,
    Python,
    Go,
}
```

### Visibility

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Visibility {
    Public,
    #[default]
    Private,
    Crate,      // Rust pub(crate)
    Protected,  // Python _ prefix convention
}
```

### Location

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Location {
    pub start_line: usize,  // 1-indexed
    pub end_line: usize,    // 1-indexed, inclusive
}
```

### Declaration

```rust
#[derive(Debug, Clone)]
pub enum Declaration {
    Function {
        name: String,
        signature: String,
        visibility: Visibility,
        location: Location,
        is_async: bool,
        doc: Option<String>,
    },
    Struct {
        name: String,
        fields: Vec<Field>,
        visibility: Visibility,
        location: Location,
        methods: Vec<Declaration>,  // impl block methods grouped here
        doc: Option<String>,
    },
    Enum {
        name: String,
        variants: Vec<String>,
        visibility: Visibility,
        location: Location,
        doc: Option<String>,
    },
    Trait {
        name: String,
        methods: Vec<String>,  // method signatures
        location: Location,
        doc: Option<String>,
    },
    TypeAlias {
        name: String,
        target: String,
        visibility: Visibility,
        location: Location,
    },
    Const {
        name: String,
        ty: String,
        visibility: Visibility,
        location: Location,
    },
    Interface {  // TypeScript
        name: String,
        members: Vec<String>,
        location: Location,
        doc: Option<String>,
    },
    Class {  // TypeScript, Python
        name: String,
        members: Vec<Declaration>,
        visibility: Visibility,
        location: Location,
        doc: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct Field {
    pub name: String,
    pub ty: String,
    pub visibility: Visibility,
}
```

### Codemap

```rust
#[derive(Debug, Clone)]
pub struct Codemap {
    pub path: PathBuf,
    pub language: Language,
    pub imports: Vec<Import>,
    pub declarations: Vec<Declaration>,
    pub token_count: usize,
    pub parse_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Import {
    pub source: String,      // Module path
    pub items: Vec<String>,  // Imported items, empty for "import *" or default
}
```

## Functions

### extract_codemap

```rust
pub fn extract_codemap(
    path: &Path,
    content: &str,
    language: Language,
    options: &ExtractOptions,
) -> Codemap
```

**Preconditions:**
- `content` is valid UTF-8
- `language` matches the file type

**Postconditions:**
- Returns `Codemap` with extracted declarations
- If parse fails, `parse_error` is set, `declarations` may be partial

### ExtractOptions

```rust
pub struct ExtractOptions {
    pub include_docs: bool,
    pub include_private: bool,
}
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `include_docs` | `bool` | `false` | Extract doc comments |
| `include_private` | `bool` | `true` | Include non-public items (capture all, filter on output) |

## Language-Specific Extraction

### Rust

#### Imports
```rust
// Extract from use declarations
use std::collections::HashMap;
use crate::utils::{foo, bar};
```
→ `[Import { source: "std::collections", items: ["HashMap"] }, Import { source: "crate::utils", items: ["foo", "bar"] }]`

#### Functions
```rust
// Extract signature without body
pub fn process<T: Display>(input: &str, config: Config) -> Result<T, Error>
where
    T: Clone,
{
    // body ignored
}
```
→ `Declaration::Function { name: "process", signature: "pub fn process<T: Display>(input: &str, config: Config) -> Result<T, Error> where T: Clone", visibility: Public, is_async: false }`

#### Async Functions
```rust
pub async fn fetch(url: &str) -> Result<Response, Error> { }
```
→ `is_async: true`, signature includes `async`

#### Structs
```rust
pub struct Config {
    pub name: String,
    timeout: Duration,
}
```
→ `Declaration::Struct { name: "Config", fields: [Field { name: "name", ty: "String", visibility: Public }, Field { name: "timeout", ty: "Duration", visibility: Private }] }`

#### Enums
```rust
pub enum Status {
    Running,
    Stopped,
    Error(String),
}
```
→ `Declaration::Enum { name: "Status", variants: ["Running", "Stopped", "Error(String)"] }`

#### Traits
```rust
pub trait Handler {
    fn handle(&self, req: Request) -> Response;
    fn name(&self) -> &str;
}
```
→ `Declaration::Trait { name: "Handler", methods: ["fn handle(&self, req: Request) -> Response", "fn name(&self) -> &str"] }`

#### Type Aliases
```rust
pub type Result<T> = std::result::Result<T, Error>;
```
→ `Declaration::TypeAlias { name: "Result<T>", target: "std::result::Result<T, Error>" }`

#### Constants
```rust
pub const MAX_SIZE: usize = 1024;
```
→ `Declaration::Const { name: "MAX_SIZE", ty: "usize" }`

#### Impl Blocks
```rust
impl Config {
    pub fn new() -> Self { }
    fn validate(&self) -> bool { }
}
```
→ Methods grouped with the `Struct` declaration:
```rust
Declaration::Struct {
    name: "Config",
    // ...
    methods: [
        Declaration::Function { name: "new", signature: "pub fn new() -> Self", visibility: Public },
        Declaration::Function { name: "validate", signature: "fn validate(&self) -> bool", visibility: Private },
    ],
}
```

### TypeScript / TSX

#### Imports
```typescript
import { useState, useEffect } from 'react';
import * as utils from './utils';
import DefaultExport from './module';
```
→
```rust
[
    Import { source: "react", items: ["useState", "useEffect"] },
    Import { source: "./utils", items: ["*"] },
    Import { source: "./module", items: ["default"] },
]
```

#### Functions
```typescript
export function process(input: string): Result<Output> { }
export async function fetchData(url: string): Promise<Response> { }
```
→ Signature includes export keyword, async keyword, full type annotations

#### Arrow Functions (exported)
```typescript
export const handler = async (req: Request): Promise<Response> => { }
```
→ `Declaration::Function { name: "handler", signature: "export const handler: async (req: Request) => Promise<Response>" }`

#### Interfaces
```typescript
export interface Config {
    name: string;
    timeout?: number;
}
```
→ `Declaration::Interface { name: "Config", members: ["name: string", "timeout?: number"] }`

#### Types
```typescript
export type Result<T> = { ok: true, value: T } | { ok: false, error: Error };
```
→ `Declaration::TypeAlias { name: "Result<T>", target: "{ ok: true, value: T } | { ok: false, error: Error }" }`

#### Classes
```typescript
export class Handler {
    constructor(private config: Config) {}
    async handle(req: Request): Promise<Response> {}
}
```
→ `Declaration::Class { name: "Handler", members: [Function { name: "constructor"... }, Function { name: "handle"... }] }`

#### React Components (TSX)
```tsx
export function Button({ label, onClick }: ButtonProps): JSX.Element { }
export const Card: React.FC<CardProps> = ({ children }) => { }
```
→ Extracted as regular functions

### JavaScript / JSX

Same as TypeScript but without type annotations. Infer what we can:

```javascript
export function process(input) { }
export async function fetchData(url) { }
export class Handler { }
```

### Python

#### Imports
```python
from typing import List, Optional
import os
from .utils import helper
```
→
```rust
[
    Import { source: "typing", items: ["List", "Optional"] },
    Import { source: "os", items: [] },
    Import { source: ".utils", items: ["helper"] },
]
```

#### Functions
```python
def process(input: str) -> Result:
    """Process the input data."""
    pass

async def fetch_data(url: str) -> Response:
    pass
```
→ Full signature with type hints, docstring extracted if `include_docs`

#### Classes
```python
class Handler:
    """Handle requests."""

    def __init__(self, config: Config) -> None:
        pass

    async def handle(self, req: Request) -> Response:
        pass

    def _private_method(self) -> None:
        pass
```
→ `Declaration::Class` with methods, `_private_method` gets `visibility: Protected`

#### Decorators
```python
@dataclass
class Config:
    name: str
    timeout: int = 30
```
→ Include decorator in signature/metadata

### Go

#### Imports
```go
import (
    "fmt"
    "net/http"
)
```
→ `[Import { source: "fmt", items: [] }, Import { source: "net/http", items: [] }]`

#### Functions
```go
func Process(input string) (Result, error) { }
func (h *Handler) Handle(req Request) Response { }
```
→ Include receiver in signature for methods

#### Structs
```go
type Config struct {
    Name    string
    Timeout time.Duration
}
```
→ Exported (capitalized) = Public, unexported = Private

#### Interfaces
```go
type Handler interface {
    Handle(req Request) Response
    Name() string
}
```
→ `Declaration::Interface { name: "Handler", members: [...] }`

#### Type Aliases
```go
type Result = struct {
    Ok    bool
    Value interface{}
}
```

## Tree-Sitter Queries

### Rust Query Patterns

```scheme
; Functions
(function_item
  (visibility_modifier)? @visibility
  name: (identifier) @name
) @function

; Structs
(struct_item
  (visibility_modifier)? @visibility
  name: (type_identifier) @name
  body: (field_declaration_list)? @fields
) @struct

; Enums
(enum_item
  (visibility_modifier)? @visibility
  name: (type_identifier) @name
  body: (enum_variant_list) @variants
) @enum

; Traits
(trait_item
  name: (type_identifier) @name
  body: (declaration_list) @body
) @trait

; Impl blocks
(impl_item
  type: (type_identifier) @impl_for
  body: (declaration_list) @methods
) @impl

; Use statements
(use_declaration
  argument: (_) @path
) @use
```

### TypeScript Query Patterns

```scheme
; Functions
(function_declaration
  name: (identifier) @name
) @function

(export_statement
  declaration: (function_declaration) @function
) @export

; Interfaces
(interface_declaration
  name: (type_identifier) @name
  body: (object_type) @body
) @interface

; Classes
(class_declaration
  name: (type_identifier) @name
  body: (class_body) @body
) @class

; Type aliases
(type_alias_declaration
  name: (type_identifier) @name
  value: (_) @value
) @type_alias
```

## Doc Comment Extraction

### Rust
```rust
/// This is a doc comment
/// It can span multiple lines
pub fn process() {}
```
Extract `outer_doc_comment` nodes immediately preceding declarations.

### TypeScript/JavaScript
```typescript
/**
 * JSDoc comment
 * @param input - The input string
 */
export function process(input: string) {}
```
Extract `comment` nodes with `/**` prefix.

### Python
```python
def process():
    """
    Docstring here.

    Args:
        input: The input string
    """
    pass
```
Extract first `expression_statement` in function body if it's a string.

### Go
```go
// Process handles the input.
// It returns the processed result.
func Process(input string) Result {}
```
Extract `//` comments immediately preceding declarations.

## Signature Formatting Rules

### Keep Full Signatures
- Generic parameters: `<T: Display + Clone>`
- Lifetime annotations: `<'a, 'b>`
- Where clauses: `where T: Clone + Send`
- Default values: `fn new(x: i32 = 0)`

### Strip Bodies
- Remove everything after `{` for functions
- Keep `{ ... }` placeholder for struct/enum bodies in output

### Normalize Whitespace
- Single spaces between tokens
- No trailing whitespace
- Preserve newlines in where clauses

## Parse Error Handling

If tree-sitter fails to parse:

1. Set `parse_error` field with error message
2. Extract what was successfully parsed (partial results)
3. Continue with other files

```rust
Codemap {
    path: "broken.rs".into(),
    language: Rust,
    imports: vec![],  // May be empty or partial
    declarations: vec![],  // May be empty or partial
    token_count: 0,
    parse_error: Some("Syntax error at line 42: unexpected token".into()),
}
```

## Examples

### Rust Input
```rust
//! Module documentation

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

/// Process data according to config
pub async fn process<T: Display>(config: &Config, data: T) -> Result<Output>
where
    T: Clone + Send,
{
    todo!()
}
```

### Extracted Codemap
```rust
Codemap {
    path: "src/lib.rs".into(),
    language: Rust,
    imports: vec![
        Import { source: "std::collections".into(), items: vec!["HashMap".into()] },
        Import { source: "anyhow".into(), items: vec!["Result".into()] },
    ],
    declarations: vec![
        Declaration::Struct {
            name: "Config".into(),
            fields: vec![
                Field { name: "name".into(), ty: "String".into(), visibility: Public },
                Field { name: "timeout".into(), ty: "Duration".into(), visibility: Public },
            ],
            visibility: Public,
            location: Location { start_line: 7, end_line: 10 },
            methods: vec![
                Declaration::Function {
                    name: "new".into(),
                    signature: "pub fn new() -> Self".into(),
                    visibility: Public,
                    location: Location { start_line: 13, end_line: 15 },
                    is_async: false,
                    doc: Some("Create a new config with defaults".into()),
                },
                Declaration::Function {
                    name: "validate".into(),
                    signature: "fn validate(&self) -> bool".into(),
                    visibility: Private,
                    location: Location { start_line: 17, end_line: 19 },
                    is_async: false,
                    doc: None,
                },
            ],
            doc: Some("Configuration for the processor".into()),
        },
        Declaration::Function {
            name: "process".into(),
            signature: "pub async fn process<T: Display>(config: &Config, data: T) -> Result<Output> where T: Clone + Send".into(),
            visibility: Public,
            location: Location { start_line: 22, end_line: 27 },
            is_async: true,
            doc: Some("Process data according to config".into()),
        },
    ],
    token_count: 156,  // Computed from rendered output
    parse_error: None,
}
```
