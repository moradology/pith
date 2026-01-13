//! TypeScript/TSX codemap extraction using tree-sitter.

use tree_sitter::Parser;

use crate::filter::Language;
use super::{Declaration, ExtractOptions, Import, Location, Visibility, find_child_by_kind, node_text, with_ts_parser, with_tsx_parser};

/// Extract imports and declarations from TypeScript/TSX source code.
pub fn extract(
    content: &str,
    language: Language,
    options: &ExtractOptions,
) -> Result<(Vec<Import>, Vec<Declaration>), String> {
    let extract_fn = |parser: &mut Parser| {
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
    };

    match language {
        Language::Tsx => with_tsx_parser(extract_fn),
        _ => with_ts_parser(extract_fn),
    }
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
            "import_statement" => {
                if let Some(import) = extract_import(child, content) {
                    imports.push(import);
                }
            }
            "export_statement" => {
                extract_export(child, content, options, declarations);
            }
            "function_declaration" => {
                if let Some(func) = extract_function(child, content, options, false) {
                    declarations.push(func);
                }
            }
            "class_declaration" => {
                if let Some(class) = extract_class(child, content, options, false) {
                    declarations.push(class);
                }
            }
            "interface_declaration" => {
                if let Some(iface) = extract_interface(child, content, options) {
                    declarations.push(iface);
                }
            }
            "type_alias_declaration" => {
                if let Some(alias) = extract_type_alias(child, content) {
                    declarations.push(alias);
                }
            }
            "lexical_declaration" => {
                // const/let declarations
                extract_lexical(child, content, options, declarations, false);
            }
            _ => {}
        }
    }
}

