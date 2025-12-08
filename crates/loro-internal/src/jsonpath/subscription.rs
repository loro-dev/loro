use std::sync::Arc;

use loro_common::TreeID;
use rustc_hash::FxHashSet;
use smallvec::SmallVec;

use crate::{
    event::{Diff, Index},
    jsonpath::{
        ast::{Query, Segment, Selector},
        JSONPathParser,
    },
    utils::subscription::Subscription,
    version::Frontiers,
    LoroDoc, LoroError, LoroResult,
};

/// Callback used by `subscribe_jsonpath`.
///
/// Note: the callback does **not** carry the query result. It is intended as a
/// lightweight notification so applications can debounce/throttle and evaluate
/// JSONPath themselves if needed.
pub type SubscribeJsonPathCallback = Arc<dyn Fn() + Send + Sync + 'static>;

/// Represents a single path segment used for matching against events.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PathElem {
    Key(String),
    /// None means "some sequence index changed (unknown / any)".
    Seq(Option<usize>),
    Node(TreeID),
}

impl From<Index> for PathElem {
    fn from(value: Index) -> Self {
        match value {
            Index::Key(k) => PathElem::Key(k.to_string()),
            Index::Seq(i) => PathElem::Seq(Some(i)),
            Index::Node(n) => PathElem::Node(n),
        }
    }
}

/// A compiled matcher that conservatively checks whether a change at some path
/// can affect the JSONPath query. False positives are allowed; false negatives
/// are not.
///
/// Matching model (conservative, no false negatives):
/// - Linearize JSONPath into steps (`selectors`, `recursive`).
/// - Drive an NFA (nondeterministic finite automaton) along the event path: a step
///   consumes a level when it matches; `recursive` also lets the automaton stay on
///   the same step and dive deeper. We keep a set of possible step indices after
///   each path element.
/// - Selector rules: Name/Index match exactly; negative Index/Slice/Wild/Filter
///   are treated as wildcards to avoid false negatives; TreeID matches Node.
/// - For list/tree children we append `Seq(None)` to represent “some child changed”.
/// - Map + Filter: try to extract `filter_keys`; if unknown, any map change whose
///   prefix matches the query triggers; otherwise only changes touching keys in
///   `filter_keys` or whose full key path matches the query will trigger.
#[derive(Debug, Clone)]
struct JsonPathMatcher {
    steps: Vec<Step>,
    has_filter: bool,
    filter_keys: Option<FxHashSet<String>>, // None => unknown, Some => keys that may affect filters
}

#[derive(Debug, Clone)]
struct Step {
    recursive: bool,
    selectors: Vec<Selector>,
}

impl JsonPathMatcher {
    fn new(query: &Query) -> Self {
        let mut steps = Vec::new();
        let mut has_filter = false;
        let mut filter_keys: Option<FxHashSet<String>> = Some(Default::default());
        build_steps(&query.segments, &mut steps);
        for step in steps.iter() {
            if step
                .selectors
                .iter()
                .any(|s| matches!(s, Selector::Filter { .. }))
            {
                has_filter = true;
                break;
            }
        }
        collect_filter_keys_from_segment(&query.segments, &mut filter_keys);
        JsonPathMatcher {
            steps,
            has_filter,
            filter_keys,
        }
    }

    /// Returns true if the provided path (from root) could affect the query.
    ///
    /// NFA keeps current step positions; `recursive` allows staying on the
    /// same step across deeper levels. A match exists if any position reaches
    /// or passes the last step after consuming the path.
    fn may_match(&self, path: &[PathElem]) -> bool {
        if self.steps.is_empty() {
            return true;
        }

        let positions = self.positions_after(path);

        positions.iter().any(|&p| p >= self.steps.len())
    }

    fn has_filter(&self) -> bool {
        self.has_filter
    }

    fn maybe_filter_keys(&self) -> Option<&rustc_hash::FxHashSet<String>> {
        self.filter_keys.as_ref()
    }

