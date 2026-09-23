//! The tokenizer families that ship with the app.
//!
//! `miktik`'s registry cannot carry these. It accepts a registered resource
//! only for a name it already knows — anything else is resolved to
//! `gpt-3.5-turbo` and the registration is refused — so a vocabulary added to
//! the app would either fail to register or, worse, count in a vocabulary the
//! user never chose. Every family here therefore loads straight from its
//! bundled file with `kitoken`, which is the same parser `miktik` uses for
//! Hugging Face definitions; the name never reaches the registry at all.
//!
//! Each file is the model's own `tokenizer.json`, gzipped, taken from the
//! model repository. `Claude`, `DeepSeek`, `Gemma` and the SillyTavern names
//! are still served by `miktik` and stay where they are: this table answers
//! only for the families it names.

use std::io::Read;

use kitoken::Kitoken;

/// One vocabulary that travels inside the binary.
pub struct BuiltinVocabulary {
    /// The name settings and the budget speak. Also what `resolve` matches on.
    pub canonical: &'static str,
    /// What a settings row shows.
    pub label: &'static str,
    /// The gzipped `tokenizer.json`.
    pub resource: &'static [u8],
    /// Other spellings that mean this family. Matched longest first, so a
    /// family named `qwen3.8` cannot swallow `qwen3-embedding`.
    pub aliases: &'static [&'static str],
}

/// Every vocabulary compiled into the app.
///
/// Ordered newest first: a settings list reads this in order, and the first
/// entry is what an unconfigured budget would pick.
pub const VOCABULARIES: &[BuiltinVocabulary] = &[
    BuiltinVocabulary {
        canonical: "qwen3.8",
        label: "Qwen3.8",
        resource: include_bytes!("../../../resources/tokenizers/qwen3.8.json.gz"),
        aliases: &["qwen3.8", "qwen3_8", "qwen3-8"],
    },
    BuiltinVocabulary {
        canonical: "deepseek-v4.1",
        label: "DeepSeek V4.1",
        resource: include_bytes!("../../../resources/tokenizers/deepseek-v4.1.json.gz"),
        aliases: &["deepseek-v4.1", "deepseek-v4", "deepseek_v4"],
    },
    BuiltinVocabulary {
        canonical: "glm",
        label: "GLM",
        resource: include_bytes!("../../../resources/tokenizers/glm.json.gz"),
        aliases: &["glm", "chatglm"],
    },
    BuiltinVocabulary {
        canonical: "gemma4",
        label: "Gemma 4 / Gemini",
        resource: include_bytes!("../../../resources/tokenizers/gemma4.json.gz"),
        aliases: &["gemma4", "gemma-4", "gemini"],
    },
    BuiltinVocabulary {
        canonical: "qwen3-embedding",
        label: "Qwen3 Embedding",
        resource: include_bytes!("../../../resources/tokenizers/qwen3-embedding.json.gz"),
        aliases: &["qwen3-embedding", "qwen3_embedding"],
    },
];

/// The prefix a vocabulary of the user's own is named with: `file:<path>`.
///
/// This is what keeps the table above from having to grow for every new model:
/// a vocabulary is a file, and the parser either reads it or says why not. It is
/// a name rather than a separate setting so that "which vocabulary counts" is
/// one string, wherever it is asked.
pub const USER_FILE_PREFIX: &str = "file:";

/// The path inside a user-supplied name, when the name is one.
pub fn user_file(model: &str) -> Option<&str> {
    let path = model.trim().strip_prefix(USER_FILE_PREFIX)?.trim();
    (!path.is_empty()).then_some(path)
}

