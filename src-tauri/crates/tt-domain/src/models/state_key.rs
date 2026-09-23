//! State keys: shape, patterns, and the tolerance rules used to accept them.
//!
//! A key is a `/`-separated path whose segments mean nothing to the domain —
//! the user's declaration names them. This module owns three things that must
//! stay deterministic and testable without IO:
//!
//! - the structural shape a usable key must have,
//! - the pattern syntax a declaration may use (`*`, `**`, or `/pattern/flags`),
//! - the mechanical normalization that lets ordinary transcription drift pass
//!   while still refusing anything that would require guessing.

use std::fmt;

use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

/// Safety bound on `/`-separated segments.
///
/// This is not a schema. The allowed key space — including what the top-level
/// segments are called and how deep keys go — comes from the user's
/// declaration; the domain only rejects keys that are structurally unusable.
const MAX_KEY_SEGMENTS: usize = 8;
/// Safety bound on the whole key, counted in characters.
const MAX_KEY_CHARS: usize = 255;
/// Safety bound on one key segment, counted in characters.
const MAX_SEGMENT_CHARS: usize = 64;

/// One segment's worth of keys, so a bare `*` matches exactly one segment:
/// keys never contain an empty segment.
const NARROW_WILDCARD: &str = "[^/]*";
/// `**` covers one or more whole segments, never zero.
const WIDE_WILDCARD: &str = "[^/]+(?:/[^/]+)*";

/// A validated `/`-separated state key, such as `环境/日期`.
///
/// Keys carry no meaning in the domain. Segment names, depth, and grouping are
/// whatever the user declares, so this type only guarantees the key is a usable
/// identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StateKey(String);

