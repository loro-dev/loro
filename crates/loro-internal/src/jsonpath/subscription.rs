//! # JSONPath Subscription Module
//!
//! This module provides a mechanism to subscribe to changes that may affect a JSONPath query
//! result, without re-evaluating the query on every change.
//!
//! ## Design Goals
//!
//! - **Conservative matching**: The matcher may produce false positives (triggering when
//!   the query result hasn't actually changed) but must never produce false negatives
//!   (missing a notification when the result did change).
//! - **Lightweight notifications**: Callbacks receive no payload; applications can
//!   debounce/throttle and evaluate the JSONPath themselves.
//! - **Efficient matching**: Uses an NFA (non-deterministic finite automaton) approach
//!   to match event paths against compiled JSONPath queries in O(path_len × steps) time.
//!
//! ## Algorithm Overview
//!
//! 1. **Compilation**: JSONPath is parsed and converted into a sequence of lightweight
//!    `MatchSelector`s (no AST storage needed for matching).
//!
//! 2. **NFA Simulation**: When an event arrives, we simulate an NFA where:
//!    - States are positions in the step sequence (0..steps.len())
//!    - Transitions occur when a selector matches the current path element
//!    - Recursive steps allow staying at the same state while consuming input
//!    - Acceptance occurs when any state reaches or exceeds `steps.len()`

use std::sync::Arc;

use loro_common::TreeID;
use smallvec::SmallVec;

use crate::{
    event::{Diff, Index},
    jsonpath::{
        ast::{Query, Segment, Selector},
        JSONPathParser,
    },
    utils::subscription::Subscription,
    LoroDoc, LoroError, LoroResult,
};

/// Callback used by `subscribe_jsonpath`.
///
/// Note: the callback does **not** carry the query result. It is intended as a
/// lightweight notification so applications can debounce/throttle and evaluate
/// JSONPath themselves if needed.
pub type SubscribeJsonPathCallback = Arc<dyn Fn() + Send + Sync + 'static>;

/// Represents a single path segment used for matching against events.
///
/// Path elements are derived from event paths and represent the location of a change
/// in the document tree. They form the "input alphabet" for our NFA simulation.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PathElem {
    /// A map key (e.g., "title" in `$.books[0].title`)
    Key(Arc<str>),
    /// A sequence (list/array) index.
    /// - `Some(i)` means a specific index changed
    /// - `None` means "some index changed" (used for list/tree child mutations
    ///   where we don't know which specific indices are affected)
    Seq(Option<usize>),
    /// A tree node identifier (for Loro's tree CRDT)
    Node(TreeID),
}

impl From<&Index> for PathElem {
    fn from(value: &Index) -> Self {
        match value {
            Index::Key(k) => PathElem::Key(k.as_ref().into()),
            Index::Seq(i) => PathElem::Seq(Some(*i)),
            Index::Node(n) => PathElem::Node(*n),
        }
    }
}

// =============================================================================
// Lightweight Matching Types
// =============================================================================
//
// Instead of storing the full Selector AST (which includes heavyweight
// FilterExpression trees), we convert selectors to a simple matching-only enum.
// This reduces memory usage and code complexity.

/// Simplified selector for matching only - no AST storage needed.
///
/// All complex selectors (Slice, Wild, Filter, negative Index) become `Wildcard`
/// since they can match any element during conservative path matching.
#[derive(Debug, Clone)]
enum MatchSelector {
    /// Exact key match (from `Selector::Name`)
    Name(Arc<str>),
    /// Exact non-negative index match (from `Selector::Index` where index >= 0)
    Index(usize),
    /// Matches anything (from Wild, Slice, Filter, or negative Index)
    Wildcard,
}

/// A single step in the linearized JSONPath query.
#[derive(Debug, Clone)]
struct Step {
    /// If true, this step uses recursive descent (`..`) and can match at any depth.
    recursive: bool,
    /// Simplified selectors for matching.
    selectors: SmallVec<[MatchSelector; 2]>,
}

/// A compiled matcher that conservatively checks whether a change at some path
/// can affect the JSONPath query.
#[derive(Debug, Clone)]
struct JsonPathMatcher {
    /// Linearized sequence of matching steps
    steps: Vec<Step>,
}

