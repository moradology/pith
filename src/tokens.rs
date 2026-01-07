//! Token counting for LLM context budget management.
//!
//! Uses tiktoken-rs for accurate OpenAI-compatible token counts,
//! with a fallback heuristic when tiktoken is unavailable.

use std::sync::OnceLock;
use tiktoken_rs::CoreBPE;

/// Token encoding to use for counting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Encoding {
    /// cl100k_base: GPT-4, GPT-3.5-turbo, ChatGPT
    #[default]
    Cl100kBase,
    /// o200k_base: GPT-4o
    O200kBase,
}

impl std::fmt::Display for Encoding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Encoding::Cl100kBase => write!(f, "cl100k_base"),
            Encoding::O200kBase => write!(f, "o200k_base"),
        }
    }
}

impl std::str::FromStr for Encoding {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "cl100k" | "cl100k_base" => Ok(Encoding::Cl100kBase),
            "o200k" | "o200k_base" => Ok(Encoding::O200kBase),
            _ => Err(format!("unknown encoding: {}", s)),
        }
    }
}

// Cached tokenizers - initialized once per encoding
static CL100K: OnceLock<Option<CoreBPE>> = OnceLock::new();
static O200K: OnceLock<Option<CoreBPE>> = OnceLock::new();

fn get_tokenizer(encoding: Encoding) -> Option<&'static CoreBPE> {
    match encoding {
        Encoding::Cl100kBase => CL100K
            .get_or_init(|| tiktoken_rs::cl100k_base().ok())
            .as_ref(),
        Encoding::O200kBase => O200K
            .get_or_init(|| tiktoken_rs::o200k_base().ok())
            .as_ref(),
    }
}

/// Count tokens using tiktoken.
fn tiktoken_count(text: &str, encoding: Encoding) -> Option<usize> {
    let bpe = get_tokenizer(encoding)?;
    Some(bpe.encode_ordinary(text).len())
}

/// Fallback heuristic: ~4 characters per token.
fn fallback_count(text: &str) -> usize {
    // Rough approximation based on average English text
    // Code tends to have ~3.5 chars/token, prose ~4.2
    // Using 4 as middle ground
    (text.len() + 3) / 4
}

/// Count tokens in text using the default encoding (cl100k_base).
///
/// This function never fails - it falls back to a heuristic if
/// tiktoken is unavailable.
///
/// # Examples
///
/// ```
/// use pith::tokens::count_tokens;
///
/// let count = count_tokens("Hello, world!");
/// assert!(count > 0);
/// ```
pub fn count_tokens(text: &str) -> usize {
    count_tokens_with_encoding(text, Encoding::default())
}

/// Count tokens in text using the specified encoding.
///
/// Falls back to a character-based heuristic (~4 chars/token)
/// if tiktoken is unavailable.
///
/// # Examples
///
/// ```
/// use pith::tokens::{count_tokens_with_encoding, Encoding};
///
/// let count = count_tokens_with_encoding("Hello, world!", Encoding::O200kBase);
/// assert!(count > 0);
/// ```
pub fn count_tokens_with_encoding(text: &str, encoding: Encoding) -> usize {
    tiktoken_count(text, encoding).unwrap_or_else(|| fallback_count(text))
}

/// Reusable token counter with cached tokenizer.
///
/// Use this when counting tokens for many strings to avoid
/// repeated tokenizer lookups.
///
/// # Examples
///
/// ```
/// use pith::tokens::{TokenCounter, Encoding};
///
/// let counter = TokenCounter::new(Encoding::Cl100kBase);
/// let count1 = counter.count("First text");
/// let count2 = counter.count("Second text");
/// ```
pub struct TokenCounter {
    encoding: Encoding,
}

impl TokenCounter {
    /// Create a new token counter with the specified encoding.
    pub fn new(encoding: Encoding) -> Self {
        Self { encoding }
    }

    /// Count tokens in the given text.
    pub fn count(&self, text: &str) -> usize {
        count_tokens_with_encoding(text, self.encoding)
    }

    /// Count tokens for multiple texts.
    pub fn count_many<'a>(&self, texts: impl IntoIterator<Item = &'a str>) -> Vec<usize> {
        texts.into_iter().map(|t| self.count(t)).collect()
    }

    /// Get the encoding this counter uses.
    pub fn encoding(&self) -> Encoding {
        self.encoding
    }
}

impl Default for TokenCounter {
    fn default() -> Self {
        Self::new(Encoding::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_string() {
        assert_eq!(count_tokens(""), 0);
    }

    #[test]
    fn test_simple_text() {
        let count = count_tokens("Hello, world!");
        // Should be a small number of tokens
        assert!(count > 0 && count < 10);
    }

    #[test]
    fn test_code() {
        let code = r#"
fn main() {
    println!("Hello, world!");
}
"#;
        let count = count_tokens(code);
        assert!(count > 0);
    }

    #[test]
    fn test_fallback_approximation() {
        // Test the fallback directly
        assert_eq!(fallback_count(""), 0);
        assert_eq!(fallback_count("a"), 1);
        assert_eq!(fallback_count("abcd"), 1);
        assert_eq!(fallback_count("abcde"), 2);
        assert_eq!(fallback_count("abcdefgh"), 2);
    }

    #[test]
    fn test_encoding_from_str() {
        assert_eq!("cl100k".parse::<Encoding>().unwrap(), Encoding::Cl100kBase);
        assert_eq!(
            "cl100k_base".parse::<Encoding>().unwrap(),
            Encoding::Cl100kBase
        );
        assert_eq!("o200k".parse::<Encoding>().unwrap(), Encoding::O200kBase);
        assert!("invalid".parse::<Encoding>().is_err());
    }

    #[test]
    fn test_token_counter() {
        let counter = TokenCounter::new(Encoding::Cl100kBase);
        let count = counter.count("Test text");
        assert!(count > 0);
    }

    #[test]
    fn test_count_many() {
        let counter = TokenCounter::default();
        let counts = counter.count_many(["one", "two", "three"]);
        assert_eq!(counts.len(), 3);
        assert!(counts.iter().all(|&c| c > 0));
    }
}