impl StateKey {
    /// Validate the key shape.
    pub fn parse(raw: &str) -> Result<Self, String> {
        check_key_shape(raw, false)?;
        Ok(Self(raw.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for StateKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Check the shape every key must have, whether it is a literal or a pattern.
///
/// Literal keys may not contain `*` because that character now means
/// wildcard; the `/pattern/flags` form is the escape hatch for a literal
/// asterisk.
fn check_key_shape(raw: &str, allow_wildcards: bool) -> Result<(), String> {
    if raw.is_empty() {
        return Err("key is required".to_string());
    }
    if raw.trim() != raw {
        return Err(format!("key `{raw}` must not have surrounding whitespace"));
    }
    if raw.chars().any(char::is_control) {
        return Err(format!("key `{raw}` must not contain control characters"));
    }
    if raw.chars().count() > MAX_KEY_CHARS {
        return Err(format!("key exceeds {MAX_KEY_CHARS} characters"));
    }

    let segments = raw.split('/').collect::<Vec<_>>();
    if segments.len() > MAX_KEY_SEGMENTS {
        return Err(format!(
            "key `{raw}` must have at most {MAX_KEY_SEGMENTS} `/`-separated segments"
        ));
    }
    for segment in &segments {
        if segment.is_empty() {
            return Err(format!("key `{raw}` must not contain an empty segment"));
        }
        if segment.chars().count() > MAX_SEGMENT_CHARS {
            return Err(format!(
                "key `{raw}` segment `{segment}` exceeds {MAX_SEGMENT_CHARS} characters"
            ));
        }
        if segment.contains('*') && !allow_wildcards {
            return Err(format!(
                "key `{raw}` must not contain `*`; use the `/pattern/flags` form if you need a literal asterisk"
            ));
        }
    }

    Ok(())
}

/// The shape checks the `/pattern/flags` form owes on top of its own syntax.
///
/// Its body is a regex, so it cannot be split on `/` and segment-checked the way
/// a literal key is; whitespace and control characters are still refused for the
/// same reasons they are on every other key.
fn check_regex_key_shape(raw: &str) -> Result<(), String> {
    if raw.trim() != raw {
        return Err(format!(
            "key pattern `{raw}` must not have surrounding whitespace"
        ));
    }
    if raw.chars().any(char::is_control) {
        return Err(format!(
            "key pattern `{raw}` must not contain control characters"
        ));
    }
    Ok(())
}

/// The form a declared key takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyPatternKind {
    /// No wildcard: the declared name is the key.
    Literal,
    /// Segment wildcards (`*`, `**`).
    Wildcard,
    /// A `/pattern/flags` escape hatch.
    Regex,
}

/// A declared key: either a literal name or a pattern covering many keys.
#[derive(Clone)]
pub struct StateKeyPattern {
    raw: String,
    kind: KeyPatternKind,
    compiled: Option<Regex>,
}

impl StateKeyPattern {
    /// Parse a declared key.
    ///
    /// A key that starts with `/` is the `pattern/flags` form — the same syntax
    /// world book keys already use, so nothing new has to be learned. Anything
    /// else is a literal with optional `*` / `**` wildcards.
    pub fn parse(raw: &str) -> Result<Self, String> {
        if raw.starts_with('/') {
            Self::parse_regex_form(raw)
        } else {
            Self::parse_wildcard_form(raw)
        }
    }

    fn parse_wildcard_form(raw: &str) -> Result<Self, String> {
        check_key_shape(raw, true)?;

        let mut body = String::new();
        let mut has_wildcard = false;
        for (index, segment) in raw.split('/').enumerate() {
            if index > 0 {
                body.push('/');
            }
            if segment == "**" {
                has_wildcard = true;
                body.push_str(WIDE_WILDCARD);
            } else if segment.contains('*') {
                has_wildcard = true;
                for (part_index, part) in segment.split('*').enumerate() {
                    if part_index > 0 {
                        body.push_str(NARROW_WILDCARD);
                    }
                    body.push_str(&regex::escape(part));
                }
            } else {
                body.push_str(&regex::escape(segment));
            }
        }

        let compiled = has_wildcard.then(|| compile(&body, false)).transpose()?;

        Ok(Self {
            raw: raw.to_string(),
            kind: if has_wildcard {
                KeyPatternKind::Wildcard
            } else {
                KeyPatternKind::Literal
            },
            compiled,
        })
    }

    fn parse_regex_form(raw: &str) -> Result<Self, String> {
        check_regex_key_shape(raw)?;

        let end = raw
            .rfind('/')
            .expect("a key starting with `/` contains a delimiter");
        if end == 0 {
            return Err(format!("key pattern `{raw}` has an empty body"));
        }

        let body = &raw[1..end];
        let flags = &raw[end + 1..];

        if body.chars().count() > MAX_KEY_CHARS {
            return Err(format!(
                "key pattern `{raw}` exceeds {MAX_KEY_CHARS} characters"
            ));
        }
        if has_unescaped_slash(body) {
            return Err(format!(
                "key pattern `{raw}` must escape `/` as `\\/` inside the body, otherwise the delimiter is ambiguous"
            ));
        }

        let mut case_insensitive = false;
        for flag in flags.chars() {
            match flag {
                'i' => case_insensitive = true,
                'u' => {}
                'g' | 'y' => {
                    return Err(format!(
                        "key pattern `{raw}` uses the JavaScript-only flag `{flag}`; declared keys are always anchored and matched once, so `g` and `y` do not apply"
                    ));
                }
                other => {
                    return Err(format!(
                        "key pattern `{raw}` uses unsupported flag `{other}`; only `i` (case-insensitive) and `u` (unicode, always on) are accepted"
                    ));
                }
            }
        }

        let unescaped = body.replace("\\/", "/");
        let compiled = compile(&unescaped, case_insensitive)?;

        Ok(Self {
            raw: raw.to_string(),
            kind: KeyPatternKind::Regex,
            compiled: Some(compiled),
        })
    }

    pub fn kind(&self) -> KeyPatternKind {
        self.kind
    }

    pub fn is_literal(&self) -> bool {
        self.kind == KeyPatternKind::Literal
    }

    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// Match as written, with no tolerance beyond what the pattern expresses.
    pub fn matches_exact(&self, text: &str) -> bool {
        match &self.compiled {
            Some(regex) => regex.is_match(text),
            None => self.raw == text,
        }
    }

    /// Match with case folding. Used only as a fallback, so a declaration that
    /// deliberately keeps two case variants of one name still resolves exactly.
    pub fn matches_tolerant(&self, text: &str) -> bool {
        match &self.compiled {
            Some(regex) => regex.is_match(text),
            // The ASCII comparison allocates nothing and covers the common case;
            // only a non-ASCII side pays for a Unicode fold.
            None if self.raw.is_ascii() && text.is_ascii() => {
                self.raw.eq_ignore_ascii_case(text)
            }
            None => self.raw.to_lowercase() == text.to_lowercase(),
        }
    }
}

impl fmt::Debug for StateKeyPattern {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StateKeyPattern")
            .field("raw", &self.raw)
            .field("kind", &self.kind)
            .finish()
    }
}

impl PartialEq for StateKeyPattern {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}

impl Eq for StateKeyPattern {}

impl Serialize for StateKeyPattern {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.raw)
    }
}

