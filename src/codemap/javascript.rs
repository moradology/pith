//! JavaScript/JSX codemap extraction using tree-sitter.
//!
//! Uses the TypeScript parser which handles JavaScript as well.

use crate::filter::Language;
use super::{Declaration, ExtractOptions, Import};

/// Extract imports and declarations from JavaScript/JSX source code.
///
/// Delegates to the TypeScript extractor since tree-sitter-typescript
/// handles JavaScript syntax.
pub fn extract(
    content: &str,
    language: Language,
    options: &ExtractOptions,
) -> Result<(Vec<Import>, Vec<Declaration>), String> {
    // Use TypeScript extractor - it handles JS fine
    // Map JSX to TSX for proper JSX handling
    let ts_lang = match language {
        Language::Jsx => Language::Tsx,
        _ => Language::TypeScript,
    };

    super::typescript::extract(content, ts_lang, options)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_js_function() {
        let code = r#"
export function greet(name) {
    return `Hello, ${name}`;
}
"#;
        let (_, decls) = extract(code, Language::JavaScript, &ExtractOptions::default()).unwrap();
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].name(), "greet");
    }

    #[test]
    fn test_extract_js_class() {
        let code = r#"
export class Handler {
    async handle(req) {
        return new Response();
    }
}
"#;
        let (_, decls) = extract(code, Language::JavaScript, &ExtractOptions::default()).unwrap();
        assert_eq!(decls.len(), 1);

        match &decls[0] {
            Declaration::Class { name, .. } => {
                assert_eq!(name, "Handler");
            }
            _ => panic!("expected class"),
        }
    }

    #[test]
    fn test_extract_js_import() {
        let code = r#"
import { useState } from 'react';
"#;
        let (imports, _) = extract(code, Language::JavaScript, &ExtractOptions::default()).unwrap();
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].source, "react");
    }
}
