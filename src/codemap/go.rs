//! Go codemap extraction using tree-sitter.

use super::{Declaration, ExtractOptions, Field, Import, Location, Visibility, find_child_by_kind, node_text, with_go_parser};

/// Extract imports and declarations from Go source code.
pub fn extract(
    content: &str,
    options: &ExtractOptions,
) -> Result<(Vec<Import>, Vec<Declaration>), String> {
    with_go_parser(|parser| {
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
            "import_declaration" => {
                extract_imports(child, content, imports);
            }
            "function_declaration" => {
                if let Some(func) = extract_function(child, content, options) {
                    if options.include_private || func.visibility() == Visibility::Public {
                        declarations.push(func);
                    }
                }
            }
            "method_declaration" => {
                if let Some(method) = extract_method(child, content, options) {
                    if options.include_private || method.visibility() == Visibility::Public {
                        declarations.push(method);
                    }
                }
            }
            "type_declaration" => {
                extract_type_decl(child, content, options, declarations);
            }
            "const_declaration" | "var_declaration" => {
                extract_const_var(child, content, options, declarations);
            }
            _ => {}
        }
    }
}

fn extract_imports(node: tree_sitter::Node, content: &str, imports: &mut Vec<Import>) {
    for child in node.children(&mut node.walk()) {
        if child.kind() == "import_spec_list" {
            for spec in child.children(&mut child.walk()) {
                if spec.kind() == "import_spec" {
                    if let Some(import) = extract_import_spec(spec, content) {
                        imports.push(import);
                    }
                }
            }
        } else if child.kind() == "import_spec" {
            if let Some(import) = extract_import_spec(child, content) {
                imports.push(import);
            }
        }
    }
}

fn extract_import_spec(node: tree_sitter::Node, content: &str) -> Option<Import> {
    let path = find_child_by_kind(node, "interpreted_string_literal")
        .map(|n| {
            let text = node_text(n, content);
            text.trim_matches('"').to_string()
        })?;

    Some(Import {
        source: path,
        items: Vec::new(),
    })
}

fn extract_function(
    node: tree_sitter::Node,
    content: &str,
    options: &ExtractOptions,
) -> Option<Declaration> {
    let name = find_child_by_kind(node, "identifier")
        .map(|n| node_text(n, content))?;

    let visibility = go_visibility(&name);

    // Build signature
    let mut signature = String::new();
    signature.push_str("func ");
    signature.push_str(&name);

    // Parameters
    if let Some(params) = find_child_by_kind(node, "parameter_list") {
        signature.push_str(&node_text(params, content));
    }

    // Return type
    if let Some(result) = find_child_by_kind(node, "result") {
        signature.push(' ');
        signature.push_str(&node_text(result, content));
    }

    let location = Location::new(
        node.start_position().row + 1,
        node.end_position().row + 1,
    );

    let doc = if options.include_docs {
        extract_comment(node, content)
    } else {
        None
    };

    Some(Declaration::Function {
        name,
        signature,
        visibility,
        location,
        is_async: false, // Go doesn't have async keyword
        doc,
    })
}

fn extract_method(
    node: tree_sitter::Node,
    content: &str,
    options: &ExtractOptions,
) -> Option<Declaration> {
    let name = find_child_by_kind(node, "field_identifier")
        .map(|n| node_text(n, content))?;

    let visibility = go_visibility(&name);

    // Build signature with receiver
    let mut signature = String::new();
    signature.push_str("func ");

    // Receiver
    if let Some(receiver) = find_child_by_kind(node, "parameter_list") {
        signature.push_str(&node_text(receiver, content));
        signature.push(' ');
    }

    signature.push_str(&name);

    // Find the second parameter list (actual parameters)
    let param_lists: Vec<_> = node.children(&mut node.walk())
        .filter(|c| c.kind() == "parameter_list")
        .collect();

    if param_lists.len() > 1 {
        signature.push_str(&node_text(param_lists[1], content));
    }

    // Return type
    if let Some(result) = find_child_by_kind(node, "result") {
        signature.push(' ');
        signature.push_str(&node_text(result, content));
    }

    let location = Location::new(
        node.start_position().row + 1,
        node.end_position().row + 1,
    );

    let doc = if options.include_docs {
        extract_comment(node, content)
    } else {
        None
    };

    Some(Declaration::Function {
        name,
        signature,
        visibility,
        location,
        is_async: false,
        doc,
    })
}

