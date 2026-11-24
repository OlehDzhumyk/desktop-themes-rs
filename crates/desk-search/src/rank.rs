// SPDX-License-Identifier: GPL-3.0-or-later
//! Ranking (`docs/spec/features/search.md`, «Ranking»; AC-SRCH-02, AC-SRCH-04).
//!
//! Tiers, best first: exact > prefix > word prefix > substring > fuzzy (in-order letters or one
//! or two typos). Text is folded (lower case, one space, typographic apostrophes made plain)
//! before it is compared. A query typed in the wrong keyboard layout (Ukrainian ЙЦУКЕН and US
//! QWERTY, in both directions) is matched too, slightly below the same match typed right.

/// The score of an exact match; the other tiers are below it.
pub const EXACT: u32 = 1000;
/// The base score of a prefix match (plus up to [`CLOSENESS`]).
pub const PREFIX: u32 = 800;
/// The base score of a word prefix match.
pub const WORD_PREFIX: u32 = 600;
/// The base score of a substring match.
pub const SUBSTRING: u32 = 400;
/// The highest fuzzy score.
pub const FUZZY_MAX: u32 = 300;
/// The most a match gains inside its tier for covering more of the candidate.
pub const CLOSENESS: u32 = 50;
/// What a match through the other keyboard layout loses.
pub const LAYOUT_PENALTY: u32 = 10;

/// Pairs of (Ukrainian ЙЦУКЕН, US QWERTY) on the same key, lower case.
const UA_US: &[(char, char)] = &[
    ('й', 'q'),
    ('ц', 'w'),
    ('у', 'e'),
    ('к', 'r'),
    ('е', 't'),
    ('н', 'y'),
    ('г', 'u'),
    ('ш', 'i'),
    ('щ', 'o'),
    ('з', 'p'),
    ('х', '['),
    ('ї', ']'),
    ('ф', 'a'),
    ('і', 's'),
    ('в', 'd'),
    ('а', 'f'),
    ('п', 'g'),
    ('р', 'h'),
    ('о', 'j'),
    ('л', 'k'),
    ('д', 'l'),
    ('ж', ';'),
    ('є', '\''),
    ('я', 'z'),
    ('ч', 'x'),
    ('с', 'c'),
    ('м', 'v'),
    ('и', 'b'),
    ('т', 'n'),
    ('ь', 'm'),
    ('б', ','),
    ('ю', '.'),
    ('ґ', '\\'),
];

/// Cyrillic letters of other layouts on the same keys, mapped to QWERTY only (one way).
const OTHER_CYRILLIC_US: &[(char, char)] = &[('ы', 's'), ('э', '\''), ('ъ', ']'), ('ё', '`')];

/// The QWERTY letter on the key of a Cyrillic letter.
fn to_qwerty(c: char) -> Option<char> {
    UA_US
        .iter()
        .chain(OTHER_CYRILLIC_US)
        .find(|(ua, _)| *ua == c)
        .map(|(_, us)| *us)
}

/// The Ukrainian letter on the key of a QWERTY character.
fn to_ukrainian(c: char) -> Option<char> {
    UA_US.iter().find(|(_, us)| *us == c).map(|(ua, _)| *ua)
}

/// Text typed in the Ukrainian layout as if the US layout was on (`сдшзищфкв` → `clipboard`).
/// `None` when no letter changes.
pub fn ukrainian_to_qwerty(text: &str) -> Option<String> {
    convert(text, to_qwerty, char::is_alphabetic)
}

/// Text typed in the US layout as if the Ukrainian layout was on (`ntrcn` → `текст`).
/// `None` when no ASCII letter changes.
pub fn qwerty_to_ukrainian(text: &str) -> Option<String> {
    convert(text, to_ukrainian, |c| c.is_ascii_alphabetic())
}

/// Map every character through `map` (others pass through); succeed only when a letter (as
/// `letter` says) changed, so digits and punctuation alone are never converted.
fn convert(text: &str, map: fn(char) -> Option<char>, letter: fn(char) -> bool) -> Option<String> {
    let mut changed = false;
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match map(c) {
            Some(m) => {
                changed |= letter(c);
                out.push(m);
            }
            None => out.push(c),
        }
    }
    changed.then_some(out)
}

/// Fold text for comparing: lower case (Unicode), typographic apostrophes as `'`, runs of space
/// and punctuation that separates words as one space, trimmed.
pub fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for c in text.chars().flat_map(char::to_lowercase) {
        let c = match c {
            '’' | 'ʼ' | '`' | '\u{2018}' => '\'',
            'ё' => 'е',
            c => c,
        };
        if c.is_whitespace() || matches!(c, '-' | '_' | '/' | '|' | '·' | '—' | '–') {
            space = !out.is_empty();
            continue;
        }
        if space {
            out.push(' ');
            space = false;
        }
        out.push(c);
    }
    out
}