impl<'de> Deserialize<'de> for StateKeyPattern {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

/// Compile a body into an anchored, whole-key matcher.
///
/// Anchoring is what separates a declared key from a world book key: a world
/// book key searches a chat transcript, so a hit anywhere counts, while a
/// declared key decides which field a name belongs to. `环境/日期` must not
/// swallow `环境/日期/细节`.
fn compile(body: &str, case_insensitive: bool) -> Result<Regex, String> {
    let anchored = format!("^(?:{body})$");
    RegexBuilder::new(&anchored)
        .case_insensitive(case_insensitive)
        .build()
        .map_err(|error| describe_regex_error(&error))
}

fn describe_regex_error(error: &regex::Error) -> String {
    let text = error.to_string();
    let reasons = text
        .lines()
        .map(str::trim)
        .filter_map(|line| line.strip_prefix("error:"))
        .map(str::trim)
        .collect::<Vec<_>>();
    if reasons.is_empty() {
        format!("invalid key pattern: {text}")
    } else {
        format!("invalid key pattern: {}", reasons.join("; "))
    }
}

fn has_unescaped_slash(body: &str) -> bool {
    let mut escaped = false;
    for character in body.chars() {
        match character {
            '\\' => escaped = !escaped,
            '/' if !escaped => return true,
            _ => escaped = false,
        }
    }
    false
}

/// A key after mechanical normalization, plus the steps that changed it.
///
/// Only transformations that are reversible and unambiguous live here: width,
/// separators, and whitespace. Nothing that would require deciding what the
/// caller *meant*.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedKey {
    pub text: String,
    /// The normalization steps that changed the input, for observability.
    pub steps: Vec<&'static str>,
}

/// Normalize a key supplied by a caller.
pub fn normalize_key(raw: &str) -> NormalizedKey {
    let mut steps: Vec<&'static str> = Vec::new();
    let note = |step: &'static str, steps: &mut Vec<&'static str>| {
        if !steps.contains(&step) {
            steps.push(step);
        }
    };

    let folded = fold_fullwidth(raw);
    if folded != raw {
        note("fullwidth", &mut steps);
    }

    let slashed = if folded.contains('\\') {
        note("separator", &mut steps);
        folded.replace('\\', "/")
    } else {
        folded
    };

    let mut segments = Vec::new();
    let mut dropped_empty_segment = false;
    let mut whitespace_changed = false;
    for segment in slashed.split('/') {
        if segment.trim() != segment {
            whitespace_changed = true;
        }
        let collapsed = segment.split_whitespace().collect::<Vec<_>>().join(" ");
        if collapsed != segment.trim() {
            whitespace_changed = true;
        }
        if collapsed.is_empty() {
            dropped_empty_segment = true;
            continue;
        }
        segments.push(collapsed);
    }

    if whitespace_changed {
        note("whitespace", &mut steps);
    }
    if dropped_empty_segment {
        note("separator", &mut steps);
    }

    NormalizedKey {
        text: segments.join("/"),
        steps,
    }
}

fn fold_fullwidth(input: &str) -> String {
    input
        .chars()
        .map(|character| match character {
            '\u{3000}' => ' ',
            '\u{FF01}'..='\u{FF5E}' => {
                char::from_u32(character as u32 - 0xFEE0).unwrap_or(character)
            }
            _ => character,
        })
        .collect()
}