fn extract_type_decl(
    node: tree_sitter::Node,
    content: &str,
    options: &ExtractOptions,
    declarations: &mut Vec<Declaration>,
) {
    for child in node.children(&mut node.walk()) {
        if child.kind() == "type_spec" {
            if let Some(decl) = extract_type_spec(child, content, options) {
                if options.include_private || decl.visibility() == Visibility::Public {
                    declarations.push(decl);
                }
            }
        }
    }
}

fn extract_type_spec(
    node: tree_sitter::Node,
    content: &str,
    options: &ExtractOptions,
) -> Option<Declaration> {
    let name = find_child_by_kind(node, "type_identifier")
        .map(|n| node_text(n, content))?;

    let visibility = go_visibility(&name);

    // Check what kind of type it is
    if let Some(struct_type) = find_child_by_kind(node, "struct_type") {
        let mut fields = Vec::new();

        if let Some(field_list) = find_child_by_kind(struct_type, "field_declaration_list") {
            for field in field_list.children(&mut field_list.walk()) {
                if field.kind() == "field_declaration" {
                    if let Some(f) = extract_struct_field(field, content) {
                        fields.push(f);
                    }
                }
            }
        }

        let location = Location::new(
            node.start_position().row + 1,
            node.end_position().row + 1,
        );

        let doc = if options.include_docs {
            extract_comment(node, content)
        } else {
            None
        };

        return Some(Declaration::Struct {
            name,
            fields,
            visibility,
            location,
            methods: Vec::new(),
            doc,
        });
    }

    if let Some(interface_type) = find_child_by_kind(node, "interface_type") {
        let mut members = Vec::new();

        for child in interface_type.children(&mut interface_type.walk()) {
            // Handle different tree-sitter-go versions
            if child.kind() == "method_spec" || child.kind() == "method_elem" {
                members.push(node_text(child, content));
            }
        }

        let location = Location::new(
            node.start_position().row + 1,
            node.end_position().row + 1,
        );

        let doc = if options.include_docs {
            extract_comment(node, content)
        } else {
            None
        };

        return Some(Declaration::Interface {
            name,
            members,
            location,
            doc,
        });
    }

    // Type alias
    let full_text = node_text(node, content);
    let target = full_text
        .split_once(&name)
        .map(|(_, s)| s.trim().to_string())
        .unwrap_or_default();

    let location = Location::new(
        node.start_position().row + 1,
        node.end_position().row + 1,
    );

    Some(Declaration::TypeAlias {
        name,
        target,
        visibility,
        location,
    })
}

fn extract_struct_field(node: tree_sitter::Node, content: &str) -> Option<Field> {
    let name = find_child_by_kind(node, "field_identifier")
        .map(|n| node_text(n, content))?;

    let visibility = go_visibility(&name);

    // Get type - could be various type nodes
    let ty = node.children(&mut node.walk())
        .find(|c| c.kind().contains("type") || c.kind() == "qualified_type" || c.kind() == "pointer_type")
        .map(|n| node_text(n, content))
        .unwrap_or_default();

    Some(Field { name, ty, visibility })
}

