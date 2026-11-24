// SPDX-License-Identifier: GPL-3.0-or-later
//! The query language of the launcher (`docs/spec/features/search.md`, «Query language»): a
//! prefix picks one provider, plain text asks all of them.

/// The longest query matched, in characters; the rest is dropped (spec «Edge cases»).
pub const MAX_QUERY_CHARS: usize = 256;

/// Which providers a query asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// Plain text: every provider, ranked.
    All,
    /// `= expr`: the calculator only.
    Calculator,
    /// `> text`: system actions only.
    SystemActions,
    /// `. text` or `file:text`: files only.
    Files,
    /// `# text`: clipboard history only.
    Clipboard,
    /// `: text`: emoji only.
    Emoji,
}

/// A parsed query: the scope and the text after the prefix, trimmed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Query {
    pub scope: Scope,
    pub text: String,
}

/// Cut `raw` to [`MAX_QUERY_CHARS`] characters (never inside a character).
pub fn truncate(raw: &str) -> &str {
    match raw.char_indices().nth(MAX_QUERY_CHARS) {
        Some((at, _)) => &raw[..at],
        None => raw,
    }
}

/// Parse the text of the search field.
///
/// `=`, `>`, `#` and `:` work with or without a space after them. `.` needs a space (or nothing)
/// after it, so `.5*2` stays a calculation. `file:` is case-insensitive.
pub fn parse(raw: &str) -> Query {
    let text = truncate(raw).trim_start();
    let (scope, rest) = split_prefix(text);
    Query {
        scope,
        text: rest.trim().to_owned(),
    }
}

fn split_prefix(text: &str) -> (Scope, &str) {
    let mut chars = text.chars();
    let first = chars.next();
    let rest = chars.as_str();
    match first {
        Some('=') => (Scope::Calculator, rest),
        Some('>') => (Scope::SystemActions, rest),
        Some('#') => (Scope::Clipboard, rest),
        Some(':') => (Scope::Emoji, rest),
        Some('.') if rest.is_empty() || rest.starts_with(char::is_whitespace) => {
            (Scope::Files, rest)
        }
        _ if text.len() >= 5
            && text.is_char_boundary(5)
            && text[..5].eq_ignore_ascii_case("file:") =>
        {
            (Scope::Files, &text[5..])
        }
        _ => (Scope::All, text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(scope: Scope, text: &str) -> Query {
        Query {
            scope,
            text: text.to_owned(),
        }
    }

    #[test]
    fn prefixes_route_to_providers() {
        for (input, want) in [
            ("text ed", q(Scope::All, "text ed")),
            ("  firefox  ", q(Scope::All, "firefox")),
            ("= 12*(3+4)", q(Scope::Calculator, "12*(3+4)")),
            ("=2+2", q(Scope::Calculator, "2+2")),
            ("> lock", q(Scope::SystemActions, "lock")),
            (">", q(Scope::SystemActions, "")),
            (". report", q(Scope::Files, "report")),
            (".", q(Scope::Files, "")),
            ("file:notes", q(Scope::Files, "notes")),
            ("FILE: notes", q(Scope::Files, "notes")),
            ("# token", q(Scope::Clipboard, "token")),
            (": smile", q(Scope::Emoji, "smile")),
            (":серце", q(Scope::Emoji, "серце")),
            (".5*2", q(Scope::All, ".5*2")),
            ("files", q(Scope::All, "files")),
            ("", q(Scope::All, "")),
            ("ф", q(Scope::All, "ф")),
        ] {
            assert_eq!(parse(input), want, "{input:?}");
        }
    }

    #[test]
    fn long_queries_are_cut_at_256_characters() {
        let long = "ї".repeat(300);
        assert_eq!(truncate(&long).chars().count(), MAX_QUERY_CHARS);
        assert_eq!(parse(&long).text.chars().count(), MAX_QUERY_CHARS);
        assert_eq!(truncate("short"), "short");
    }
}