/// The forms of a folded query to try: itself, then the other-layout readings.
pub fn variants(folded_query: &str) -> Vec<(String, u32)> {
    let mut out = vec![(folded_query.to_owned(), 0)];
    for v in [
        ukrainian_to_qwerty(folded_query),
        qwerty_to_ukrainian(folded_query),
    ]
    .into_iter()
    .flatten()
    {
        if !out.iter().any(|(o, _)| *o == v) {
            out.push((v, LAYOUT_PENALTY));
        }
    }
    out
}

/// A bonus up to [`CLOSENESS`] for how much of the candidate the query covers.
fn closeness(query_len: usize, candidate_len: usize) -> u32 {
    if candidate_len == 0 {
        return 0;
    }
    let ratio = query_len.min(candidate_len) * CLOSENESS as usize / candidate_len;
    u32::try_from(ratio).unwrap_or(CLOSENESS)
}

/// Score a folded query against a folded candidate; `None` when it does not match at all.
pub fn score_folded(query: &str, candidate: &str) -> Option<u32> {
    if query.is_empty() || candidate.is_empty() {
        return None;
    }
    let qlen = query.chars().count();
    let clen = candidate.chars().count();
    if candidate == query {
        return Some(EXACT);
    }
    if candidate.starts_with(query) {
        return Some(PREFIX + closeness(qlen, clen));
    }
    if word_prefix(query, candidate) {
        return Some(WORD_PREFIX + closeness(qlen, clen));
    }
    if candidate.contains(query) {
        return Some(SUBSTRING + closeness(qlen, clen));
    }
    fuzzy(query, candidate)
}

/// Every word of the query starts some word of the candidate, each candidate word used once.
fn word_prefix(query: &str, candidate: &str) -> bool {
    let words: Vec<&str> = candidate.split(' ').collect();
    let mut used = vec![false; words.len()];
    query.split(' ').all(|q| {
        let hit = words
            .iter()
            .enumerate()
            .find(|(i, w)| !used[*i] && w.starts_with(q));
        match hit {
            Some((i, _)) => {
                used[i] = true;
                true
            }
            None => false,
        }
    })
}

/// Fuzzy tier: one or two typos against a word start, or all letters in order.
fn fuzzy(query: &str, candidate: &str) -> Option<u32> {
    let q: Vec<char> = query.chars().filter(|c| *c != ' ').collect();
    if q.len() < 2 {
        return None;
    }
    let mut best: Option<u32> = None;
    if q.len() >= 4 {
        let allowed = if q.len() >= 8 { 2 } else { 1 };
        let c: Vec<char> = candidate.chars().collect();
        let starts = std::iter::once(0).chain(
            c.iter()
                .enumerate()
                .filter(|(_, ch)| **ch == ' ')
                .map(|(i, _)| i + 1),
        );
        for start in starts {
            let tail = &c[start..];
            // the word start may be one shorter or longer than the query (a dropped or extra letter)
            for len in q.len().saturating_sub(1)..=q.len() + 1 {
                if len == 0 || len > tail.len() {
                    continue;
                }
                let d = edit_distance(&q, &tail[..len]);
                if d <= allowed {
                    let s = FUZZY_MAX - 40 * u32::try_from(d).unwrap_or(2);
                    best = best.max(Some(s));
                }
            }
        }
    }
    if best.is_none()
        && let Some(span) = subsequence_span(&q, candidate)
    {
        // 100 to 200: tighter spans rank higher
        let tight = q.len() * 100 / span.max(1);
        best = Some(100 + u32::try_from(tight.min(100)).unwrap_or(100));
    }
    best
}

/// The shortest span of `candidate` (in characters) holding the letters of `q` in order,
/// greedily from each possible start.
fn subsequence_span(q: &[char], candidate: &str) -> Option<usize> {
    let c: Vec<char> = candidate.chars().collect();
    let mut best: Option<usize> = None;
    for start in (0..c.len()).filter(|&i| c[i] == q[0]) {
        let mut qi = 0;
        for (i, ch) in c.iter().enumerate().skip(start) {
            if *ch == q[qi] {
                qi += 1;
                if qi == q.len() {
                    let span = i - start + 1;
                    best = Some(best.map_or(span, |b| b.min(span)));
                    break;
                }
            }
        }
    }
    best
}

/// Optimal string alignment distance (Levenshtein plus adjacent transposition).
fn edit_distance(a: &[char], b: &[char]) -> usize {
    let width = b.len() + 1;
    let mut d = vec![0usize; (a.len() + 1) * width];
    for i in 0..=a.len() {
        d[i * width] = i;
    }
    for (j, cell) in d.iter_mut().enumerate().take(width) {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut v = (d[(i - 1) * width + j] + 1)
                .min(d[i * width + j - 1] + 1)
                .min(d[(i - 1) * width + j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(d[(i - 2) * width + j - 2] + 1);
            }
            d[i * width + j] = v;
        }
    }
    d[a.len() * width + b.len()]
}

/// One searchable text of an item and its weight in percent (the name is 100).
#[derive(Clone, Copy, Debug)]
pub struct Field<'a> {
    pub text: &'a str,
    pub weight: u32,
}

