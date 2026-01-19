//! Rust codemap extraction using tree-sitter.

use std::collections::HashMap;
use tree_sitter::Node;

use super::{
    find_child_by_kind, node_text, with_rust_parser, Declaration, ExtractOptions, Field, Import,
    Location, Visibility,
};

/// Extract imports and declarations from Rust source code.
pub fn extract(
    content: &str,
    options: &ExtractOptions,
) -> Result<(Vec<Import>, Vec<Declaration>), String> {
    with_rust_parser(|parser| {
        let tree = parser
            .parse(content, None)
            .ok_or_else(|| "failed to parse".to_string())?;

        let mut imports = Vec::new();
        let mut declarations = Vec::new();
        let mut impl_blocks: Vec<(String, Vec<Declaration>)> = Vec::new();

        extract_from_node(
            tree.root_node(),
            content,
            options,
            &mut imports,
            &mut declarations,
            &mut impl_blocks,
        );

        // Merge impl methods with their structs using HashMap for O(1) lookup
        if !impl_blocks.is_empty() {
            // Build index: struct name -> position in declarations (clone names to avoid borrow)
            let struct_indices: HashMap<String, usize> = declarations
                .iter()
                .enumerate()
                .filter_map(|(i, d)| match d {
                    Declaration::Struct { name, .. } => Some((name.clone(), i)),
                    _ => None,
                })
                .collect();

            // Merge impl methods using the index
            for (impl_type, methods) in impl_blocks {
                if let Some(&idx) = struct_indices.get(&impl_type) {
                    if let Declaration::Struct {
                        methods: struct_methods,
                        ..
                    } = &mut declarations[idx]
                    {
                        struct_methods.extend(methods);
                    }
                } else {
                    // No matching struct found, add methods as standalone
                    declarations.extend(methods);
                }
            }
        }

        Ok((imports, declarations))
    })?
}

fn extract_from_node(
    node: Node,
    content: &str,
    options: &ExtractOptions,
    imports: &mut Vec<Import>,
    declarations: &mut Vec<Declaration>,
    impl_blocks: &mut Vec<(String, Vec<Declaration>)>,
) {
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        match child.kind() {
            "use_declaration" => {
                if let Some(import) = extract_use(child, content) {
                    imports.push(import);
                }
            }
            "function_item" => {
                if let Some(func) = extract_function(child, content, options) {
                    if options.include_private || func.visibility() == Visibility::Public {
                        declarations.push(func);
                    }
                }
            }
            "struct_item" => {
                if let Some(decl) = extract_struct(child, content, options) {
                    if options.include_private || decl.visibility() == Visibility::Public {
                        declarations.push(decl);
                    }
                }
            }
            "enum_item" => {
                if let Some(decl) = extract_enum(child, content, options) {
                    if options.include_private || decl.visibility() == Visibility::Public {
                        declarations.push(decl);
                    }
                }
            }
            "trait_item" => {
                if let Some(decl) = extract_trait(child, content, options) {
                    if options.include_private || decl.visibility() == Visibility::Public {
                        declarations.push(decl);
                    }
                }
            }
            "type_item" => {
                if let Some(decl) = extract_type_alias(child, content) {
                    if options.include_private || decl.visibility() == Visibility::Public {
                        declarations.push(decl);
                    }
                }
            }
            "const_item" | "static_item" => {
                if let Some(decl) = extract_const(child, content) {
                    if options.include_private || decl.visibility() == Visibility::Public {
                        declarations.push(decl);
                    }
                }
            }
            "impl_item" => {
                // Collect impl blocks for later merging with HashMap
                if let Some(impl_data) = extract_impl(child, content, options) {
                    impl_blocks.push(impl_data);
                }
            }
            _ => {}
        }
    }
}

