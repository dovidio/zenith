//! The semantic vocabulary of ink.
//!
//! A tone says what a token *means*, not what colour it should be. This module is
//! deliberately free of any UI dependency: the window maps tones onto a palette, a
//! terminal could map them onto ANSI styles, and the parser never needs to know which.
//!
//! See `theme.rs` for the tonal ladder that gives each tone its ink.

/// The complete vocabulary of ink in zenith. Fifteen tones, no more.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Tone {
    /// Identifiers, variables, everything unnamed.
    Plain,
    /// Prose in the margin: line and block comments.
    Comment,
    /// Doc comments — comments that address the reader directly.
    Doc,
    /// Control and declaration keywords: the darkest ink on the page.
    Keyword,
    /// Modifiers, storage classes: keywords spoken softly.
    KeywordSoft,
    /// Types, traits, classes, tags.
    Type,
    /// Function and macro names.
    Call,
    /// Named constants and macro invocations.
    Constant,
    /// String literals.
    Str,
    /// Numeric and boolean literals.
    Number,
    /// Operators.
    Operator,
    /// Punctuation, brackets, separators. Nearly a whisper.
    Punct,
    /// Attributes, decorators, annotations, markup tags.
    Meta,
    /// A code block inside a prose document: always monospace, never serif, even when
    /// the rest of the file is being set as prose.
    Raw,
    /// Something the parser could not make sense of.
    Invalid,
}

impl Tone {
    pub const COUNT: usize = 15;

    pub const ALL: [Tone; Tone::COUNT] = [
        Tone::Plain,
        Tone::Comment,
        Tone::Doc,
        Tone::Keyword,
        Tone::KeywordSoft,
        Tone::Type,
        Tone::Call,
        Tone::Constant,
        Tone::Str,
        Tone::Number,
        Tone::Operator,
        Tone::Punct,
        Tone::Meta,
        Tone::Raw,
        Tone::Invalid,
    ];

    #[inline]
    pub fn index(self) -> usize {
        match self {
            Tone::Plain => 0,
            Tone::Comment => 1,
            Tone::Doc => 2,
            Tone::Keyword => 3,
            Tone::KeywordSoft => 4,
            Tone::Type => 5,
            Tone::Call => 6,
            Tone::Constant => 7,
            Tone::Str => 8,
            Tone::Number => 9,
            Tone::Operator => 10,
            Tone::Punct => 11,
            Tone::Meta => 12,
            Tone::Raw => 13,
            Tone::Invalid => 14,
        }
    }

    /// Should this tone be set in the serif prose face rather than the mono face?
    /// Comments are prose, so they are typeset as prose. Nothing else is.
    #[inline]
    pub fn is_prose(self) -> bool {
        matches!(self, Tone::Comment | Tone::Doc)
    }
}

#[cfg(test)]
mod tests {
    use super::Tone;

    #[test]
    fn every_tone_has_its_own_slot() {
        // The palette is indexed by `Tone::index`, so a duplicate or a gap would
        // silently give one token class another one's ink.
        let mut seen = [false; Tone::COUNT];
        for tone in Tone::ALL {
            let i = tone.index();
            assert!(i < Tone::COUNT, "{tone:?} indexes past the palette");
            assert!(!seen[i], "{tone:?} shares slot {i}");
            seen[i] = true;
        }
        assert!(seen.iter().all(|&s| s), "every slot must be claimed");
    }

    #[test]
    fn only_comments_are_prose() {
        for tone in Tone::ALL {
            assert_eq!(
                tone.is_prose(),
                matches!(tone, Tone::Comment | Tone::Doc),
                "{tone:?}"
            );
        }
    }
}
