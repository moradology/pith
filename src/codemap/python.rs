//! Python codemap extraction using tree-sitter.

use super::{Declaration, ExtractOptions, Import, Location, Visibility, find_child_by_kind, node_text, with_python_parser};

/// Extract imports and declarations from Python source code.
pub fn extract(
    content: &str,
    options: &ExtractOptions,
) -> Result<(Vec<Import>, Vec<Declaration>), String> {
    with_python_parser(|parser| {
        let tree = parser
            .parse(content, None)
            .ok_or_else(|| "failed to parse".to_string())?;

        let mut imports = Vec::new();
        let mut declarations = Vec::new();

        extract_from_node(
            tree.root_node(),
            content,
            options,
            &mut imports,
            &mut declarations,
        );

        Ok((imports, declarations))
    })
}

fn extract_from_node(
    node: tree_sitter::Node,
    content: &str,
    options: &ExtractOptions,
    imports: &mut Vec<Import>,
    declarations: &mut Vec<Declaration>,
) {
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        match child.kind() {
            "import_statement" | "import_from_statement" => {
                if let Some(import) = extract_import(child, content) {
                    imports.push(import);
                }
            }
            "function_definition" => {
                if let Some(func) = extract_function(child, content, options) {
                    if options.include_private || func.visibility() == Visibility::Public {
                        declarations.push(func);
                    }
                }
            }
            "class_definition" => {
                if let Some(class) = extract_class(child, content, options) {
                    declarations.push(class);
                }
            }
            "decorated_definition" => {
                // Handle decorated functions/classes
                extract_decorated(child, content, options, declarations);
            }
            _ => {}
        }
    }
}

fn extract_import(node: tree_sitter::Node, content: &str) -> Option<Import> {
    let text = node_text(node, content);

    if node.kind() == "import_from_statement" {
        // from X import Y, Z
        let mut parts = text.splitn(2, " import ");
        let source = parts.next()?
            .trim_start_matches("from ")
            .trim()
            .to_string();

        let items: Vec<String> = parts.next()
            .map(|s| {
                s.split(',')
                    .map(|item| item.split(" as ").next().unwrap_or(item).trim().to_string())
                    .filter(|s| !s.is_empty() && s != "*")
                    .collect()
            })
            .unwrap_or_default();

        Some(Import { source, items })
    } else {
        // import X
        let source = text.trim_start_matches("import ").trim().to_string();
        Some(Import { source, items: Vec::new() })
    }
}

