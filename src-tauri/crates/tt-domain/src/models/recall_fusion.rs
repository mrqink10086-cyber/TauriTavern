//! Turning several retrievals into one ranked list.
//!
//! The pieces are deliberately small and pure: candidates arrive as ranks, get
//! their obvious losers dropped, are fused across retrievers, discounted by how
//! far back they were published, and finally thinned so near-duplicates cannot
//! take every slot.
//!
//! Two decisions run through all of it. Ranking is done on ranks, never on the
//! retrievers' own scores — a cosine and a token-overlap share are different
//! quantities, while "first in this retriever" is the same fact in both. And
//! absolute similarity is never the filter: a relevance floor is only there to
//! cut noise, because measured semantic and non-semantic scores overlap far too
//! much for a threshold to decide what is relevant.

use super::recall::{RecallCandidate, RecallRetriever};

/// The rank constant of reciprocal rank fusion. Large enough that the top few
/// places differ clearly, small enough that deep ranks still contribute.
pub const RRF_K: usize = 60;
/// Dense retrieval carries the signal; the lexical pass is here to catch the
/// names, ids, and numbers that embeddings systematically miss.
pub const DENSE_WEIGHT: f32 = 1.0;
pub const LEXICAL_WEIGHT: f32 = 0.8;
/// How many places each retriever keeps unconditionally. The Pareto front alone
/// can be very small, and losing a retriever's own top hits to a tie-break is
/// not a trade-off anyone asked for.
pub const SKYLINE_GUARANTEED_PLACES: usize = 3;
/// The floor of the distance decay: the oldest fact still scores 0.8.
pub const FLOOR_DECAY_BASE: f32 = 0.8;
/// How many floors the decay needs to fall half of the way to its floor.
pub const FLOOR_DECAY_SCALE: f32 = 100.0;
/// How much diversity outweighs relevance once candidates start crowding.
pub const MMR_LAMBDA: f32 = 0.7;

/// A candidate with the score it is ranked by.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScoredCandidate {
    pub id: i64,
    pub score: f32,
}

/// The weight fusion gives a retriever's rank.
pub const fn retriever_weight(retriever: RecallRetriever) -> f32 {
    match retriever {
        RecallRetriever::Dense => DENSE_WEIGHT,
        RecallRetriever::Lexical => LEXICAL_WEIGHT,
    }
}

/// Keep every candidate that no other candidate beats on both retrievers.
///
/// A candidate is dominated when another one is at least as good in every
/// retriever and strictly better in one. What is left is the trade-off curve —
/// the records that are the best dense hit, the best lexical hit, or a genuine
/// compromise between them. Each retriever's own top places are kept as well,
/// so a small front cannot drop a retriever's best answer.
///
/// Input is expected to hold one entry per record, with the record's best rank
/// per retriever already merged.
pub fn skyline_union(candidates: &[RecallCandidate]) -> Vec<RecallCandidate> {
    candidates
        .iter()
        .filter(|candidate| {
            let dominated = candidates.iter().any(|other| {
                other.id != candidate.id && dominates(other, candidate)
            });
            !dominated || is_guaranteed(candidate)
        })
        .cloned()
        .collect()
}

/// Reciprocal rank fusion: each retriever contributes `weight / (K + rank)`.
pub fn rrf_scores(candidates: &[RecallCandidate]) -> Vec<ScoredCandidate> {
    let mut scored = candidates
        .iter()
        .map(|candidate| ScoredCandidate {
            id: candidate.id,
            score: candidate
                .ranks
                .iter()
                .map(|entry| retriever_weight(entry.retriever) / (RRF_K + entry.rank) as f32)
                .sum(),
        })
        .collect::<Vec<_>>();
    sort_by_score(&mut scored);
    scored
}

/// How much a record's age costs it, between [`FLOOR_DECAY_BASE`] and 1.
///
/// Distance is measured in floors rather than wall-clock time: a story's
/// "recently" is the floors the reader just passed, not the hours between
/// sessions. The decay is logarithmic, so an old fact stays reachable instead of
/// dropping off a cliff. A record with no floor yet was published by the run
/// that is happening now, so it counts as current.
pub fn floor_decay(floor: Option<i64>, current_floor: i64) -> f32 {
    let distance = floor.map_or(0.0, |floor| (current_floor - floor).max(0) as f32);
    FLOOR_DECAY_BASE
        + (1.0 - FLOOR_DECAY_BASE) / (1.0 + (1.0 + distance / FLOOR_DECAY_SCALE).ln())
}

/// The number ranking and diversity selection work with.
pub fn final_score(rrf: f32, floor: Option<i64>, current_floor: i64, importance: f32) -> f32 {
    rrf * floor_decay(floor, current_floor) * (1.0 + 0.01 * importance)
}