    fn positions_after(&self, path: &[PathElem]) -> SmallVec<[usize; 8]> {
        // Simulate the NFA on a given path and return all reachable step indices
        // after consuming the entire path. An index == steps.len() means the
        // query may already match; indices < len mean a partial prefix match.
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

                if step.recursive {
                    next.push(pos);
                }

                if selector_matches(&step.selectors, elem) {
                    let new_pos = pos + 1;
                    next.push(new_pos);

                    if step.recursive {
                        next.push(pos);
                    }
                }
            }
            positions = dedup_positions(next);
            if positions.is_empty() {
                break;
            }
        }

        positions
    }
}

fn selector_matches(selectors: &[Selector], elem: &PathElem) -> bool {
    selectors.iter().any(|sel| match sel {
        Selector::Name { name } => matches!(elem, PathElem::Key(k) if k == name),
        Selector::Index { index } => {
            if matches!(elem, PathElem::Seq(None)) {
                return true;
            }
            match elem {
                PathElem::Seq(Some(i)) => {
                    if *index >= 0 {
                        *i as i64 == *index
                    } else {
                        // negative index: we don't know len, so treat as possible match
                        true
                    }
                }
                _ => false,
            }
        }
        Selector::Slice { .. } => matches!(
            elem,
            PathElem::Seq(_) | PathElem::Key(_) | PathElem::Node(_)
        ),
        Selector::Wild {} => true,
        Selector::Filter { .. } => true, // filters are treated as wildcard to avoid false negatives
    })
}

fn build_steps(segment: &Segment, steps: &mut Vec<Step>) {
    match segment {
        Segment::Root {} => {}
        Segment::Child { left, selectors } => {
            build_steps(left, steps);
            steps.push(Step {
                recursive: false,
                selectors: selectors.clone(),
            });
        }
        Segment::Recursive { left, selectors } => {
            build_steps(left, steps);
            steps.push(Step {
                recursive: true,
                selectors: selectors.clone(),
            });
        }
    }
}

/// Collect possible map keys referenced by filters; None means "unknown/wildcard" (fall back to always fire).
fn collect_filter_keys_from_segment(segment: &Segment, acc: &mut Option<FxHashSet<String>>) {
    match segment {
        Segment::Root {} => {}
        Segment::Child { left, selectors } | Segment::Recursive { left, selectors } => {
            collect_filter_keys_from_segment(left, acc);
            for sel in selectors {
                if let Selector::Filter { expression } = sel {
                    merge_filter_keys(acc, collect_keys_from_filter(expression));
                }
            }
        }
    }
}

fn merge_filter_keys(acc: &mut Option<FxHashSet<String>>, incoming: Option<FxHashSet<String>>) {
    match incoming {
        None => *acc = None,
        Some(src) => {
            if let Some(dst) = acc {
                for k in src {
                    dst.insert(k);
                }
            }
        }
    }
}

fn collect_keys_from_filter(
    expr: &crate::jsonpath::ast::FilterExpression,
) -> Option<FxHashSet<String>> {
    use crate::jsonpath::ast::FilterExpression::*;
    let mut set = FxHashSet::default();
    let mut unknown = false;
    fn merge(dst: &mut FxHashSet<String>, src: Option<FxHashSet<String>>, unknown: &mut bool) {
        match src {
            Some(s) => dst.extend(s),
            None => *unknown = true,
        }
    }
    match expr {
        True_ {} | False_ {} | Null {} | StringLiteral { .. } | Int { .. } | Float { .. } => {}
        Array { values } => {
            for v in values {
                merge(&mut set, collect_keys_from_filter(v), &mut unknown);
            }
        }
        Not { expression } => merge(&mut set, collect_keys_from_filter(expression), &mut unknown),
        Logical {
            left,
            right,
            operator: _,
        } => {
            merge(&mut set, collect_keys_from_filter(left), &mut unknown);
            merge(&mut set, collect_keys_from_filter(right), &mut unknown);
        }
        Comparison { left, right, .. } => {
            merge(&mut set, collect_keys_from_filter(left), &mut unknown);
            merge(&mut set, collect_keys_from_filter(right), &mut unknown);
        }
        RelativeQuery { query } | RootQuery { query } => {
            merge(
                &mut set,
                collect_keys_from_segment(&query.segments),
                &mut unknown,
            );
        }
        Function { args, .. } => {
            for a in args {
                merge(&mut set, collect_keys_from_filter(a), &mut unknown);
            }
        }
    }
    if unknown {
        None
    } else {
        Some(set)
    }
}