/// Why two declared keys cannot both be kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlapReason {
    /// The same literal name is declared twice.
    IdenticalLiteral,
    /// A literal name is already covered by another pattern.
    LiteralCovered,
    /// Two patterns can match the same key.
    PatternIntersection,
}

/// Two declared keys that can accept the same key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternOverlap {
    pub first: String,
    pub second: String,
    pub reason: OverlapReason,
}

/// Find declarations that can accept the same key.
///
/// Ambiguity is a configuration mistake, so it is reported when the
/// declaration is saved rather than guessed at when a key arrives. Literal
/// names are checked exactly against patterns; two wildcard patterns are
/// compared segment by segment, conservatively — the `/pattern/flags` escape
/// hatch is deliberately not analyzed, so a user who needs an exotic pattern
/// can still express it.
pub fn check_pattern_overlaps(patterns: &[StateKeyPattern]) -> Vec<PatternOverlap> {
    let mut overlaps = Vec::new();
    for (index, left) in patterns.iter().enumerate() {
        for right in patterns.iter().skip(index + 1) {
            if let Some(reason) = overlap_between(left, right) {
                overlaps.push(PatternOverlap {
                    first: left.as_str().to_string(),
                    second: right.as_str().to_string(),
                    reason,
                });
            }
        }
    }
    overlaps
}

fn overlap_between(left: &StateKeyPattern, right: &StateKeyPattern) -> Option<OverlapReason> {
    match (left.kind, right.kind) {
        (KeyPatternKind::Literal, KeyPatternKind::Literal) => left
            .raw
            .eq_ignore_ascii_case(&right.raw)
            .then_some(OverlapReason::IdenticalLiteral),
        (KeyPatternKind::Literal, _) => literal_covered(right, &left.raw),
        (_, KeyPatternKind::Literal) => literal_covered(left, &right.raw),
        _ => patterns_intersect(left, right).then_some(OverlapReason::PatternIntersection),
    }
}

fn literal_covered(pattern: &StateKeyPattern, literal: &str) -> Option<OverlapReason> {
    StateKey::parse(literal)
        .ok()
        .filter(|key| pattern.matches_exact(key.as_str()))
        .map(|_| OverlapReason::LiteralCovered)
}

/// One segment of a wildcard pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    /// A segment with no wildcard.
    Literal(String),
    /// A segment containing `*`, or `**` in a non-final position.
    Star,
    /// A whole-segment `**`.
    Wide,
}

fn patterns_intersect(left: &StateKeyPattern, right: &StateKeyPattern) -> bool {
    if left.kind == KeyPatternKind::Regex || right.kind == KeyPatternKind::Regex {
        // A free-form pattern cannot be compared structurally. Literal names
        // are still checked against it exactly, which covers the common case.
        return false;
    }

    let left_segments = segments_of(left);
    let right_segments = segments_of(right);
    let mut seen = vec![vec![None; right_segments.len() + 1]; left_segments.len() + 1];
    segments_overlap(&left_segments, &right_segments, 0, 0, &mut seen)
}

fn segments_of(pattern: &StateKeyPattern) -> Vec<Segment> {
    pattern
        .raw
        .split('/')
        .map(|segment| {
            if segment == "**" {
                Segment::Wide
            } else if segment.contains('*') {
                Segment::Star
            } else {
                Segment::Literal(segment.to_string())
            }
        })
        .collect()
}