/// Pick candidates that are relevant *and* unlike what is already picked.
///
/// Crowding is what breaks retrieval at scale, not scan time: once an index
/// holds a hundred near-identical facts, they take every slot and the result
/// stops being informative. `λ × score − (1−λ) × largest similarity to the
/// selection` keeps the best candidate and then prefers records that add
/// something. `similarity` is the stored vectors' cosine, supplied by the caller
/// because the domain never holds embeddings; an empty selection has no penalty.
///
/// Anything below `noise_floor` is dropped first — a floor is there to cut
/// noise, never to decide relevance.
pub fn mmr_select(
    candidates: &[ScoredCandidate],
    limit: usize,
    noise_floor: f32,
    similarity: impl Fn(i64, i64) -> f32,
) -> Vec<ScoredCandidate> {
    let mut remaining = candidates
        .iter()
        .copied()
        .filter(|candidate| candidate.score >= noise_floor)
        .collect::<Vec<_>>();
    sort_by_score(&mut remaining);

    let mut selected: Vec<ScoredCandidate> = Vec::new();
    while selected.len() < limit && !remaining.is_empty() {
        let mut best = 0;
        let mut best_value = f32::NEG_INFINITY;
        for (index, candidate) in remaining.iter().enumerate() {
            let penalty = selected
                .iter()
                .map(|chosen| similarity(candidate.id, chosen.id))
                .fold(0.0_f32, f32::max);
            let value = MMR_LAMBDA * candidate.score - (1.0 - MMR_LAMBDA) * penalty;
            // Strict comparison keeps the earlier candidate on a tie, so the
            // same input always produces the same selection.
            if value > best_value {
                best_value = value;
                best = index;
            }
        }
        selected.push(remaining.remove(best));
    }
    selected
}

/// Best first, ties broken by id so the order never depends on hash iteration.
fn sort_by_score(candidates: &mut [ScoredCandidate]) {
    candidates.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.id.cmp(&right.id))
    });
}

fn dominates(left: &RecallCandidate, right: &RecallCandidate) -> bool {
    let mut strictly_better = false;
    for retriever in RecallRetriever::ALL {
        let (left_rank, right_rank) = (
            missing_as_last(left, retriever),
            missing_as_last(right, retriever),
        );
        if left_rank > right_rank {
            return false;
        }
        if left_rank < right_rank {
            strictly_better = true;
        }
    }
    strictly_better
}

/// A retriever that did not return a record ranks it last, which is what makes
/// "returned by both, high in one" beat "returned by one, low".
fn missing_as_last(candidate: &RecallCandidate, retriever: RecallRetriever) -> usize {
    candidate.rank(retriever).unwrap_or(usize::MAX)
}

fn is_guaranteed(candidate: &RecallCandidate) -> bool {
    RecallRetriever::ALL.iter().any(|retriever| {
        candidate
            .rank(*retriever)
            .is_some_and(|rank| rank < SKYLINE_GUARANTEED_PLACES)
    })
}

#[cfg(test)]
mod tests {
    use super::{
        RRF_K, ScoredCandidate, final_score, floor_decay, mmr_select, rrf_scores, skyline_union,
    };
    use crate::models::recall::{
        RecallCandidate, RecallChannelId, RecallRetriever, RetrievalRank,
    };

    fn candidate(id: i64, dense: Option<usize>, lexical: Option<usize>) -> RecallCandidate {
        let mut ranks = Vec::new();
        for (retriever, rank) in [
            (RecallRetriever::Dense, dense),
            (RecallRetriever::Lexical, lexical),
        ] {
            if let Some(rank) = rank {
                ranks.push(RetrievalRank { retriever, rank });
            }
        }
        RecallCandidate {
            id,
            channel: RecallChannelId::StateHistory,
            ranks,
            floor: None,
            importance: 0.0,
        }
    }

    fn ids(candidates: &[RecallCandidate]) -> Vec<i64> {
        candidates.iter().map(|candidate| candidate.id).collect()
    }

    #[test]
    fn a_candidate_better_in_both_retrievers_removes_the_other() {
        let kept = skyline_union(&[candidate(1, Some(0), Some(0)), candidate(2, Some(9), Some(9))]);

        assert_eq!(
            ids(&kept),
            vec![1],
            "a record worse in every retriever has nothing to trade off"
        );
    }

    #[test]
    fn only_the_trade_off_curve_survives() {
        let kept = skyline_union(&[
            // The best dense hit, the best lexical hit, and a compromise.
            candidate(1, Some(0), Some(9)),
            candidate(2, Some(9), Some(0)),
            candidate(3, Some(5), Some(5)),
            // Worse than the compromise in both retrievers.
            candidate(4, Some(6), Some(6)),
        ]);

        assert_eq!(
            ids(&kept),
            vec![1, 2, 3],
            "nobody's best can still be nobody's worse; a record worse everywhere is not a trade-off"
        );
    }