fn collect_keys_from_segment(segment: &Segment) -> Option<rustc_hash::FxHashSet<String>> {
    let mut set = FxHashSet::default();
    let mut unknown = false;
    fn merge(dst: &mut FxHashSet<String>, src: Option<FxHashSet<String>>, unknown: &mut bool) {
        match src {
            Some(s) => dst.extend(s),
            None => *unknown = true,
        }
    }
    match segment {
        Segment::Root {} => {}
        Segment::Child { left, selectors } => {
            merge(&mut set, collect_keys_from_segment(left), &mut unknown);
            for sel in selectors {
                match sel {
                    Selector::Name { name } => {
                        set.insert(name.clone());
                    }
                    Selector::Filter { expression } => {
                        merge(&mut set, collect_keys_from_filter(expression), &mut unknown)
                    }
                    Selector::Index { .. } | Selector::Slice { .. } | Selector::Wild {} => {
                        // index/slice/wildcard may address many keys; mark unknown
                        unknown = true;
                    }
                }
            }
        }
        Segment::Recursive { left, selectors } => {
            // Recursive descent can hit arbitrary descendants: mark unknown
            merge(&mut set, collect_keys_from_segment(left), &mut unknown);
            unknown = true;
            for sel in selectors {
                if let Selector::Filter { expression } = sel {
                    merge(&mut set, collect_keys_from_filter(expression), &mut unknown);
                }
            }
        }
    }
    if unknown {
        None
    } else {
        Some(set)
    }
}

fn dedup_positions(mut v: SmallVec<[usize; 8]>) -> SmallVec<[usize; 8]> {
    v.sort_unstable();
    v.dedup();
    v
}

impl LoroDoc {
    /// Subscribe to updates that may affect the given JSONPath query.
    ///
    /// - The callback is invoked when a change *might* alter the query result.
    /// - The callback receives no query result to stay lightweight; applications can
    ///   debounce/throttle and evaluate JSONPath themselves if needed.
    /// - The matcher is conservative: it may fire false positives, but avoids false negatives.
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

        {
            let mut state = self.state.lock().unwrap();
            if !state.is_recording() {
                state.start_recording();
            }
        }

        let last_frontiers = Arc::new(std::sync::Mutex::new(None::<Frontiers>));