impl JsonPathMatcher {
    /// Compile a JSONPath query into a matcher.
    fn new(query: &Query) -> Self {
        let mut steps = Vec::new();
        build_steps(&query.segments, &mut steps);
        JsonPathMatcher { steps }
    }

    /// Returns true if the provided path (from root) could affect the query result.
    fn may_match(&self, path: &[PathElem]) -> bool {
        if self.steps.is_empty() {
            return true;
        }
        let positions = self.positions_after(path);
        positions.iter().any(|&p| p >= self.steps.len())
    }

    /// Check if any position was reached by passing through a Wildcard step.
    /// Wildcard steps come from filter/wild/slice selectors - any key change
    /// under them could affect the query result.
    #[inline]
    fn passed_through_wildcard(&self, positions: &[usize]) -> bool {
        positions.iter().any(|&pos| {
            pos > 0
                && self.steps[pos - 1]
                    .selectors
                    .iter()
                    .any(|s| matches!(s, MatchSelector::Wildcard))
        })
    }

    /// Simulate the NFA on a path and return all reachable positions after consuming it.
    ///
    /// - Position `== steps.len()`: Query fully matched
    /// - Position `< steps.len()`: Partial match
    /// - Empty result: No match possible
    fn positions_after(&self, path: &[PathElem]) -> SmallVec<[usize; 8]> {
        let mut positions = SmallVec::<[usize; 8]>::new();
        positions.push(0);

        for elem in path {
            let mut next = SmallVec::<[usize; 8]>::new();

            for &pos in positions.iter() {
                if pos >= self.steps.len() {
                    next.push(pos);
                    continue;
                }

                let step = &self.steps[pos];

                // Recursive descent: can stay at same position
                if step.recursive {
                    next.push(pos);
                }

                // If selector matches, advance to next step
                if selector_matches(&step.selectors, elem) {
                    next.push(pos + 1);
                }
            }

            dedup_positions(&mut next);
            if next.is_empty() {
                return next;
            }
            positions = next;
        }

        positions
    }
}

/// Check if any selector matches the given path element.
fn selector_matches(selectors: &[MatchSelector], elem: &PathElem) -> bool {
    selectors.iter().any(|sel| match sel {
        MatchSelector::Name(name) => matches!(elem, PathElem::Key(k) if k == name),
        MatchSelector::Index(idx) => match elem {
            PathElem::Seq(Some(i)) => *i == *idx,
            PathElem::Seq(None) => true, // Unknown index - conservative match
            _ => false,
        },
        MatchSelector::Wildcard => true,
    })
}

/// Convert AST Selector to lightweight MatchSelector.
fn to_match_selector(sel: &Selector) -> MatchSelector {
    match sel {
        Selector::Name { name } => MatchSelector::Name(name.as_str().into()),
        Selector::Index { index } if *index >= 0 => MatchSelector::Index(*index as usize),
        // Negative index, Slice, Wild, Filter all become Wildcard
        _ => MatchSelector::Wildcard,
    }
}

/// Linearize the JSONPath AST into a sequence of matching steps.
fn build_steps(segment: &Segment, steps: &mut Vec<Step>) {
    match segment {
        Segment::Root {} => {}
        Segment::Child { left, selectors } => {
            build_steps(left, steps);
            steps.push(Step {
                recursive: false,
                selectors: selectors.iter().map(to_match_selector).collect(),
            });
        }
        Segment::Recursive { left, selectors } => {
            build_steps(left, steps);
            steps.push(Step {
                recursive: true,
                selectors: selectors.iter().map(to_match_selector).collect(),
            });
        }
    }
}

/// Deduplicate positions in-place.
#[inline]
fn dedup_positions(v: &mut SmallVec<[usize; 8]>) {
    v.sort_unstable();
    v.dedup();
}

// =============================================================================
// Public API
// =============================================================================