/// Read a vocabulary the user supplied.
///
/// Two shapes are accepted because two are what a model repository hands out: a
/// Hugging Face `tokenizer.json`, and the SentencePiece `.model` that older
/// releases ship instead. Which one it is, is visible in the first byte, so the
/// file does not have to be named in any particular way.
pub fn load_user_file(path: &str) -> Result<Kitoken, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("`{path}` cannot be read: {error}"))?;
    let looks_like_json = bytes
        .iter()
        .find(|byte| !byte.is_ascii_whitespace())
        .is_some_and(|byte| *byte == b'{');

    if looks_like_json {
        Kitoken::from_tokenizers_slice(&bytes)
            .map_err(|error| format!("`{path}` is not a readable Hugging Face tokenizer: {error}"))
    } else {
        Kitoken::from_sentencepiece_slice(&bytes)
            .map_err(|error| format!("`{path}` is not a readable SentencePiece model: {error}"))
    }
}

/// The characters that may follow an alias for it to still be that alias.
///
/// A prefix match has to stop at a word boundary: `qwen3-embedding` starts with
/// `qwen3` and is a different vocabulary, so `qwen3` alone may not claim it.
const CONTINUATIONS: [char; 5] = ['-', '.', '_', ':', ' '];

/// The family a model name means, if one of ours.
///
/// A caller usually hands over the model the chat is configured with, and those
/// arrive the way a model repository names them: `Qwen/Qwen3.8-27B`,
/// `zai-org/GLM-4.6`, `google/gemma-4-31B-it`. The owner segment is therefore
/// dropped before matching — it says whose release it is, not which vocabulary
/// it uses.
///
/// Empty names mean nothing here: the caller that got one has its own default,
/// and answering for it would hide that.
pub fn resolve(model: &str) -> Option<&'static BuiltinVocabulary> {
    let trimmed = model.trim();
    if trimmed.is_empty() {
        return None;
    }

    let name = trimmed
        .rsplit('/')
        .next()
        .unwrap_or(trimmed)
        .trim()
        .to_ascii_lowercase();
    if name.is_empty() {
        return None;
    }

    let mut best: Option<(&BuiltinVocabulary, usize)> = None;
    for vocabulary in VOCABULARIES {
        for alias in vocabulary.aliases {
            let matched = name == *alias
                || (name.starts_with(alias)
                    && name[alias.len()..]
                        .chars()
                        .next()
                        .is_some_and(|next| CONTINUATIONS.contains(&next)));
            if matched && best.is_none_or(|(_, length)| alias.len() > length) {
                best = Some((vocabulary, alias.len()));
            }
        }
    }

    best.map(|(vocabulary, _)| vocabulary)
}