    #[test]
    fn a_retrievers_top_places_are_kept_even_when_dominated() {
        let kept = skyline_union(&[
            // Dominates 2 on both retrievers, but 2 is the lexical runner-up.
            candidate(1, Some(0), Some(0)),
            candidate(2, Some(30), Some(1)),
        ]);

        assert_eq!(
            ids(&kept),
            vec![1, 2],
            "a retriever's own top hits must not be lost to the front being small"
        );
    }

    #[test]
    fn a_record_only_one_retriever_returned_is_ranked_last_by_the_other() {
        let kept = skyline_union(&[candidate(1, Some(0), Some(20)), candidate(2, Some(5), None)]);

        assert_eq!(
            ids(&kept),
            vec![1],
            "absent from the lexical retriever is worse than a poor lexical rank"
        );
    }

    #[test]
    fn fusion_prefers_a_record_both_retrievers_ranked_well() {
        let scores = rrf_scores(&[
            candidate(1, Some(0), Some(0)),
            candidate(2, Some(0), None),
            candidate(3, None, Some(50)),
        ]);

        assert_eq!(scores[0].id, 1);
        assert!(
            scores[0].score > scores[1].score && scores[1].score > scores[2].score,
            "a weak lexical place must still be worth something: {scores:?}"
        );
        assert!(scores[0].score > 1.0 / RRF_K as f32, "both retrievers contributed");
    }

    #[test]
    fn fusion_scores_are_ordered_best_first_and_deterministic() {
        let candidates = [candidate(2, Some(3), None), candidate(1, Some(5), Some(9))];

        let first = rrf_scores(&candidates);
        assert_eq!(first, rrf_scores(&candidates), "the same input must not reshuffle");
        assert!(
            first[0].score >= first[1].score,
            "the result is handed to MMR already ordered: {first:?}"
        );
    }

    #[test]
    fn the_newest_floor_costs_nothing_and_the_decay_stays_above_its_floor() {
        assert_eq!(floor_decay(Some(40), 40), 1.0);
        assert_eq!(
            floor_decay(None, 40),
            1.0,
            "a record whose floor is not bound yet was published by this run"
        );
        assert!(floor_decay(Some(39), 40) < 1.0);
        assert!(
            floor_decay(Some(-1_000_000), 40) > crate::models::recall_fusion::FLOOR_DECAY_BASE,
            "the decay approaches its floor without reaching it"
        );
        assert_eq!(
            floor_decay(Some(100_000), 40),
            1.0,
            "a future floor must not reward a record by accident"
        );
    }

    #[test]
    fn an_often_recalled_record_gets_a_small_nudge_not_a_second_life() {
        let plain = final_score(0.5, Some(10), 20, 0.0);
        let recalled = final_score(0.5, Some(10), 20, 10.0);

        assert!(recalled > plain);
        assert!(
            recalled < plain * 2.0,
            "hit feedback nudges the ranking; it does not override it"
        );
    }

    #[test]
    fn a_near_duplicate_does_not_take_the_slot_a_diverse_record_needs() {
        let candidates = [
            ScoredCandidate { id: 1, score: 0.9 },
            ScoredCandidate { id: 2, score: 0.88 },
            ScoredCandidate { id: 3, score: 0.5 },
        ];
        // 2 restates 1; 3 is unrelated but weaker.
        let similarity = |left: i64, right: i64| {
            if left == 1 && right == 2 || left == 2 && right == 1 {
                0.99
            } else {
                0.0
            }
        };

        let selected = mmr_select(&candidates, 2, 0.0, similarity);

        assert_eq!(
            selected.iter().map(|candidate| candidate.id).collect::<Vec<_>>(),
            vec![1, 3],
            "the second slot goes to the record that adds something"
        );
    }

    #[test]
    fn selection_stops_at_the_noise_floor_and_the_limit() {
        let candidates = [
            ScoredCandidate { id: 1, score: 0.9 },
            ScoredCandidate { id: 2, score: 0.05 },
        ];

        assert_eq!(
            mmr_select(&candidates, 5, 0.1, |_, _| 0.0)
                .iter()
                .map(|candidate| candidate.id)
                .collect::<Vec<_>>(),
            vec![1],
            "the noise floor cuts the tail before the limit is reached"
        );
        assert_eq!(mmr_select(&candidates, 0, 0.0, |_, _| 0.0).len(), 0);
    }

    #[test]
    fn selection_is_deterministic_when_scores_tie() {
        let candidates = [
            ScoredCandidate { id: 2, score: 0.5 },
            ScoredCandidate { id: 1, score: 0.5 },
        ];

        assert_eq!(
            mmr_select(&candidates, 2, 0.0, |_, _| 0.0)
                .iter()
                .map(|candidate| candidate.id)
                .collect::<Vec<_>>(),
            vec![1, 2],
            "ties are broken by id, not by the order the retrievers happened to return"
        );
    }
}