fn segments_overlap(
    left: &[Segment],
    right: &[Segment],
    left_at: usize,
    right_at: usize,
    seen: &mut [Vec<Option<bool>>],
) -> bool {
    if let Some(cached) = seen[left_at][right_at] {
        return cached;
    }

    let result = if left_at == left.len() && right_at == right.len() {
        true
    } else if left_at == left.len() || right_at == right.len() {
        false
    } else {
        match (&left[left_at], &right[right_at]) {
            // `**` covers at least one segment, so every split that consumes
            // one or more of the other side has to be tried.
            (Segment::Wide, _) => ((right_at + 1)..=right.len())
                .any(|take| segments_overlap(left, right, left_at + 1, take, seen)),
            (_, Segment::Wide) => ((left_at + 1)..=left.len())
                .any(|take| segments_overlap(left, right, take, right_at + 1, seen)),
            (Segment::Literal(left_text), Segment::Literal(right_text)) => {
                left_text.eq_ignore_ascii_case(right_text)
                    && segments_overlap(left, right, left_at + 1, right_at + 1, seen)
            }
            // A wildcard segment accepts whatever the other side names, so the
            // comparison continues. This can report a conflict that a narrower
            // pattern would not actually have; the escape hatch above is the
            // way out.
            _ => segments_overlap(left, right, left_at + 1, right_at + 1, seen),
        }
    };

    seen[left_at][right_at] = Some(result);
    result
}

#[cfg(test)]
mod tests {
    use super::{
        KeyPatternKind, OverlapReason, StateKey, StateKeyPattern, check_pattern_overlaps,
        normalize_key,
    };

    fn pattern(raw: &str) -> StateKeyPattern {
        StateKeyPattern::parse(raw).unwrap_or_else(|error| panic!("`{raw}` must parse: {error}"))
    }

    #[test]
    fn a_pattern_is_anchored_so_a_prefix_never_swallows_its_children() {
        let parent = pattern("环境/日期");

        assert!(parent.matches_exact("环境/日期"));
        assert!(
            !parent.matches_exact("环境/日期/细节"),
            "a declared key decides which field a name belongs to, so a prefix must not match"
        );
    }

    #[test]
    fn a_narrow_wildcard_covers_exactly_one_segment() {
        let wildcard = pattern("环境/*/来源");

        assert!(wildcard.matches_exact("环境/日期/来源"));
        assert!(wildcard.matches_exact("环境/时间/来源"));
        assert!(!wildcard.matches_exact("环境/日期/来源/备注"));
        assert!(!wildcard.matches_exact("环境/日期/地点"));
    }

    #[test]
    fn a_wide_wildcard_covers_one_or_more_segments() {
        let wildcard = pattern("环境/**");

        assert!(wildcard.matches_exact("环境/日期"));
        assert!(wildcard.matches_exact("环境/日期/细节/备注"));
        assert!(
            !wildcard.matches_exact("环境"),
            "`**` means one or more segments, never zero"
        );

        let in_the_middle = pattern("环境/**/备注");
        assert!(in_the_middle.matches_exact("环境/日期/备注"));
        assert!(in_the_middle.matches_exact("环境/日期/细节/备注"));
    }

    #[test]
    fn a_wildcard_inside_a_segment_is_a_partial_match() {
        let partial = pattern("环境/日*");

        assert!(partial.matches_exact("环境/日期"));
        assert!(partial.matches_exact("环境/日志"));
        assert!(!partial.matches_exact("环境/时间"));
    }

    #[test]
    fn a_literal_key_rejects_a_star_and_points_at_the_escape_hatch() {
        let error = StateKey::parse("环境/日*").expect_err("`*` must not be a literal key");
        assert!(
            error.contains("/pattern/flags"),
            "error must name the way out: {error}"
        );
    }

    #[test]
    fn the_regex_form_reuses_the_syntax_world_book_keys_already_use() {
        let alternation = pattern("/(日期|时间|时刻)/");
        assert_eq!(alternation.kind(), KeyPatternKind::Regex);
        assert!(alternation.matches_exact("日期"));
        assert!(alternation.matches_exact("时刻"));
        assert!(
            !alternation.matches_exact("日期时间"),
            "the escape hatch is anchored too"
        );

        let with_separator = pattern("/环境\\/(日期|时间)/");
        assert!(with_separator.matches_exact("环境/日期"));
        assert!(!with_separator.matches_exact("环境/日期/细节"));
    }

    #[test]
    fn the_regex_form_is_case_insensitive_only_when_asked() {
        assert!(!pattern("/date/").matches_exact("DATE"));
        assert!(pattern("/date/i").matches_exact("DATE"));
    }

