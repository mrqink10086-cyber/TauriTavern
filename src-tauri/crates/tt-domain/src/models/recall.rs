//! The channels a first-party recall query may read from.
//!
//! A channel is one vector collection plus the policy that governs it: what gets
//! indexed into it, how many candidates it contributes, and where the surviving
//! text lands. Naming them in the domain is what keeps the collection name, the
//! Profile spelling, and the panel label from drifting apart.

use serde::{Deserialize, Serialize};

/// One recall channel.
///
/// The wire spelling is the kebab-case id: a Profile stores it, the collection
/// name is derived from it, and the panel shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecallChannelId {
    /// Facts rendered from the state document each floor published.
    StateHistory,
    /// Chat message text: the corpus the vectors extension already indexes.
    ChatText,
    /// Narrative summary nodes produced by the extraction pipeline.
    Narrative,
}

impl RecallChannelId {
    pub const ALL: [Self; 3] = [Self::StateHistory, Self::ChatText, Self::Narrative];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StateHistory => "state-history",
            Self::ChatText => "chat-text",
            Self::Narrative => "narrative",
        }
    }

    pub fn from_id(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|channel| channel.as_str() == value)
    }
}

/// How a candidate was retrieved.
///
/// Fusion happens on these two, not on channels: the same collection is read
/// both ways, and the two readings are the dimensions a candidate can trade off
/// between. Channels decide where a surviving block lands and how much budget it
/// may spend, which is a later step than ranking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecallRetriever {
    /// Vector similarity, exact scan.
    Dense,
    /// Bigram token overlap over the same records.
    Lexical,
}

impl RecallRetriever {
    pub const ALL: [Self; 2] = [Self::Dense, Self::Lexical];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Dense => "dense",
            Self::Lexical => "lexical",
        }
    }
}

/// Where one retriever's own ranking placed a record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetrievalRank {
    pub retriever: RecallRetriever,
    /// Position in that retriever's result, best first (0 is its top hit).
    pub rank: usize,
}

/// One record as fusion sees it.
///
/// Fusion works on ranks, never on the retrievers' own scores: a cosine and a
/// token-overlap share are different quantities with different distributions,
/// while "first in this retriever" means the same thing in both.
#[derive(Debug, Clone, PartialEq)]
pub struct RecallCandidate {
    /// The record's content hash, which identifies it inside its scope.
    pub id: i64,
    /// The collection the record lives in, which is what its block will be
    /// configured by.
    pub channel: RecallChannelId,
    /// One entry per retriever that returned it. A record several channels
    /// returned keeps the best rank per retriever, and one entry per retriever.
    pub ranks: Vec<RetrievalRank>,
    /// The floor the record was published at, when its channel tracks floors.
    pub floor: Option<i64>,
    /// How often the record has been recalled before.
    pub importance: f32,
}

impl RecallCandidate {
    /// This retriever's rank for the record, if it returned it at all.
    pub fn rank(&self, retriever: RecallRetriever) -> Option<usize> {
        self.ranks
            .iter()
            .find(|entry| entry.retriever == retriever)
            .map(|entry| entry.rank)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        RecallCandidate, RecallChannelId, RecallRetriever, RetrievalRank,
    };

    #[test]
    fn channel_ids_round_trip_and_stay_distinct() {
        for (index, channel) in RecallChannelId::ALL.iter().enumerate() {
            assert_eq!(RecallChannelId::from_id(channel.as_str()), Some(*channel));
            assert!(
                !RecallChannelId::ALL[..index].contains(channel),
                "two channels must not share one id"
            );
        }
        assert!(RecallChannelId::from_id("unknown").is_none());
    }

    #[test]
    fn channel_ids_serialize_as_their_kebab_case_id() {
        assert_eq!(
            serde_json::to_string(&RecallChannelId::StateHistory).expect("serializes"),
            "\"state-history\""
        );
        assert_eq!(
            serde_json::from_str::<RecallChannelId>("\"chat-text\"").expect("deserializes"),
            RecallChannelId::ChatText
        );
    }

    #[test]
    fn retrievers_have_the_wire_ids_fusion_traces_are_written_with() {
        for (index, retriever) in RecallRetriever::ALL.iter().enumerate() {
            assert!(
                !RecallRetriever::ALL[..index]
                    .iter()
                    .any(|other| other.as_str() == retriever.as_str()),
                "two retrievers must not share one id"
            );
        }
        assert_eq!(RecallRetriever::Dense.as_str(), "dense");
        assert_eq!(RecallRetriever::Lexical.as_str(), "lexical");
    }

    #[test]
    fn a_candidate_reports_only_the_retrievers_that_returned_it() {
        let candidate = RecallCandidate {
            id: 7,
            channel: RecallChannelId::StateHistory,
            ranks: vec![RetrievalRank {
                retriever: RecallRetriever::Dense,
                rank: 2,
            }],
            floor: Some(3),
            importance: 0.0,
        };

        assert_eq!(candidate.rank(RecallRetriever::Dense), Some(2));
        assert_eq!(
            candidate.rank(RecallRetriever::Lexical),
            None,
            "a retriever that did not return the record has no rank, which fusion reads as absent"
        );
    }
}