fn extract_const_var(
    node: tree_sitter::Node,
    content: &str,
    options: &ExtractOptions,
    declarations: &mut Vec<Declaration>,
) {
    for child in node.children(&mut node.walk()) {
        if child.kind() == "const_spec" || child.kind() == "var_spec" {
            if let Some(name_node) = find_child_by_kind(child, "identifier") {
                let name = node_text(name_node, content);
                let visibility = go_visibility(&name);

                if !options.include_private && visibility != Visibility::Public {
                    continue;
                }

                let ty = find_child_by_kind(child, "type_identifier")
                    .map(|n| node_text(n, content))
                    .unwrap_or_default();

                let location = Location::new(
                    child.start_position().row + 1,
                    child.end_position().row + 1,
                );

                declarations.push(Declaration::Const {
                    name,
                    ty,
                    visibility,
                    location,
                });
            }
        }
    }
}

fn extract_comment(node: tree_sitter::Node, content: &str) -> Option<String> {
    let mut prev = node.prev_sibling();
    let mut doc_lines = Vec::new();

    while let Some(sibling) = prev {
        if sibling.kind() == "comment" {
            let text = node_text(sibling, content);
            let line = text.trim_start_matches("//").trim();
            doc_lines.push(line.to_string());
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

/// Go visibility is determined by capitalization of the first letter.
fn go_visibility(name: &str) -> Visibility {
    if name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
        Visibility::Public
    } else {
        Visibility::Private
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_function() {
        let code = r#"
package main

// Process handles the input.
func Process(input string) (string, error) {
    return input, nil
}
"#;
        let opts = ExtractOptions { include_docs: true, ..Default::default() };
        let (_, decls) = extract(code, &opts).unwrap();
        assert_eq!(decls.len(), 1);

        match &decls[0] {
            Declaration::Function { name, visibility, doc, .. } => {
                assert_eq!(name, "Process");
                assert_eq!(*visibility, Visibility::Public);
                assert!(doc.is_some());
            }
            _ => panic!("expected function"),
        }
    }

    #[test]
    fn test_extract_struct() {
        let code = r#"
package main

type Config struct {
    Name    string
    Timeout int
}
"#;
        let (_, decls) = extract(code, &ExtractOptions::default()).unwrap();
        assert_eq!(decls.len(), 1);

        match &decls[0] {
            Declaration::Struct { name, fields, .. } => {
                assert_eq!(name, "Config");
                assert_eq!(fields.len(), 2);
            }
            _ => panic!("expected struct"),
        }
    }

    #[test]
    fn test_extract_interface() {
        let code = r#"
package main

type Handler interface {
    Handle(req Request) Response
    Name() string
}
"#;
        let (_, decls) = extract(code, &ExtractOptions::default()).unwrap();
        assert_eq!(decls.len(), 1);

        match &decls[0] {
            Declaration::Interface { name, members, .. } => {
                assert_eq!(name, "Handler");
                assert_eq!(members.len(), 2);
            }
            _ => panic!("expected interface"),
        }
    }

    #[test]
    fn test_extract_import() {
        let code = r#"
package main

import (
    "fmt"
    "net/http"
)
"#;
        let (imports, _) = extract(code, &ExtractOptions::default()).unwrap();
        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].source, "fmt");
        assert_eq!(imports[1].source, "net/http");
    }

    #[test]
    fn test_go_visibility() {
        let code = r#"
package main

func PublicFunc() {}
func privateFunc() {}
"#;
        let opts = ExtractOptions { include_private: true, ..Default::default() };
        let (_, decls) = extract(code, &opts).unwrap();
        assert_eq!(decls.len(), 2);

        assert_eq!(decls[0].visibility(), Visibility::Public);
        assert_eq!(decls[1].visibility(), Visibility::Private);
    }

    #[test]
    fn test_extract_method() {
        let code = r#"
package main

func (h *Handler) Handle(req Request) Response {
    return Response{}
}
"#;
        let (_, decls) = extract(code, &ExtractOptions::default()).unwrap();
        assert_eq!(decls.len(), 1);

        match &decls[0] {
            Declaration::Function { name, signature, .. } => {
                assert_eq!(name, "Handle");
                assert!(signature.contains("*Handler"));
            }
            _ => panic!("expected function"),
        }
    }
}