    #[test]
    fn javascript_only_flags_are_rejected_with_a_reason() {
        let error = StateKeyPattern::parse("/日期/g").expect_err("`g` must be rejected");
        assert!(
            error.contains("JavaScript-only"),
            "error must explain why: {error}"
        );

        let unsupported = StateKeyPattern::parse("/日期/x").expect_err("`x` must be rejected");
        assert!(
            unsupported.contains("only `i`"),
            "error must list the accepted set: {unsupported}"
        );
    }

    #[test]
    fn an_unescaped_slash_inside_the_regex_form_is_rejected() {
        let error =
            StateKeyPattern::parse("/环境/日期/").expect_err("an ambiguous delimiter must fail");
        assert!(
            error.contains("escape"),
            "error must say how to fix it: {error}"
        );
    }

    #[test]
    fn normalization_absorbs_transcription_drift() {
        let spaced = normalize_key("  环境 / 日期 / 时间  ");
        assert_eq!(spaced.text, "环境/日期/时间");
        assert!(spaced.steps.contains(&"whitespace"));

        let fullwidth = normalize_key("环境／日期／时间");
        assert_eq!(fullwidth.text, "环境/日期/时间");
        assert!(fullwidth.steps.contains(&"fullwidth"));

        let backslash = normalize_key("环境\\日期\\时间");
        assert_eq!(backslash.text, "环境/日期/时间");
        assert!(backslash.steps.contains(&"separator"));

        let repeated = normalize_key("/环境//日期/");
        assert_eq!(repeated.text, "环境/日期");
        assert!(repeated.steps.contains(&"separator"));
    }

    #[test]
    fn normalization_leaves_an_already_clean_key_untouched() {
        let clean = normalize_key("环境/日期");
        assert_eq!(clean.text, "环境/日期");
        assert!(clean.steps.is_empty(), "a clean key must not report normalization");
    }

    #[test]
    fn case_is_only_folded_by_the_tolerant_comparison() {
        let declared = pattern("Date/Time");

        assert!(!declared.matches_exact("date/time"));
        assert!(declared.matches_tolerant("date/time"));
    }

    #[test]
    fn a_literal_name_inside_a_pattern_is_reported_as_covered() {
        let overlaps = check_pattern_overlaps(&[pattern("环境/*/来源"), pattern("环境/日期/来源")]);

        assert_eq!(overlaps.len(), 1);
        assert_eq!(overlaps[0].reason, OverlapReason::LiteralCovered);
    }

    #[test]
    fn two_wildcard_patterns_that_can_meet_are_reported() {
        let overlaps = check_pattern_overlaps(&[pattern("环境/*"), pattern("环境/**")]);
        assert_eq!(overlaps.len(), 1);
        assert_eq!(overlaps[0].reason, OverlapReason::PatternIntersection);
    }

    #[test]
    fn disjoint_patterns_are_not_reported() {
        assert!(check_pattern_overlaps(&[pattern("环境/*"), pattern("角色/*")]).is_empty());
        assert!(check_pattern_overlaps(&[pattern("环境/日*"), pattern("环境/时间")]).is_empty());
        assert!(
            check_pattern_overlaps(&[pattern("环境/*/来源"), pattern("环境/*/地点")]).is_empty()
        );
    }

    #[test]
    fn the_same_literal_declared_twice_is_reported() {
        let overlaps = check_pattern_overlaps(&[pattern("环境/日期"), pattern("环境/日期")]);
        assert_eq!(overlaps.len(), 1);
        assert_eq!(overlaps[0].reason, OverlapReason::IdenticalLiteral);
    }

    #[test]
    fn an_unanalyzable_escape_hatch_does_not_block_a_valid_declaration() {
        assert!(
            check_pattern_overlaps(&[pattern("/(日期|时间)/"), pattern("环境/*")]).is_empty(),
            "a free-form pattern cannot be compared structurally and must not be guessed at"
        );
    }

    #[test]
    fn a_broken_regex_reports_the_parser_reason() {
        let error = StateKeyPattern::parse("/日期(/").expect_err("an unclosed group must fail");
        assert!(
            error.contains("unclosed") || error.contains("group"),
            "the parser reason must survive: {error}"
        );
    }
}