fn extract_use(node: Node, content: &str) -> Option<Import> {
    let text = node_text(node, content);

    // Parse use statement to extract source and items
    // Strip "pub " for re-exports, then "use ", then trailing ";"
    let text = text
        .trim_start_matches("pub ")
        .trim_start_matches("use ")
        .trim_end_matches(';')
        .trim();

    // Handle different use patterns
    if let Some(open_brace) = text.find('{') {
        // use foo::{bar, baz}
        // use crate::foo::{bar, baz}
        // use foo::{self, bar}
        // NOTE: We intentionally keep aliases as written: `bar as baz`.

        let before_brace = text[..open_brace].trim_end();
        let source = before_brace
            .strip_suffix("::")
            .unwrap_or(before_brace)
            .trim()
            .to_string();

        let close_brace = text.rfind('}')?;
        let items_str = text[open_brace + 1..close_brace].trim();

        let items: Vec<String> = items_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        return Some(Import {
            source,
            items: items.into(),
        });
    }

    // Simple use: use foo::bar
    // Also handle alias: use foo::bar as baz
    if let Some((source, item)) = text.rsplit_once("::") {
        return Some(Import {
            source: source.to_string(),
            items: smallvec::smallvec![item.trim().to_string()],
        });
    }

    // Fallback: just use the whole thing
    Some(Import {
        source: text.to_string(),
        items: smallvec::smallvec![],
    })
}

fn extract_function(node: Node, content: &str, options: &ExtractOptions) -> Option<Declaration> {
    let name = find_child_by_kind(node, "identifier").map(|n| node_text(n, content))?;

    let visibility = extract_visibility(node, content);

    // Build signature (everything up to the body)
    let signature = build_function_signature(node, content);

    // Check for async - try node children first, then signature text
    let is_async = node
        .children(&mut node.walk())
        .any(|c| c.kind() == "async" || c.kind() == "async_specifier")
        || signature.starts_with("async ")
        || signature.contains(" async ");

    let location = Location::new(node.start_position().row + 1, node.end_position().row + 1);

    let doc = if options.include_docs {
        extract_doc_comment(node, content)
    } else {
        None
    };

    Some(Declaration::Function {
        name,
        signature,
        visibility,
        location,
        is_async,
        doc,
    })
}