/// Load a family's definition from the resource compiled into the binary.
///
/// Decompression and parsing happen here rather than in the async caller: this
/// is CPU work on a few megabytes, so the caller moves it to a blocking task
/// instead of holding the runtime while it runs.
pub fn load(vocabulary: &BuiltinVocabulary) -> Result<Kitoken, String> {
    let mut json = Vec::new();
    flate2::read::GzDecoder::new(vocabulary.resource)
        .read_to_end(&mut json)
        .map_err(|error| format!("`{}` cannot be decompressed: {error}", vocabulary.canonical))?;

    Kitoken::from_tokenizers_slice(&json)
        .map_err(|error| format!("`{}` cannot be parsed: {error}", vocabulary.canonical))
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::{VOCABULARIES, load, resolve};

    #[test]
    fn a_family_answers_for_its_own_names() {
        assert_eq!(resolve("Qwen3.8-27B").map(|v| v.canonical), Some("qwen3.8"));
        assert_eq!(resolve("  glm-4.6  ").map(|v| v.canonical), Some("glm"));
        assert_eq!(
            resolve("DeepSeek-V4.1-Flash").map(|v| v.canonical),
            Some("deepseek-v4.1")
        );
        assert_eq!(resolve("gemini-2.5-pro").map(|v| v.canonical), Some("gemma4"));
    }

    #[test]
    fn a_repository_style_id_matches_on_its_own_segment() {
        // What a chat actually stores when the model is picked from a list.
        assert_eq!(
            resolve("Qwen/Qwen3.8-27B").map(|v| v.canonical),
            Some("qwen3.8")
        );
        assert_eq!(resolve("zai-org/GLM-4.6").map(|v| v.canonical), Some("glm"));
        assert_eq!(
            resolve("deepseek-ai/DeepSeek-V4.1-Flash").map(|v| v.canonical),
            Some("deepseek-v4.1")
        );
        assert_eq!(
            resolve("google/gemma-4-31B-it").map(|v| v.canonical),
            Some("gemma4")
        );
    }

    #[test]
    fn a_longer_alias_wins_over_a_prefix_of_it() {
        // `qwen3-embedding` begins with a name that is also a family.
        assert_eq!(
            resolve("qwen3-embedding-8b").map(|v| v.canonical),
            Some("qwen3-embedding")
        );
    }

    #[test]
    fn a_user_supplied_vocabulary_is_read_from_its_path() {
        let directory = std::env::temp_dir().join(format!(
            "tt-user-vocabulary-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).expect("temp directory");

        // The real thing, taken apart the way the loader takes it apart.
        let vocabulary = VOCABULARIES
            .iter()
            .find(|vocabulary| vocabulary.canonical == "glm")
            .expect("glm ships with the app");
        let mut json = Vec::new();
        flate2::read::GzDecoder::new(vocabulary.resource)
            .read_to_end(&mut json)
            .expect("the bundled resource decompresses");
        let path = directory.join("mine.json");
        std::fs::write(&path, &json).expect("write the vocabulary");
        let named = format!("file:{}", path.display());

        assert_eq!(super::user_file(&named), path.to_str());
        assert_eq!(super::user_file("  file:  /tmp/x.json  "), Some("/tmp/x.json"));
        assert!(super::user_file("glm").is_none());

        let tokenizer = super::load_user_file(path.to_str().expect("path"))
            .expect("a Hugging Face tokenizer.json loads");
        assert!(
            !tokenizer
                .encode("她站在窗边", true)
                .expect("encode")
                .is_empty()
        );

        // A file that is neither shape reports why instead of guessing.
        let broken = directory.join("broken.json");
        std::fs::write(&broken, b"not a tokenizer").expect("write the broken file");
        let error = super::load_user_file(broken.to_str().expect("path"))
            .expect_err("an unreadable file is an error, not a fallback");
        assert!(error.contains("broken.json"), "{error}");

        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn a_name_we_do_not_ship_is_left_to_the_caller() {
        // The registry answers for these, and answering here would shadow it.
        assert!(resolve("claude").is_none());
        assert!(resolve("gpt-4o").is_none());
        assert!(resolve("").is_none());
        // A prefix has to end at a word boundary, not mid-word.
        assert!(resolve("glmzzz").is_none());
    }

    #[test]
    fn every_shipped_family_loads_and_counts() {
        for vocabulary in VOCABULARIES {
            let tokenizer = load(vocabulary)
                .unwrap_or_else(|error| panic!("`{}` must load: {error}", vocabulary.canonical));
            let ids = tokenizer
                .encode("她站在窗边，说：「今天不走了。」", true)
                .unwrap_or_else(|error| panic!("`{}` must encode: {error}", vocabulary.canonical));
            assert!(
                !ids.is_empty(),
                "`{}` returned no tokens",
                vocabulary.canonical
            );
        }
    }

    #[test]
    fn a_name_matches_one_family_only() {
        // Named models must not be claimed twice; the table is small enough to
        // check exhaustively rather than trusting the ordering rules.
        for vocabulary in VOCABULARIES {
            for alias in vocabulary.aliases {
                let winners: Vec<&str> = VOCABULARIES
                    .iter()
                    .filter(|other| {
                        other.aliases.iter().any(|candidate| {
                            candidate == alias
                                || candidate.starts_with(alias)
                                || alias.starts_with(candidate)
                        })
                    })
                    .map(|other| other.canonical)
                    .collect();
                assert_eq!(
                    winners,
                    [vocabulary.canonical],
                    "alias `{alias}` touches more than one family"
                );
            }
        }
    }
}