        let sub = self.subscribe_root(Arc::new(move |event| {
            if event.events.is_empty() {
                return;
            }

            // Deduplicate within the same commit/import (same `to` frontiers)
            {
                let mut last = last_frontiers.lock().unwrap();
                if let Some(prev) = last.as_ref() {
                    if *prev == event.event_meta.to {
                        return;
                    }
                }
                *last = Some(event.event_meta.to.clone());
            }

            let matcher = matcher.clone();
            let mut fired = false;

            for container_diff in event.events.iter() {
                if fired {
                    break;
                }
                let base_path: Vec<PathElem> = container_diff
                    .path
                    .iter()
                    .map(|(_, idx)| idx.clone().into())
                    .collect();

                // 1) Path to the container itself (affects queries targeting the container)
                if matcher.may_match(&base_path) {
                    fired = true;
                    break;
                }

                // 2) Derived paths for map entry changes
                if let Diff::Map(map) = &container_diff.diff {
                    // Map triggering strategy:
                    // - Require the map path prefix to advance the JSONPath matcher (avoid global over-triggering).
                    // - If filter_keys is None: any change under this map whose prefix matches will trigger (conservative but bounded).
                    // - If filter_keys is known:
                    //   * key intersects filter_keys -> trigger;
                    //   * or the key path itself matches the JSONPath selectors.
                    let mut should_fire = false;
                    let filter_keys = matcher.maybe_filter_keys();

                    // Skip early if the map path prefix cannot advance the matcher.
                    let base_positions = matcher.positions_after(&base_path);
                    if base_positions.is_empty() {
                        continue;
                    }

                    if !should_fire {
                        for key in map.updated.keys() {
                            let key_in_filter = filter_keys
                                .map(|keys| keys.contains(key.as_ref()))
                                .unwrap_or(false);
                            let mut extended = base_path.clone();
                            extended.push(PathElem::Key(key.to_string()));
                            let positions = matcher.positions_after(&extended);
                            let may_match_path = matcher.may_match(&extended);
                            let positions_non_empty = !positions.is_empty();

                            // Key path is directly selected by the query (e.g., Name selector).
                            if may_match_path {
                                should_fire = true;
                                break;
                            }

                            // In filter queries, touching a key used in the filter must trigger
                            // even if the query ultimately selects a sibling field (e.g., title).
                            if matcher.has_filter() {
                                match filter_keys {
                                    None => {
                                        // Unknown filter keys but prefix matches -> conservative trigger.
                                        should_fire = true;
                                        break;
                                    }
                                    Some(_) => {
                                        if key_in_filter {
                                            should_fire = true;
                                            break;
                                        }
                                    }
                                }
                            }

                            // Without filters: if prefix matches and the subtree could contain targets
                            // (positions non-empty), conservatively trigger for whole-subtree replacements.
                            if !matcher.has_filter() && positions_non_empty {
                                should_fire = true;
                                break;
                            }
                        }
                    }

                    if should_fire {
                        fired = true;
                        break;
                    }
                }

                // 3) For lists / movable lists / trees, any child mutation may matter.
                // Add an "unknown index" segment to represent potential child impact.
                match &container_diff.diff {
                    Diff::List(_) | Diff::Tree(_) | Diff::Unknown => {
                        let mut extended = base_path.clone();
                        extended.push(PathElem::Seq(None));
                        if !matcher.positions_after(&extended).is_empty() {
                            fired = true;
                        }
                    }
                    #[cfg(feature = "counter")]
                    Diff::Counter(_) => {
                        let mut extended = base_path.clone();
                        extended.push(PathElem::Seq(None));
                        if !matcher.positions_after(&extended).is_empty() {
                            fired = true;
                        }
                    }
                    Diff::Text(_) => {
                        // text has no children; base path already checked
                    }
                    _ => {}
                }
            }

            if fired {
                (callback)();
            }
        }));

        Ok(sub)
    }
}

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
    fn jsonpath_subscribe_filter_restricts_map_keys() {
        let doc = LoroDoc::new_auto_commit();
        let book = make_book(&doc, 0, "A", true, 10);
        doc.commit_then_renew();

        let hit = Arc::new(AtomicUsize::new(0));
        let hit_ref = hit.clone();
        let _sub = doc
            .subscribe_jsonpath(
                "$.books[?@.price>5].title",
                Arc::new(move || {
                    hit_ref.fetch_add(1, Ordering::SeqCst);
                }),
            )
            .unwrap();

        // unrelated key should not trigger because filter keys = {price} and path = title
        book.insert("note", "ignored").unwrap();
        doc.commit_then_renew();
        assert_eq!(hit.load(Ordering::SeqCst), 0);

        // filter-relevant key should trigger
        book.insert("price", 42).unwrap();
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

        // Multiple updates within one commit should coalesce to a single callback.
        book.insert("title", "X").unwrap();
        book.insert("title", "Y").unwrap();
        doc.commit_then_renew();
        assert_eq!(hit.load(Ordering::SeqCst), 1);
    }
}
