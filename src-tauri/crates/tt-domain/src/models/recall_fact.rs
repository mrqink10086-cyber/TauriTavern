//! Turning a published state document into indexable facts.
//!
//! The embedded object is not the bare value — it is the rendered sentence
//! `label (key): values`. Both halves are load-bearing: a short value on its own
//! ("72", "下午") carries almost no semantic content, and the key path is itself
//! a retrieval signal, so a search for `好感度` has to be able to reach a fact
//! whose value is only a number.
//!
//! Two identities come out of here, and they answer different questions:
//! the content hash decides whether a fact needs embedding again, and the
//! version index decides which published state version a record belongs to.

use sha2::{Digest, Sha256};

use super::state::{KeyResolution, StateDeclaration, StateDocument, StateKey};

/// The `kind` stamped on every record this module produces.
pub const FACT_KIND: &str = "fact";

/// One field of one published state version, ready to index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecallFact {
    /// The canonical key, so a record can be matched back to its field.
    pub field_key: String,
    pub sentence: String,
    /// [`fact_content_hash`] of this sentence.
    pub hash: i64,
}

/// Render every non-empty field of a document into a fact.
///
/// A field whose value was cleared produces nothing: there is no fact to
/// retrieve, and an empty sentence would only dilute the index. Ordering is the
/// key order so the same document always yields the same batch, whatever order
/// the fields were written in.
pub fn render_fact_sentences(
    document: &StateDocument,
    declaration: &StateDeclaration,
    model_profile: &str,
) -> Vec<RecallFact> {
    let mut fields = document.fields.iter().collect::<Vec<_>>();
    fields.sort_by(|left, right| left.key.cmp(&right.key));

    fields
        .into_iter()
        .filter(|field| !field.values.is_empty())
        .map(|field| {
            let field_key = field.key.as_str().to_string();
            let sentence = render_fact_sentence(
                &field_label(declaration, &field.key),
                &field.key,
                &field.values,
            );
            let hash = fact_content_hash(model_profile, &field_key, &sentence);
            RecallFact {
                field_key,
                sentence,
                hash,
            }
        })
        .collect()
}

/// The sentence a fact is embedded from.
pub fn render_fact_sentence(label: &str, key: &StateKey, values: &[String]) -> String {
    let joined = values.join("; ");
    // A label that only repeats the key would render as `日期 (日期)`, which is
    // noise in a text that is scored by token overlap.
    if label.is_empty() || label == key.as_str() {
        format!("{key}: {joined}")
    } else {
        format!("{label} ({key}): {joined}")
    }
}

/// Identity of a fact's content, used to skip embedding what has not changed.
///
/// The embedding profile is part of the identity: the same sentence under
/// another model is a different vector, and a hash that ignored that would keep
/// the stale one.
pub fn fact_content_hash(model_profile: &str, field_key: &str, sentence: &str) -> i64 {
    let mut digest = Sha256::new();
    digest.update(model_profile.as_bytes());
    digest.update([0]);
    digest.update(field_key.as_bytes());
    digest.update([0]);
    digest.update(sentence.as_bytes());
    digest_to_i64(digest.finalize())
}

/// The value stored as `VectorMetadata::index` for one published state version.
///
/// Floor binding works backwards from a version id: the frontend knows the
/// `state_id` a message carries and the floor it landed on, and nothing else in
/// the record identifies the version. Hashing keeps the record key stable when
/// the same version is bound again.
pub fn state_version_index(state_id: &str) -> i64 {
    digest_to_i64(Sha256::digest(state_id.as_bytes()))
}

/// The display name of a field, falling back to its last path segment.
///
/// The declaration owns the real labels; a chat without one — or a key it does
/// not name — still gets something readable, and the full key is in the sentence
/// either way.
fn field_label(declaration: &StateDeclaration, key: &StateKey) -> String {
    match declaration.resolve(key.as_str()) {
        KeyResolution::Matched { label, .. } if !label.trim().is_empty() => label,
        _ => key
            .as_str()
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_string(),
    }
}

fn digest_to_i64(digest: impl AsRef<[u8]>) -> i64 {
    let digest = digest.as_ref();
    let mut head = [0_u8; 8];
    head.copy_from_slice(&digest[..8]);
    i64::from_le_bytes(head)
}

#[cfg(test)]
mod tests {
    use super::{fact_content_hash, render_fact_sentences, state_version_index};
    use crate::models::state::{DeclaredStateField, StateDeclaration, StateDocument, StateField};
    use crate::models::state_access::StateFieldAccess;
    use crate::models::state_key::{StateKey, StateKeyPattern};

    fn key(raw: &str) -> StateKey {
        StateKey::parse(raw).expect("test key must be valid")
    }

    fn field(raw: &str, values: &[&str]) -> StateField {
        StateField {
            key: key(raw),
            values: values.iter().map(|value| value.to_string()).collect(),
        }
    }