fn extract_import(node: tree_sitter::Node, content: &str) -> Option<Import> {
    let text = node_text(node, content);

    // Simple parsing: extract from "module"
    let source = text
        .split(&['\'', '"'][..])
        .nth(1)
        .map(|s| s.to_string())?;

    // Extract imported items
    let mut items = Vec::new();

    if text.contains('{') {
        // Named imports
        if let Some(start) = text.find('{') {
            if let Some(end) = text.find('}') {
                let inner = &text[start + 1..end];
                items = inner
                    .split(',')
                    .map(|s| s.split(" as ").next().unwrap_or(s).trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
        }
    } else if text.contains("* as") {
        items.push("*".to_string());
    }

    Some(Import { source, items: items.into() })
}

fn extract_export(
    node: tree_sitter::Node,
    content: &str,
    options: &ExtractOptions,
    declarations: &mut Vec<Declaration>,
) {
    for child in node.children(&mut node.walk()) {
        match child.kind() {
            "function_declaration" => {
                if let Some(func) = extract_function(child, content, options, true) {
                    declarations.push(func);
                }
            }
            "class_declaration" => {
                if let Some(class) = extract_class(child, content, options, true) {
                    declarations.push(class);
                }
            }
            "interface_declaration" => {
                if let Some(iface) = extract_interface(child, content, options) {
                    declarations.push(iface);
                }
            }
            "type_alias_declaration" => {
                if let Some(alias) = extract_type_alias(child, content) {
                    declarations.push(alias);
                }
            }
            "lexical_declaration" => {
                extract_lexical(child, content, options, declarations, true);
            }
            _ => {}
        }
    }
}

fn extract_function(
    node: tree_sitter::Node,
    content: &str,
    options: &ExtractOptions,
    is_exported: bool,
) -> Option<Declaration> {
    let name = find_child_by_kind(node, "identifier")
        .map(|n| node_text(n, content))?;

    let is_async = node.children(&mut node.walk())
        .any(|c| c.kind() == "async");

    // Build signature
    let mut signature = String::new();
    if is_exported {
        signature.push_str("export ");
    }
    if is_async {
        signature.push_str("async ");
    }
    signature.push_str("function ");
    signature.push_str(&name);

    // Add parameters
    if let Some(params) = find_child_by_kind(node, "formal_parameters") {
        signature.push_str(&node_text(params, content));
    }

    // Add return type
    if let Some(ret) = find_child_by_kind(node, "type_annotation") {
        signature.push_str(&node_text(ret, content));
    }

    let location = Location::new(
        node.start_position().row + 1,
        node.end_position().row + 1,
    );

    let doc = if options.include_docs {
        extract_jsdoc(node, content)
    } else {
        None
    };

    Some(Declaration::Function {
        name,
        signature,
        visibility: if is_exported { Visibility::Public } else { Visibility::Private },
        location,
        is_async,
        doc,
    })
}

fn extract_class(
    node: tree_sitter::Node,
    content: &str,
    options: &ExtractOptions,
    is_exported: bool,
) -> Option<Declaration> {
    let name = find_child_by_kind(node, "type_identifier")
        .map(|n| node_text(n, content))?;

    let mut members = Vec::new();

    if let Some(body) = find_child_by_kind(node, "class_body") {
        for child in body.children(&mut body.walk()) {
            match child.kind() {
                "method_definition" => {
                    if let Some(method) = extract_method(child, content, options) {
                        members.push(method);
                    }
                }
                "public_field_definition" | "field_definition" => {
                    // Could extract fields here
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
        extract_jsdoc(node, content)
    } else {
        None
    };

    Some(Declaration::Class {
        name,
        members,
        visibility: if is_exported { Visibility::Public } else { Visibility::Private },
        location,
        doc,
    })
}

fn extract_method(
    node: tree_sitter::Node,
    content: &str,
    options: &ExtractOptions,
) -> Option<Declaration> {
    let name = find_child_by_kind(node, "property_identifier")
        .map(|n| node_text(n, content))?;

    let is_async = node.children(&mut node.walk())
        .any(|c| c.kind() == "async");

    let mut signature = String::new();
    if is_async {
        signature.push_str("async ");
    }
    signature.push_str(&name);

    if let Some(params) = find_child_by_kind(node, "formal_parameters") {
        signature.push_str(&node_text(params, content));
    }

    if let Some(ret) = find_child_by_kind(node, "type_annotation") {
        signature.push_str(&node_text(ret, content));
    }

    let location = Location::new(
        node.start_position().row + 1,
        node.end_position().row + 1,
    );

    let doc = if options.include_docs {
        extract_jsdoc(node, content)
    } else {
        None
    };

    Some(Declaration::Function {
        name,
        signature,
        visibility: Visibility::Public,
        location,
        is_async,
        doc,
    })
}

fn extract_interface(
    node: tree_sitter::Node,
    content: &str,
    options: &ExtractOptions,
) -> Option<Declaration> {
    let name = find_child_by_kind(node, "type_identifier")
        .map(|n| node_text(n, content))?;

    let mut members = Vec::new();

    if let Some(body) = find_child_by_kind(node, "object_type")
        .or_else(|| find_child_by_kind(node, "interface_body"))
    {
        for child in body.children(&mut body.walk()) {
            if child.kind() == "property_signature" || child.kind() == "method_signature" {
                members.push(node_text(child, content).trim_end_matches([',', ';']).to_string());
            }
        }
    }

    let location = Location::new(
        node.start_position().row + 1,
        node.end_position().row + 1,
    );

    let doc = if options.include_docs {
        extract_jsdoc(node, content)
    } else {
        None
    };

    Some(Declaration::Interface {
        name,
        members: members.into(),
        location,
        doc,
    })
}

fn extract_type_alias(node: tree_sitter::Node, content: &str) -> Option<Declaration> {
    let name = find_child_by_kind(node, "type_identifier")
        .map(|n| node_text(n, content))?;

    let full_text = node_text(node, content);
    let target = full_text
        .split('=')
        .nth(1)
        .map(|s| s.trim().trim_end_matches(';').to_string())
        .unwrap_or_default();

    let location = Location::new(
        node.start_position().row + 1,
        node.end_position().row + 1,
    );

    Some(Declaration::TypeAlias {
        name,
        target,
        visibility: Visibility::Public,
        location,
    })
}

fn extract_lexical(
    node: tree_sitter::Node,
    content: &str,
    options: &ExtractOptions,
    declarations: &mut Vec<Declaration>,
    is_exported: bool,
) {
    for child in node.children(&mut node.walk()) {
        if child.kind() == "variable_declarator" {
            // Check if it's an arrow function
            if let Some(arrow) = find_child_by_kind(child, "arrow_function") {
                let name = find_child_by_kind(child, "identifier")
                    .map(|n| node_text(n, content));

                if let Some(name) = name {
                    let is_async = arrow.children(&mut arrow.walk())
                        .any(|c| c.kind() == "async");

                    let mut signature = String::new();
                    if is_exported {
                        signature.push_str("export ");
                    }
                    signature.push_str("const ");
                    signature.push_str(&name);

                    // Type annotation on the variable
                    if let Some(type_ann) = find_child_by_kind(child, "type_annotation") {
                        signature.push_str(&node_text(type_ann, content));
                    }

                    let location = Location::new(
                        node.start_position().row + 1,
                        node.end_position().row + 1,
                    );

                    let doc = if options.include_docs {
                        extract_jsdoc(node, content)
                    } else {
                        None
                    };

                    declarations.push(Declaration::Function {
                        name,
                        signature,
                        visibility: if is_exported { Visibility::Public } else { Visibility::Private },
                        location,
                        is_async,
                        doc,
                    });
                }
            }
        }
    }
}

fn extract_jsdoc(node: tree_sitter::Node, content: &str) -> Option<String> {
    let mut prev = node.prev_sibling();

    while let Some(sibling) = prev {
        if sibling.kind() == "comment" {
            let text = node_text(sibling, content);
            if text.starts_with("/**") {
                let inner = text
                    .trim_start_matches("/**")
                    .trim_end_matches("*/")
                    .lines()
                    .map(|l| l.trim().trim_start_matches('*').trim())
                    .filter(|l| !l.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n");
                return Some(inner);
            }
        }
        prev = sibling.prev_sibling();
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_function() {
        let code = r#"
export function greet(name: string): string {
    return `Hello, ${name}`;
}
"#;
        let (_, decls) = extract(code, Language::TypeScript, &ExtractOptions::default()).unwrap();
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].name(), "greet");
    }

    #[test]
    fn test_extract_interface() {
        let code = r#"
export interface Config {
    name: string;
    timeout: number;
}
"#;
        let (_, decls) = extract(code, Language::TypeScript, &ExtractOptions::default()).unwrap();
        assert_eq!(decls.len(), 1);

        match &decls[0] {
            Declaration::Interface { name, members, .. } => {
                assert_eq!(name, "Config");
                assert_eq!(members.len(), 2);
            }
            _ => panic!("expected interface"),
        }
    }

    #[test]
    fn test_extract_import() {
        let code = r#"
import { useState, useEffect } from 'react';
import * as utils from './utils';
"#;
        let (imports, _) = extract(code, Language::TypeScript, &ExtractOptions::default()).unwrap();
        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].source, "react");
        assert_eq!(imports[0].items.len(), 2);
    }

    #[test]
    fn test_extract_class() {
        let code = r#"
export class Handler {
    async handle(req: Request): Promise<Response> {
        return new Response();
    }
}
"#;
        let (_, decls) = extract(code, Language::TypeScript, &ExtractOptions::default()).unwrap();
        assert_eq!(decls.len(), 1);

        match &decls[0] {
            Declaration::Class { name, members, .. } => {
                assert_eq!(name, "Handler");
                assert_eq!(members.len(), 1);
            }
            _ => panic!("expected class"),
        }
    }
}