fn build_function_signature(node: Node, content: &str) -> String {
    let mut signature = String::new();

    for child in node.children(&mut node.walk()) {
        match child.kind() {
            "block" => break, // Stop before the function body
            _ => {
                let text = node_text(child, content);
                if !signature.is_empty() && !text.starts_with(',') && !text.starts_with(')') {
                    signature.push(' ');
                }
                signature.push_str(&text);
            }
        }
    }

    // Clean up whitespace
    signature.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn extract_struct(node: Node, content: &str, options: &ExtractOptions) -> Option<Declaration> {
    let name = find_child_by_kind(node, "type_identifier").map(|n| node_text(n, content))?;

    let visibility = extract_visibility(node, content);

    let mut fields = Vec::new();
    if let Some(field_list) = find_child_by_kind(node, "field_declaration_list") {
        for field_node in field_list.children(&mut field_list.walk()) {
            if field_node.kind() == "field_declaration" {
                if let Some(field) = extract_field(field_node, content) {
                    fields.push(field);
                }
            }
        }
    }

    let location = Location::new(node.start_position().row + 1, node.end_position().row + 1);

    let doc = if options.include_docs {
        extract_doc_comment(node, content)
    } else {
        None
    };

    Some(Declaration::Struct {
        name,
        fields: fields.into(),
        visibility,
        location,
        methods: Vec::new(), // Will be populated by impl extraction
        doc,
    })
}

fn extract_field(node: Node, content: &str) -> Option<Field> {
    let name = find_child_by_kind(node, "field_identifier").map(|n| node_text(n, content))?;

    let ty = find_child_by_kind(node, "type_identifier")
        .or_else(|| find_child_by_kind(node, "generic_type"))
        .or_else(|| find_child_by_kind(node, "reference_type"))
        .or_else(|| find_child_by_kind(node, "primitive_type"))
        .map(|n| node_text(n, content))
        .unwrap_or_default();

    let visibility = extract_visibility(node, content);

    Some(Field {
        name,
        ty,
        visibility,
    })
}

fn extract_enum(node: Node, content: &str, options: &ExtractOptions) -> Option<Declaration> {
    let name = find_child_by_kind(node, "type_identifier").map(|n| node_text(n, content))?;

    let visibility = extract_visibility(node, content);

    let mut variants = Vec::new();
    if let Some(variant_list) = find_child_by_kind(node, "enum_variant_list") {
        for variant_node in variant_list.children(&mut variant_list.walk()) {
            if variant_node.kind() == "enum_variant" {
                let variant_text = node_text(variant_node, content);
                variants.push(variant_text);
            }
        }
    }

    let location = Location::new(node.start_position().row + 1, node.end_position().row + 1);

    let doc = if options.include_docs {
        extract_doc_comment(node, content)
    } else {
        None
    };

    Some(Declaration::Enum {
        name,
        variants: variants.into(),
        visibility,
        location,
        doc,
    })
}

fn extract_trait(node: Node, content: &str, options: &ExtractOptions) -> Option<Declaration> {
    let name = find_child_by_kind(node, "type_identifier").map(|n| node_text(n, content))?;

    let mut methods = Vec::new();
    if let Some(body) = find_child_by_kind(node, "declaration_list") {
        for item in body.children(&mut body.walk()) {
            if item.kind() == "function_signature_item" {
                let sig = node_text(item, content);
                methods.push(sig.trim_end_matches(';').to_string());
            }
        }
    }

    let visibility = extract_visibility(node, content);

    let location = Location::new(node.start_position().row + 1, node.end_position().row + 1);

    let doc = if options.include_docs {
        extract_doc_comment(node, content)
    } else {
        None
    };

    Some(Declaration::Trait {
        name,
        methods: methods.into(),
        visibility,
        location,
        doc,
    })
}

fn extract_type_alias(node: Node, content: &str) -> Option<Declaration> {
    let name = find_child_by_kind(node, "type_identifier").map(|n| node_text(n, content))?;

    let visibility = extract_visibility(node, content);

    // Get the target type (everything after =)
    let full_text = node_text(node, content);
    let target = full_text
        .split('=')
        .nth(1)
        .map(|s| s.trim().trim_end_matches(';').to_string())
        .unwrap_or_default();

    let location = Location::new(node.start_position().row + 1, node.end_position().row + 1);

    Some(Declaration::TypeAlias {
        name,
        target,
        visibility,
        location,
    })
}

fn extract_const(node: Node, content: &str) -> Option<Declaration> {
    let name = find_child_by_kind(node, "identifier").map(|n| node_text(n, content))?;

    let visibility = extract_visibility(node, content);

    let ty = find_child_by_kind(node, "type_identifier")
        .or_else(|| find_child_by_kind(node, "primitive_type"))
        .map(|n| node_text(n, content))
        .unwrap_or_default();

    let location = Location::new(node.start_position().row + 1, node.end_position().row + 1);

    Some(Declaration::Const {
        name,
        ty,
        visibility,
        location,
    })
}

/// Extract impl block and return (type_name, methods) for later merging.
fn extract_impl(
    node: Node,
    content: &str,
    options: &ExtractOptions,
) -> Option<(String, Vec<Declaration>)> {
    // Get the type being implemented
    let impl_type = find_child_by_kind(node, "type_identifier").map(|n| node_text(n, content))?;

    // Extract methods from the impl block
    let mut methods = Vec::new();
    if let Some(body) = find_child_by_kind(node, "declaration_list") {
        for item in body.children(&mut body.walk()) {
            if item.kind() == "function_item" {
                if let Some(func) = extract_function(item, content, options) {
                    if options.include_private || func.visibility() == Visibility::Public {
                        methods.push(func);
                    }
                }
            }
        }
    }

    if methods.is_empty() {
        None
    } else {
        Some((impl_type, methods))
    }
}

fn extract_visibility(node: Node, content: &str) -> Visibility {
    for child in node.children(&mut node.walk()) {
        if child.kind() == "visibility_modifier" {
            let text = node_text(child, content);
            if text.contains("pub(crate)") {
                return Visibility::Crate;
            } else if text.starts_with("pub") {
                return Visibility::Public;
            }
        }
    }
    Visibility::Private
}

fn extract_doc_comment(node: Node, content: &str) -> Option<String> {
    // Look for preceding doc comments
    let mut prev = node.prev_sibling();
    let mut doc_lines = Vec::new();

    while let Some(sibling) = prev {
        if sibling.kind() == "line_comment" {
            let text = node_text(sibling, content);
            if text.starts_with("///") {
                doc_lines.push(text.trim_start_matches("///").trim().to_string());
            } else {
                break;
            }
        } else if sibling.kind() == "block_comment" {
            let text = node_text(sibling, content);
            if text.starts_with("/**") {
                // Block doc comment
                let inner = text.trim_start_matches("/**").trim_end_matches("*/").trim();
                return Some(inner.to_string());
            }
            break;
        } else {
            break;
        }
        prev = sibling.prev_sibling();
    }

    if doc_lines.is_empty() {
        None
    } else {
        doc_lines.reverse();
        Some(doc_lines.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_function() {
        let code = r#"
pub fn hello(name: &str) -> String {
    format!("Hello, {}", name)
}
"#;
        let (_, decls) = extract(code, &ExtractOptions::default()).unwrap();
        assert_eq!(decls.len(), 1);

        match &decls[0] {
            Declaration::Function {
                name, visibility, ..
            } => {
                assert_eq!(name, "hello");
                assert_eq!(*visibility, Visibility::Public);
            }
            _ => panic!("expected function"),
        }
    }

    #[test]
    fn test_extract_struct() {
        let code = r#"
pub struct Config {
    pub name: String,
    timeout: u64,
}
"#;
        let (_, decls) = extract(code, &ExtractOptions::default()).unwrap();
        assert_eq!(decls.len(), 1);

        match &decls[0] {
            Declaration::Struct {
                name,
                fields,
                visibility,
                ..
            } => {
                assert_eq!(name, "Config");
                assert_eq!(*visibility, Visibility::Public);
                assert_eq!(fields.len(), 2);
            }
            _ => panic!("expected struct"),
        }
    }

    #[test]
    fn test_extract_enum() {
        let code = r#"
pub enum Status {
    Running,
    Stopped,
    Error(String),
}
"#;
        let (_, decls) = extract(code, &ExtractOptions::default()).unwrap();
        assert_eq!(decls.len(), 1);

        match &decls[0] {
            Declaration::Enum { name, variants, .. } => {
                assert_eq!(name, "Status");
                assert_eq!(variants.len(), 3);
            }
            _ => panic!("expected enum"),
        }
    }

    #[test]
    fn test_extract_use() {
        let code = r#"
 use std::collections::HashMap;
 use std::io::{Read, Write};
 use crate::foo::{bar, baz};
 use foo::bar as baz;
 use foo::{self, qux};
 "#;
        let (imports, _) = extract(code, &ExtractOptions::default()).unwrap();

        assert!(imports.iter().any(|i| {
            i.source == "std::collections" && i.items.iter().any(|it| it == "HashMap")
        }));
        assert!(imports.iter().any(|i| {
            i.source == "std::io"
                && i.items.iter().any(|it| it == "Read")
                && i.items.iter().any(|it| it == "Write")
        }));
        assert!(imports.iter().any(|i| {
            i.source == "crate::foo"
                && i.items.iter().any(|it| it == "bar")
                && i.items.iter().any(|it| it == "baz")
        }));
        assert!(imports
            .iter()
            .any(|i| { i.source == "foo" && i.items.iter().any(|it| it == "bar as baz") }));
        assert!(imports.iter().any(|i| {
            i.source == "foo"
                && i.items.iter().any(|it| it == "self")
                && i.items.iter().any(|it| it == "qux")
        }));
    }

    #[test]
    fn test_private_filtering() {
        let code = r#"
 pub fn public_fn() {}
 fn private_fn() {}
 
 pub trait PublicTrait {
     fn a(&self);
 }
 trait PrivateTrait {
     fn b(&self);
 }
 "#;
        // With include_private = false (default)
        let (_, decls) = extract(code, &ExtractOptions::default()).unwrap();
        assert_eq!(decls.len(), 2);
        assert!(decls.iter().any(|d| d.name() == "public_fn"));
        assert!(decls.iter().any(|d| d.name() == "PublicTrait"));
        assert!(!decls.iter().any(|d| d.name() == "PrivateTrait"));

        // With include_private = true
        let opts = ExtractOptions {
            include_private: true,
            ..Default::default()
        };
        let (_, decls) = extract(code, &opts).unwrap();
        assert_eq!(decls.len(), 4);
        assert!(decls.iter().any(|d| d.name() == "private_fn"));
        assert!(decls.iter().any(|d| d.name() == "PrivateTrait"));
    }

    #[test]
    fn test_async_function() {
        let code = r#"
pub async fn fetch_data() -> Result<(), Error> {}
"#;
        let (_, decls) = extract(code, &ExtractOptions::default()).unwrap();

        match &decls[0] {
            Declaration::Function { is_async, .. } => {
                assert!(*is_async);
            }
            _ => panic!("expected function"),
        }
    }
}