impl LoroDoc {
    /// Subscribe to updates that may affect the given JSONPath query.
    ///
    /// ## Behavior
    ///
    /// - **Conservative matching**: The callback may fire for changes that don't
    ///   actually affect the query result (false positives), but it will never
    ///   miss a change that does affect it (no false negatives).
    /// - **Lightweight notifications**: The callback receives no payload. Applications
    ///   should debounce/throttle and re-evaluate the JSONPath if needed.
    ///
    /// ## Returns
    ///
    /// A `Subscription` handle. Drop it to unsubscribe.
    #[cfg(feature = "jsonpath")]
    pub fn subscribe_jsonpath(
        &self,
        jsonpath: &str,
        callback: SubscribeJsonPathCallback,
    ) -> LoroResult<Subscription> {
        let query = JSONPathParser::new()
            .parse(jsonpath)
            .map_err(|e| LoroError::ArgErr(e.to_string().into_boxed_str()))?;

        let matcher = Arc::new(JsonPathMatcher::new(&query));

        let sub = self.subscribe_root(Arc::new(move |event| {
            if event.events.is_empty() {
                return;
            }

            let matcher = &matcher;
            let mut fired = false;

            for container_diff in event.events.iter() {
                if fired {
                    break;
                }

                // Convert the container's path to PathElem representation
                let base_path: SmallVec<[PathElem; 8]> = container_diff
                    .path
                    .iter()
                    .map(|(_, idx)| PathElem::from(idx))
                    .collect();

                // Check 1: Does the container path itself match the query?
                if matcher.may_match(&base_path) {
                    fired = true;
                    break;
                }

                // Check 2: Map-specific handling - check each changed key
                if let Diff::Map(map) = &container_diff.diff {
                    let base_positions = matcher.positions_after(&base_path);
                    if base_positions.is_empty() {
                        continue;
                    }

                    // If we passed through a Wildcard (filter/wild/slice), any key
                    // change could affect the result - be conservative.
                    let past_wildcard = matcher.passed_through_wildcard(&base_positions);

                    for key in map.updated.keys() {
                        let mut extended: SmallVec<[PathElem; 8]> = base_path.clone();
                        extended.push(PathElem::Key(key.as_ref().into()));

                        let extended_positions = matcher.positions_after(&extended);

                        // Trigger if key path could match OR we're inside a wildcard selection
                        if !extended_positions.is_empty() || past_wildcard {
                            fired = true;
                            break;
                        }
                    }

                    if fired {
                        break;
                    }
                }

                // Check 3: List/Tree/Counter child mutations
                // These container types can have child changes at unknown indices
                let has_child_changes = matches!(
                    &container_diff.diff,
                    Diff::List(_) | Diff::Tree(_) | Diff::Unknown
                );
                #[cfg(feature = "counter")]
                let has_child_changes =
                    has_child_changes || matches!(&container_diff.diff, Diff::Counter(_));

                if has_child_changes {
                    let mut extended: SmallVec<[PathElem; 8]> = base_path.clone();
                    extended.push(PathElem::Seq(None)); // "some child changed"
                    if !matcher.positions_after(&extended).is_empty() {
                        fired = true;
                    }
                }
            }

            if fired {
                (callback)();
            }
        }));

        Ok(sub)
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LoroDoc, MapHandler};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn make_book(
        doc: &LoroDoc,
        idx: usize,
        title: &str,
        available: bool,
        price: i64,
    ) -> MapHandler {
        let books = doc.get_list("books");
        let book = books
            .insert_container(idx, MapHandler::new_detached())
            .unwrap();
        book.insert("title", title).unwrap();
        book.insert("available", available).unwrap();
        book.insert("price", price).unwrap();
        book
    }

    #[test]
    fn jsonpath_subscribe_triggers_on_specific_key() {
        let doc = LoroDoc::new_auto_commit();
        let first_book = make_book(&doc, 0, "Old", true, 10);
        doc.commit_then_renew();

        let hit = Arc::new(AtomicUsize::new(0));
        let hit_ref = hit.clone();
        let _sub = doc
            .subscribe_jsonpath(
                "$.books[0].title",
                Arc::new(move || {
                    hit_ref.fetch_add(1, Ordering::SeqCst);
                }),
            )
            .unwrap();

        first_book.insert("title", "New").unwrap();
        doc.commit_then_renew();

        assert!(hit.load(Ordering::SeqCst) >= 1);
    }

    #[test]
    fn jsonpath_subscribe_wildcard_on_list() {
        let doc = LoroDoc::new_auto_commit();
        make_book(&doc, 0, "A", true, 10);
        let second_book = make_book(&doc, 1, "B", true, 20);
        doc.commit_then_renew();

        let hit = Arc::new(AtomicUsize::new(0));
        let hit_ref = hit.clone();
        let _sub = doc
            .subscribe_jsonpath(
                "$.books[*].price",
                Arc::new(move || {
                    hit_ref.fetch_add(1, Ordering::SeqCst);
                }),
            )
            .unwrap();

        second_book.insert("price", 25).unwrap();
        doc.commit_then_renew();

        assert!(hit.load(Ordering::SeqCst) >= 1);
    }