/// The best score of a raw query against the fields of one item, over all layout variants.
/// `variants` comes from [`variants`] of the folded query; fields are folded here.
pub fn score_item(variants: &[(String, u32)], fields: &[Field<'_>]) -> Option<u32> {
    let mut best: Option<u32> = None;
    for field in fields {
        let folded = fold(field.text);
        for (q, penalty) in variants {
            if let Some(s) = score_folded(q, &folded) {
                let s = (s * field.weight / 100).saturating_sub(*penalty);
                best = best.max(Some(s));
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn score(q: &str, c: &str) -> Option<u32> {
        score_item(
            &variants(&fold(q)),
            &[Field {
                text: c,
                weight: 100,
            }],
        )
    }

    #[test]
    fn tiers_are_ordered() {
        let exact = score("files", "Files").unwrap();
        let prefix = score("fil", "Files").unwrap();
        let word = score("ed", "Text Editor").unwrap();
        let sub = score("dito", "Text Editor").unwrap();
        let fuzzy = score("txe", "Text Editor").unwrap();
        assert_eq!(exact, EXACT);
        assert!(exact > prefix && prefix > word && word > sub && sub > fuzzy);
        assert!(fuzzy <= FUZZY_MAX);
        assert_eq!(score("xyz", "Text Editor"), None);
    }

    #[test]
    fn ac_srch_02_text_ed_is_a_strong_match_for_text_editor() {
        let s = score("text ed", "Text Editor").unwrap();
        assert!(s >= PREFIX, "{s}");
        // words in another order still match as word prefixes
        assert!(score("ed text", "Text Editor").unwrap() >= WORD_PREFIX);
        // a longer name with the same prefix ranks lower
        assert!(s > score("text ed", "Text Editor Extra Tools").unwrap());
    }

    #[test]
    fn unicode_case_folding() {
        assert_eq!(score("ТЕРМІНАЛ", "термінал"), Some(EXACT));
        assert_eq!(fold("  Ґанок’s   Text-Editor "), "ґанок's text editor");
        assert_eq!(score("ёлка", "Елка"), Some(EXACT));
    }

    #[test]
    fn ac_srch_04_layout_mixups_both_ways() {
        assert_eq!(
            ukrainian_to_qwerty("сдшзищфкв").as_deref(),
            Some("clipboard")
        );
        assert_eq!(qwerty_to_ukrainian("ntrcn").as_deref(), Some("текст"));
        assert_eq!(
            qwerty_to_ukrainian("clipboard").as_deref(),
            Some("сдшзищфкв")
        );
        assert_eq!(ukrainian_to_qwerty("clipboard"), None);
        assert_eq!(qwerty_to_ukrainian("сдш"), None);
        // a query switched halfway: the wrong-layout part is converted, the rest stays
        assert_eq!(qwerty_to_ukrainian("cд").as_deref(), Some("сд"));
        assert_eq!(qwerty_to_ukrainian("2.5,"), None);
        assert_eq!(ukrainian_to_qwerty("ыуфкср").as_deref(), Some("search"));

        let s = score("сдшзищфкв", "Clipboard").unwrap();
        assert_eq!(s, EXACT - LAYOUT_PENALTY);
        assert!(score("сдшз", "Clipboard history").unwrap() >= PREFIX - LAYOUT_PENALTY);
        assert!(score("ntrcn", "Текстовий редактор").unwrap() >= PREFIX - LAYOUT_PENALTY);
    }

    #[test]
    fn typos_and_subsequences() {
        // one dropped letter
        assert!(score("термнал", "Термінал").unwrap() >= FUZZY_MAX - 40);
        // a transposition
        assert!(score("тремінал", "Термінал").unwrap() >= FUZZY_MAX - 40);
        // letters in order
        let s = score("ffx", "Firefox").unwrap();
        assert!((100..=200).contains(&s), "{s}");
        assert_eq!(score("q", "Firefox"), None);
        assert_eq!(score("fq", "Firefox"), None);
    }

    #[test]
    fn weights_scale_scores() {
        let v = variants(&fold("editor"));
        let name = score_item(
            &v,
            &[Field {
                text: "Editor",
                weight: 100,
            }],
        )
        .unwrap();
        let kw = score_item(
            &v,
            &[Field {
                text: "editor",
                weight: 60,
            }],
        )
        .unwrap();
        assert_eq!(name, EXACT);
        assert_eq!(kw, EXACT * 60 / 100);
    }

    #[test]
    fn edit_distance_counts() {
        let d = |a: &str, b: &str| {
            edit_distance(
                &a.chars().collect::<Vec<_>>(),
                &b.chars().collect::<Vec<_>>(),
            )
        };
        assert_eq!(d("abc", "abc"), 0);
        assert_eq!(d("abc", "acb"), 1);
        assert_eq!(d("abc", "ab"), 1);
        assert_eq!(d("", "ab"), 2);
    }
}
