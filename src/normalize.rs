use unicode_normalization::char::{decompose_canonical, is_combining_mark};

const SEPARATORS: [char; 8] = ['.', '-', '_', '/', '\\', ' ', ':', '('];

/// A lowercased, accent-stripped view of a string, with a map from every
/// normalized character index back to its source character index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Normalized {
    chars: Vec<char>,
    origins: Vec<usize>,
    source: Vec<char>,
}

impl Normalized {
    /// Lowercases `text`, decomposes it canonically, then drops combining
    /// marks, recording where each surviving character came from.
    pub fn new(text: &str) -> Self {
        let source: Vec<char> = text.chars().collect();
        let mut chars: Vec<char> = Vec::with_capacity(source.len());
        let mut origins: Vec<usize> = Vec::with_capacity(source.len());
        for (index, original) in source.iter().enumerate() {
            for lowered in original.to_lowercase() {
                decompose_canonical(lowered, |decomposed| {
                    if !is_combining_mark(decomposed) {
                        chars.push(decomposed);
                        origins.push(index);
                    }
                });
            }
        }
        Self {
            chars,
            origins,
            source,
        }
    }

    /// The normalized characters.
    pub fn chars(&self) -> &[char] {
        &self.chars
    }

    /// The normalized characters as a string.
    pub fn text(&self) -> String {
        self.chars.iter().collect()
    }

    /// Number of normalized characters.
    pub fn len(&self) -> usize {
        self.chars.len()
    }

    /// True when normalization produced no character at all.
    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }

    /// Source character index the normalized character at `index` came from.
    pub fn source_index(&self, index: usize) -> Option<usize> {
        self.origins.get(index).copied()
    }

    /// True when the source character behind `index` is an uppercase letter
    /// following a lowercase one.
    pub fn is_camel_hump(&self, index: usize) -> bool {
        let Some(origin) = self.source_index(index) else {
            return false;
        };
        let (Some(current), Some(previous)) = (
            self.source.get(origin),
            origin
                .checked_sub(1)
                .and_then(|before| self.source.get(before)),
        ) else {
            return false;
        };
        current.is_uppercase() && previous.is_lowercase()
    }

    /// True when `index` starts a word: index 0, right after a separator, or
    /// a camelCase hump read on the source string.
    pub fn is_word_start(&self, index: usize) -> bool {
        if index >= self.chars.len() {
            return false;
        }
        if index == 0 {
            return true;
        }
        let after_separator = index
            .checked_sub(1)
            .and_then(|before| self.chars.get(before))
            .is_some_and(|previous| SEPARATORS.contains(previous));
        after_separator || self.is_camel_hump(index)
    }
}

#[cfg(test)]
mod tests {
    use super::Normalized;

    #[test]
    fn lowercases_ascii_and_keeps_every_index_aligned() {
        let normalized = Normalized::new("RipGrep");
        assert_eq!(normalized.text(), "ripgrep");
        assert_eq!(normalized.len(), 7);
        for index in 0..normalized.len() {
            assert_eq!(normalized.source_index(index), Some(index));
        }
    }

    #[test]
    fn strips_accents_from_a_precomposed_letter() {
        let normalized = Normalized::new("Réunions");
        assert_eq!(normalized.text(), "reunions");
    }

    #[test]
    fn maps_indices_back_through_a_multibyte_source_character() {
        let normalized = Normalized::new("Réunions");
        assert_eq!(normalized.source_index(1), Some(1));
        assert_eq!(normalized.source_index(2), Some(2));
        let source: Vec<char> = "Réunions".chars().collect();
        assert_eq!(source.get(1), Some(&'é'));
    }

    #[test]
    fn maps_indices_back_after_dropping_a_standalone_combining_mark() {
        let normalized = Normalized::new("Re\u{301}unions");
        assert_eq!(normalized.text(), "reunions");
        assert_eq!(normalized.len(), 8);
        assert_eq!(normalized.source_index(0), Some(0));
        assert_eq!(normalized.source_index(1), Some(1));
        assert_eq!(normalized.source_index(2), Some(3));
        assert_eq!(normalized.source_index(7), Some(8));
    }

    #[test]
    fn an_empty_string_normalizes_to_nothing() {
        let normalized = Normalized::new("");
        assert!(normalized.is_empty());
        assert_eq!(normalized.source_index(0), None);
        assert!(!normalized.is_word_start(0));
    }

    #[test]
    fn detects_camel_humps_on_the_source_string() {
        let normalized = Normalized::new("ripGrep");
        assert!(normalized.is_camel_hump(3));
        assert!(!normalized.is_camel_hump(0));
        assert!(!normalized.is_camel_hump(4));
        assert!(!Normalized::new("ripgrep").is_camel_hump(3));
    }

    #[test]
    fn treats_index_zero_and_post_separator_positions_as_word_starts() {
        let normalized = Normalized::new("rip-grep");
        assert!(normalized.is_word_start(0));
        assert!(normalized.is_word_start(4));
        assert!(!normalized.is_word_start(1));
        assert!(!normalized.is_word_start(8));
        for separator in ['.', '-', '_', '/', '\\', ' ', ':', '('] {
            let text = format!("a{separator}b");
            assert!(Normalized::new(&text).is_word_start(2));
        }
    }
}