    #[test]
    fn jsonpath_subscribe_negative_index() {
        let doc = LoroDoc::new_auto_commit();
        make_book(&doc, 0, "A", true, 10);
        make_book(&doc, 1, "B", true, 20);
        doc.commit_then_renew();

        let hit = Arc::new(AtomicUsize::new(0));
        let hit_ref = hit.clone();
        let _sub = doc
            .subscribe_jsonpath(
                "$.books[-1].title",
                Arc::new(move || {
                    hit_ref.fetch_add(1, Ordering::SeqCst);
                }),
            )
            .unwrap();

        let books = doc.get_list("books");
        let last = books.get_child_handler(1).unwrap();
        last.as_map().unwrap().insert("title", "B updated").unwrap();
        doc.commit_then_renew();

        assert!(hit.load(Ordering::SeqCst) >= 1);
    }

    #[test]
    fn jsonpath_subscribe_slice_range() {
        let doc = LoroDoc::new_auto_commit();
        make_book(&doc, 0, "A", true, 10);
        let second_book = make_book(&doc, 1, "B", true, 20);
        make_book(&doc, 2, "C", true, 30);
        doc.commit_then_renew();

        let hit = Arc::new(AtomicUsize::new(0));
        let hit_ref = hit.clone();
        let _sub = doc
            .subscribe_jsonpath(
                "$.books[0:2].title",
                Arc::new(move || {
                    hit_ref.fetch_add(1, Ordering::SeqCst);
                }),
            )
            .unwrap();

        second_book.insert("title", "B updated").unwrap();
        doc.commit_then_renew();

        assert!(hit.load(Ordering::SeqCst) >= 1);
    }

    #[test]
    fn jsonpath_subscribe_recursive() {
        let doc = LoroDoc::new_auto_commit();
        let store = doc.get_map("store");
        let nested = store
            .insert_container("inventory", MapHandler::new_detached())
            .unwrap();
        nested.insert("total", 3).unwrap();
        doc.commit_then_renew();

        let hit = Arc::new(AtomicUsize::new(0));
        let hit_ref = hit.clone();
        let _sub = doc
            .subscribe_jsonpath(
                "$..total",
                Arc::new(move || {
                    hit_ref.fetch_add(1, Ordering::SeqCst);
                }),
            )
            .unwrap();

        nested.insert("total", 4).unwrap();
        doc.commit_then_renew();

        assert!(hit.load(Ordering::SeqCst) >= 1);
    }

    #[test]
    fn jsonpath_subscribe_filter_treated_as_wildcard() {
        let doc = LoroDoc::new_auto_commit();
        make_book(&doc, 0, "A", true, 10);
        let second_book = make_book(&doc, 1, "B", false, 20);
        doc.commit_then_renew();

        let hit = Arc::new(AtomicUsize::new(0));
        let hit_ref = hit.clone();
        let _sub = doc
            .subscribe_jsonpath(
                "$.books[?@.available].title",
                Arc::new(move || {
                    hit_ref.fetch_add(1, Ordering::SeqCst);
                }),
            )
            .unwrap();

        second_book.insert("available", true).unwrap();
        doc.commit_then_renew();

        assert!(hit.load(Ordering::SeqCst) >= 1);
    }

    #[test]
    fn jsonpath_subscribe_triggers_once_per_commit() {
        let doc = LoroDoc::new_auto_commit();
        let book = make_book(&doc, 0, "A", true, 10);
        doc.commit_then_renew();

        let hit = Arc::new(AtomicUsize::new(0));
        let hit_ref = hit.clone();
        let _sub = doc
            .subscribe_jsonpath(
                "$.books[0].title",
                Arc::new(move || {
                    hit_ref.fetch_add(1, Ordering::SeqCst);
                }),
            )
            .unwrap();

        book.insert("title", "X").unwrap();
        book.insert("title", "Y").unwrap();
        doc.commit_then_renew();

        assert_eq!(hit.load(Ordering::SeqCst), 1);
    }
}