    fn document(fields: Vec<StateField>) -> StateDocument {
        StateDocument { fields }
    }

    fn declaration(entries: &[(&str, &str)]) -> StateDeclaration {
        StateDeclaration {
            fields: entries
                .iter()
                .map(|(pattern, label)| DeclaredStateField {
                    pattern: StateKeyPattern::parse(pattern)
                        .unwrap_or_else(|error| panic!("`{pattern}` must parse: {error}")),
                    label: label.to_string(),
                    access: StateFieldAccess::DECLARED,
                    initial: Vec::new(),
                })
                .collect(),
            panels: Default::default(),
            machine: None,
            predicates: None,
            limits: Default::default(),
        }
    }

    const PROFILE: &str = "Qwen/Qwen3-Embedding-0.6B|f32-v1";

    #[test]
    fn a_field_renders_as_label_key_and_values() {
        let facts = render_fact_sentences(
            &document(vec![field("角色/林/好感度", &["72"])]),
            &declaration(&[("角色/*/好感度", "好感度")]),
            PROFILE,
        );

        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].field_key, "角色/林/好感度");
        assert_eq!(facts[0].sentence, "好感度 (角色/林/好感度): 72");
        assert_eq!(
            facts[0].hash,
            fact_content_hash(PROFILE, "角色/林/好感度", "好感度 (角色/林/好感度): 72")
        );
    }

    #[test]
    fn several_values_render_as_one_sentence() {
        let facts = render_fact_sentences(
            &document(vec![field("角色/林/着装", &["风衣", "围巾"])]),
            &declaration(&[("角色/*/着装", "着装")]),
            PROFILE,
        );

        assert_eq!(facts[0].sentence, "着装 (角色/林/着装): 风衣; 围巾");
    }

    #[test]
    fn a_cleared_field_produces_no_fact() {
        let facts = render_fact_sentences(
            &document(vec![field("环境/地点", &[]), field("环境/时间", &["夜晚"])]),
            &declaration(&[("环境/*", "环境")]),
            PROFILE,
        );

        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].field_key, "环境/时间");
    }

    #[test]
    fn a_chinese_key_is_kept_whole_in_the_sentence() {
        let facts = render_fact_sentences(
            &document(vec![field("环境/日期", &["2026/09/10"])]),
            &StateDeclaration::default(),
            PROFILE,
        );

        assert_eq!(facts[0].sentence, "日期 (环境/日期): 2026/09/10");
        assert_eq!(facts[0].field_key, "环境/日期");
    }

    #[test]
    fn a_single_segment_key_does_not_repeat_its_label() {
        let facts = render_fact_sentences(
            &document(vec![field("日期", &["2026/09/10"])]),
            &StateDeclaration::default(),
            PROFILE,
        );

        assert_eq!(facts[0].sentence, "日期: 2026/09/10");
    }

    #[test]
    fn facts_are_ordered_by_key_not_by_write_order() {
        let facts = render_fact_sentences(
            &document(vec![
                field("环境/时间", &["夜晚"]),
                field("环境/地点", &["咖啡馆"]),
            ]),
            &StateDeclaration::default(),
            PROFILE,
        );

        assert_eq!(
            facts.iter().map(|fact| fact.field_key.as_str()).collect::<Vec<_>>(),
            ["环境/地点", "环境/时间"]
        );
    }

    #[test]
    fn a_content_hash_is_stable_and_sensitive_to_every_input() {
        let base = fact_content_hash(PROFILE, "环境/时间", "时间 (环境/时间): 夜晚");

        assert_eq!(
            base,
            fact_content_hash(PROFILE, "环境/时间", "时间 (环境/时间): 夜晚"),
            "the same content must skip re-embedding"
        );
        assert_ne!(base, fact_content_hash("another|profile", "环境/时间", "时间 (环境/时间): 夜晚"));
        assert_ne!(base, fact_content_hash(PROFILE, "环境/地点", "时间 (环境/时间): 夜晚"));
        assert_ne!(base, fact_content_hash(PROFILE, "环境/时间", "时间 (环境/时间): 早晨"));
    }

    #[test]
    fn a_fact_hash_matches_the_standalone_hash() {
        let facts = render_fact_sentences(
            &document(vec![field("环境/时间", &["夜晚"])]),
            &StateDeclaration::default(),
            PROFILE,
        );

        assert_eq!(
            facts[0].hash,
            fact_content_hash(PROFILE, "环境/时间", &facts[0].sentence),
            "the batch must use the same identity the incremental diff compares against"
        );
    }

    #[test]
    fn a_version_index_is_deterministic_and_version_specific() {
        let index = state_version_index("3f2a");

        assert_eq!(index, state_version_index("3f2a"));
        assert_ne!(index, state_version_index("3f2b"));
    }
}