fn extract_function(
    node: tree_sitter::Node,
    content: &str,
    options: &ExtractOptions,
) -> Option<Declaration> {
    let name = find_child_by_kind(node, "identifier")
        .map(|n| node_text(n, content))?;

    let is_async = node.children(&mut node.walk())
        .any(|c| c.kind() == "async");

    // Determine visibility from name convention
    let visibility = if name.starts_with("__") && !name.ends_with("__") {
        Visibility::Private
    } else if name.starts_with('_') {
        Visibility::Protected
    } else {
        Visibility::Public
    };

    // Build signature
    let mut signature = String::new();
    if is_async {
        signature.push_str("async ");
    }
    signature.push_str("def ");
    signature.push_str(&name);

    // Parameters
    if let Some(params) = find_child_by_kind(node, "parameters") {
        signature.push_str(&node_text(params, content));
    }

    // Return type annotation
    if let Some(ret) = find_child_by_kind(node, "type") {
        signature.push_str(" -> ");
        signature.push_str(&node_text(ret, content));
    }

    let location = Location::new(
        node.start_position().row + 1,
        node.end_position().row + 1,
    );

    let doc = if options.include_docs {
        extract_docstring(node, content)
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

fn extract_class(
    node: tree_sitter::Node,
    content: &str,
    options: &ExtractOptions,
) -> Option<Declaration> {
    let name = find_child_by_kind(node, "identifier")
        .map(|n| node_text(n, content))?;

    let visibility = if name.starts_with('_') {
        Visibility::Protected
    } else {
        Visibility::Public
    };

    let mut members = Vec::new();

    if let Some(body) = find_child_by_kind(node, "block") {
        for child in body.children(&mut body.walk()) {
            match child.kind() {
                "function_definition" => {
                    if let Some(method) = extract_function(child, content, options) {
                        if options.include_private || method.visibility() != Visibility::Private {
                            members.push(method);
                        }
                    }
                }
                "decorated_definition" => {
                    extract_decorated(child, content, options, &mut members);
                }
                _ => {}
            }
        }
    }

    let location = Location::new(
        node.start_position().row + 1,
        node.end_position().row + 1,
    );

    let doc = if options.include_docs {
        extract_docstring(node, content)
    } else {
        None
    };

    Some(Declaration::Class {
        name,
        members,
        visibility,
        location,
        doc,
    })
}

fn extract_decorated(
    node: tree_sitter::Node,
    content: &str,
    options: &ExtractOptions,
    declarations: &mut Vec<Declaration>,
) {
    for child in node.children(&mut node.walk()) {
        match child.kind() {
            "function_definition" => {
                if let Some(func) = extract_function(child, content, options) {
                    if options.include_private || func.visibility() == Visibility::Public {
                        declarations.push(func);
                    }
                }
            }
            "class_definition" => {
                if let Some(class) = extract_class(child, content, options) {
                    declarations.push(class);
                }
            }
            _ => {}
        }
    }
}

fn extract_docstring(node: tree_sitter::Node, content: &str) -> Option<String> {
    // Look for docstring as first statement in block
    let block = find_child_by_kind(node, "block")?;

    // Only check first statement
    if let Some(child) = block.children(&mut block.walk()).next() {
        if child.kind() == "expression_statement" {
            if let Some(string) = find_child_by_kind(child, "string") {
                let text = node_text(string, content);
                // Remove quotes
                let inner = text
                    .trim_start_matches("\"\"\"")
                    .trim_start_matches("'''")
                    .trim_end_matches("\"\"\"")
                    .trim_end_matches("'''")
                    .trim_start_matches('"')
                    .trim_start_matches('\'')
                    .trim_end_matches('"')
                    .trim_end_matches('\'')
                    .trim();
                return Some(inner.to_string());
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_function() {
        let code = r#"
def greet(name: str) -> str:
    """Greet someone."""
    return f"Hello, {name}"
"#;
        let opts = ExtractOptions { include_docs: true, ..Default::default() };
        let (_, decls) = extract(code, &opts).unwrap();
        assert_eq!(decls.len(), 1);

        match &decls[0] {
            Declaration::Function { name, doc, .. } => {
                assert_eq!(name, "greet");
                assert!(doc.is_some());
            }
            _ => panic!("expected function"),
        }
    }

    #[test]
    fn test_extract_class() {
        let code = r#"
class Handler:
    """Handle requests."""

    def __init__(self, config):
        self.config = config

    async def handle(self, request) -> Response:
        pass
"#;
        let (_, decls) = extract(code, &ExtractOptions::default()).unwrap();
        assert_eq!(decls.len(), 1);

        match &decls[0] {
            Declaration::Class { name, members, .. } => {
                assert_eq!(name, "Handler");
                // __init__ and handle
                assert!(members.len() >= 1);
            }
            _ => panic!("expected class"),
        }
    }

    #[test]
    fn test_extract_import() {
        let code = r#"
from typing import List, Optional
import os
from .utils import helper
"#;
        let (imports, _) = extract(code, &ExtractOptions::default()).unwrap();
        assert_eq!(imports.len(), 3);
        assert_eq!(imports[0].source, "typing");
        assert_eq!(imports[0].items.len(), 2);
    }

    #[test]
    fn test_private_visibility() {
        let code = r#"
def public_func():
    pass

def _protected_func():
    pass

def __private_func():
    pass
"#;
        let opts = ExtractOptions { include_private: true, ..Default::default() };
        let (_, decls) = extract(code, &opts).unwrap();
        assert_eq!(decls.len(), 3);

        assert_eq!(decls[0].visibility(), Visibility::Public);
        assert_eq!(decls[1].visibility(), Visibility::Protected);
        assert_eq!(decls[2].visibility(), Visibility::Private);
    }

    #[test]
    fn test_async_function() {
        let code = r#"
async def fetch_data(url: str) -> bytes:
    pass
"#;
        let (_, decls) = extract(code, &ExtractOptions::default()).unwrap();

        match &decls[0] {
            Declaration::Function { is_async, signature, .. } => {
                assert!(*is_async);
                assert!(signature.contains("async"));
            }
            _ => panic!("expected function"),
        }
    }
}
