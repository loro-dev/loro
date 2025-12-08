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
//! 1. **Compilation**: JSONPath is parsed and linearized into a sequence of `Step`s,
//!    each containing selectors and a `recursive` flag (for `..` descent).
//!
//! 2. **NFA Simulation**: When an event arrives, we simulate an NFA where:
//!    - States are positions in the step sequence (0..steps.len())
//!    - Transitions occur when a selector matches the current path element
//!    - Recursive steps allow staying at the same state while consuming input
//!    - Acceptance occurs when any state reaches or exceeds `steps.len()`
//!
//! 3. **Filter Optimization**: For queries with filter expressions (e.g., `[?@.price>5]`),
//!    we extract referenced keys to avoid triggering on unrelated map key changes.

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
    Key(String),
    /// A sequence (list/array) index.
    /// - `Some(i)` means a specific index changed
    /// - `None` means "some index changed" (used for list/tree child mutations
    ///   where we don't know which specific indices are affected)
    Seq(Option<usize>),
    /// A tree node identifier (for Loro's tree CRDT)
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
/// ## Matching Model (NFA-based, conservative)
///
/// 1. **Linearization**: The JSONPath AST is flattened into a sequence of `Step`s.
///    Each step has selectors (what to match) and a `recursive` flag (for `..`).
///
/// 2. **NFA Simulation**: We maintain a set of "positions" (indices into `steps`).
///    For each path element:
///    - If `step.recursive`: the position can stay (allowing arbitrary depth)
///    - If selector matches: advance to the next position
///    - A match occurs when any position >= `steps.len()` (all steps consumed)
///
/// 3. **Selector Matching Rules** (conservative to avoid false negatives):
///    - `Name`: exact string match against `PathElem::Key`
///    - `Index(n)`: exact match if n >= 0; wildcard if n < 0 (unknown array length)
///    - `Slice`: treated as wildcard (matches any element type)
///    - `Wild`: matches everything
///    - `Filter`: treated as wildcard (filter evaluation is deferred)
///
/// 4. **Filter Optimization**: We extract keys referenced in filter expressions
///    (e.g., `price` from `[?@.price>5]`). Map key changes only trigger if:
///    - The key path directly matches the query, OR
///    - The key is in `filter_keys` (affects filter evaluation)
///
/// ## Example
///
/// Query: `$.books[*].title`
/// Steps: `[Name("books"), Wild, Name("title")]`
///
/// Event path `[books, 0, title]`:
/// - Start: positions = {0}
/// - After "books": Name matches → positions = {1}
/// - After "0": Wild matches → positions = {2}
/// - After "title": Name matches → positions = {3}
/// - 3 >= 3 (steps.len()) → match!
#[derive(Debug, Clone)]
struct JsonPathMatcher {
    /// Linearized sequence of matching steps from the JSONPath query
    steps: Vec<Step>,
    /// Whether the query contains any filter expressions `[?...]`
    has_filter: bool,
    /// Keys referenced in filter expressions (for optimization).
    /// - `None`: unknown/complex filter, must trigger conservatively
    /// - `Some(keys)`: only these keys can affect filter evaluation
    filter_keys: Option<FxHashSet<String>>,
}

/// A single step in the linearized JSONPath query.
///
/// Corresponds to one segment in the path (e.g., `.books` or `[0]` or `..title`).
#[derive(Debug, Clone)]
struct Step {
    /// If true, this step uses recursive descent (`..`) and can match at any depth.
    /// The NFA can stay at this position while consuming input, or advance when matched.
    recursive: bool,
    /// Selectors that can match at this step. Any selector matching is sufficient.
    selectors: Vec<Selector>,
}

impl JsonPathMatcher {
    /// Compile a JSONPath query into a matcher.
    ///
    /// This performs three tasks:
    /// 1. Linearize the AST into steps
    /// 2. Detect presence of filter expressions
    /// 3. Extract keys referenced in filters (for optimization)
    fn new(query: &Query) -> Self {
        let mut steps = Vec::new();
        let mut has_filter = false;
        let mut filter_keys: Option<FxHashSet<String>> = Some(Default::default());

        // Linearize the JSONPath AST into a sequence of steps
        build_steps(&query.segments, &mut steps);

        // Check if any step contains a filter selector
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

        // Extract keys referenced in filter expressions for optimization
        collect_filter_keys_from_segment(&query.segments, &mut filter_keys);

        JsonPathMatcher {
            steps,
            has_filter,
            filter_keys,
        }
    }

    /// Returns true if the provided path (from root) could affect the query result.
    ///
    /// This is the main entry point for checking if a change should trigger the callback.
    ///
    /// ## Algorithm
    ///
    /// Simulates an NFA where positions represent progress through the query steps.
    /// A match occurs when any position reaches or exceeds `steps.len()`, meaning
    /// all required steps have been matched and the path reaches the query target.
    ///
    /// ## Example
    ///
    /// Query: `$.a.b` → steps = [Name("a"), Name("b")]
    /// - Path `[a]` → positions = {1} → 1 < 2 → no match (partial)
    /// - Path `[a, b]` → positions = {2} → 2 >= 2 → match!
    /// - Path `[a, b, c]` → positions = {2} → still a match (child of target)
    fn may_match(&self, path: &[PathElem]) -> bool {
        // Empty query (just `$`) matches everything
        if self.steps.is_empty() {
            return true;
        }

        let positions = self.positions_after(path);

        // Match if any position has consumed all steps
        positions.iter().any(|&p| p >= self.steps.len())
    }

    /// Returns whether this query contains filter expressions.
    fn has_filter(&self) -> bool {
        self.has_filter
    }

    /// Returns the set of keys referenced in filter expressions, if known.
    ///
    /// - `Some(keys)`: Only these keys can affect filter evaluation
    /// - `None`: Filter is too complex; must trigger conservatively on any key change
    fn maybe_filter_keys(&self) -> Option<&rustc_hash::FxHashSet<String>> {
        self.filter_keys.as_ref()
    }

    /// Check if any of the given positions have passed through (consumed) a filter step.
    ///
    /// This is used to determine whether filter-key changes are relevant at this path level.
    /// We only care about filter keys when we're inside an element that was selected by
    /// the filter, not when we're still at a level before the filter is evaluated.
    ///
    /// ## Example
    ///
    /// Query: `$.a.b[?@.x].c`
    /// Steps: `[Name("a"), Name("b"), Filter(?@.x), Name("c")]`
    /// Filter is at step index 2.
    ///
    /// - Path `[a]` → positions = {1} → 1 > 2? No → not past filter
    /// - Path `[a, b, item]` → positions = {3} → 3 > 2? Yes → past filter
    ///
    /// Only in the second case should filter-key changes trigger.
    fn passed_through_filter(&self, positions: &[usize]) -> bool {
        if !self.has_filter {
            return false;
        }

        // Find filter step indices and check if any position is past them
        for (step_idx, step) in self.steps.iter().enumerate() {
            let is_filter_step = step
                .selectors
                .iter()
                .any(|s| matches!(s, Selector::Filter { .. }));

            if is_filter_step {
                // If any position is past this filter step, we're inside a filtered element
                if positions.iter().any(|&pos| pos > step_idx) {
                    return true;
                }
            }
        }
        false
    }

    /// Simulate the NFA on a path and return all reachable positions after consuming it.
    ///
    /// ## Return Value Interpretation
    ///
    /// - Position `== steps.len()`: The query is fully matched (path reaches target)
    /// - Position `< steps.len()`: Partial match (path is a prefix of potential targets)
    /// - Empty result: No match possible (path diverged from query)
    ///
    /// ## NFA Transitions
    ///
    /// For each path element and each current position:
    /// 1. If step is recursive (`..`): stay at current position (ε-transition)
    /// 2. If selector matches element: advance to next position
    ///
    /// This allows recursive steps to match at any depth while non-recursive
    /// steps require exact level matching.
    fn positions_after(&self, path: &[PathElem]) -> SmallVec<[usize; 8]> {
        let mut positions = SmallVec::<[usize; 8]>::new();
        positions.push(0); // Start at the first step

        for elem in path {
            let mut next = SmallVec::<[usize; 8]>::new();

            for &pos in positions.iter() {
                // Already past all steps - propagate this "matched" state
                if pos >= self.steps.len() {
                    next.push(pos);
                    continue;
                }

                let step = &self.steps[pos];

                // Recursive descent (`..`): can stay at same position
                // This allows matching at arbitrary depth
                if step.recursive {
                    next.push(pos);
                }

                // If selector matches, advance to next step
                if selector_matches(&step.selectors, elem) {
                    next.push(pos + 1);
                }
            }

            // Deduplicate and continue
            positions = dedup_positions(next);
            if positions.is_empty() {
                break; // No possible matches - early exit
            }
        }

        positions
    }
}

/// Check if any selector in the list matches the given path element.
///
/// This function implements conservative matching: it may return true when
/// a more precise analysis would return false, but never vice versa.
///
/// ## Matching Rules by Selector Type
///
/// | Selector      | Matches                                           |
/// |---------------|---------------------------------------------------|
/// | `Name(s)`     | `Key(k)` where `k == s`                           |
/// | `Index(n≥0)`  | `Seq(Some(i))` where `i == n`, or `Seq(None)`     |
/// | `Index(n<0)`  | Any `Seq` (negative index requires array length)  |
/// | `Slice`       | Any element (conservative - slice bounds unknown) |
/// | `Wild`        | Everything                                        |
/// | `Filter`      | Everything (filter eval deferred to query time)   |
fn selector_matches(selectors: &[Selector], elem: &PathElem) -> bool {
    selectors.iter().any(|sel| match sel {
        // Name selector: exact string match against map keys
        Selector::Name { name } => matches!(elem, PathElem::Key(k) if k == name),

        // Index selector: match array indices
        Selector::Index { index } => {
            // Seq(None) means "some unknown index" - conservatively match
            if matches!(elem, PathElem::Seq(None)) {
                return true;
            }
            match elem {
                PathElem::Seq(Some(i)) => {
                    if *index >= 0 {
                        // Positive index: exact match
                        *i as i64 == *index
                    } else {
                        // Negative index (e.g., [-1] for last element):
                        // We don't know array length, so conservatively match any index
                        true
                    }
                }
                _ => false,
            }
        }

        // Slice selector (e.g., [0:2], [::2]):
        // Conservatively match any element since we don't track array bounds
        Selector::Slice { .. } => matches!(
            elem,
            PathElem::Seq(_) | PathElem::Key(_) | PathElem::Node(_)
        ),

        // Wildcard: matches everything
        Selector::Wild {} => true,

        // Filter expressions (e.g., [?@.price>5]):
        // Treated as wildcard during path matching; actual filtering
        // happens at query evaluation time
        Selector::Filter { .. } => true,
    })
}

/// Linearize the JSONPath AST into a sequence of matching steps.
///
/// The AST is a nested structure (Child/Recursive segments contain their parent).
/// We flatten it into a linear sequence where each step represents one level of
/// path matching.
///
/// ## Example
///
/// Query: `$.books..title`
/// AST: `Recursive { left: Child { left: Root, selectors: [Name("books")] }, selectors: [Name("title")] }`
/// Steps: `[Step { recursive: false, selectors: [Name("books")] },
///          Step { recursive: true, selectors: [Name("title")] }]`
fn build_steps(segment: &Segment, steps: &mut Vec<Step>) {
    match segment {
        // Root (`$`) doesn't produce a step - it's the implicit starting point
        Segment::Root {} => {}

        // Child segment (`.x` or `[x]`): exact level match required
        Segment::Child { left, selectors } => {
            build_steps(left, steps); // Process parent first (left-to-right)
            steps.push(Step {
                recursive: false,
                selectors: selectors.clone(),
            });
        }

        // Recursive segment (`..x`): can match at any depth
        Segment::Recursive { left, selectors } => {
            build_steps(left, steps); // Process parent first
            steps.push(Step {
                recursive: true,
                selectors: selectors.clone(),
            });
        }
    }
}

// =============================================================================
// Filter Key Extraction
// =============================================================================
//
// These functions extract map keys referenced in filter expressions, enabling
// an optimization: if we know exactly which keys a filter depends on, we can
// skip triggering for changes to unrelated keys.
//
// For example, for query `$.books[?@.price>5].title`:
// - Filter references key "price"
// - Only changes to "price" or "title" should trigger
// - Changes to "author" can be safely ignored
//
// If the filter is too complex (e.g., contains wildcards or recursive queries),
// we return None to indicate "unknown" and fall back to conservative triggering.
// =============================================================================

/// Traverse the AST and collect keys referenced in filter expressions.
///
/// ## Return Value
///
/// - `Some(keys)`: Accumulated set of all referenced keys
/// - `None`: Filter is too complex; must trigger conservatively
fn collect_filter_keys_from_segment(segment: &Segment, acc: &mut Option<FxHashSet<String>>) {
    match segment {
        Segment::Root {} => {}
        Segment::Child { left, selectors } | Segment::Recursive { left, selectors } => {
            // Process parent segment first
            collect_filter_keys_from_segment(left, acc);

            // Extract keys from any filter selectors
            for sel in selectors {
                if let Selector::Filter { expression } = sel {
                    merge_filter_keys(acc, collect_keys_from_filter(expression));
                }
            }
        }
    }
}

/// Merge incoming filter keys into the accumulator.
///
/// If incoming is None (unknown), the accumulator becomes None.
/// This propagates "unknown" status through the entire collection.
fn merge_filter_keys(acc: &mut Option<FxHashSet<String>>, incoming: Option<FxHashSet<String>>) {
    match incoming {
        None => *acc = None, // Unknown taints the whole result
        Some(src) => {
            if let Some(dst) = acc {
                for k in src {
                    dst.insert(k);
                }
            }
            // If acc is already None, keep it None
        }
    }
}

/// Extract keys referenced in a filter expression.
///
/// Recursively traverses the filter AST to find all map keys that could
/// affect the filter's result.
///
/// ## Examples
///
/// - `@.price > 5` → `Some({"price"})`
/// - `@.price > 5 && @.available` → `Some({"price", "available"})`
/// - `@[*]` (wildcard) → `None` (unknown)
fn collect_keys_from_filter(
    expr: &crate::jsonpath::ast::FilterExpression,
) -> Option<FxHashSet<String>> {
    use crate::jsonpath::ast::FilterExpression::*;
    let mut set = FxHashSet::default();
    let mut unknown = false;

    // Helper to merge results and track unknown status
    fn merge(dst: &mut FxHashSet<String>, src: Option<FxHashSet<String>>, unknown: &mut bool) {
        match src {
            Some(s) => dst.extend(s),
            None => *unknown = true,
        }
    }

    match expr {
        // Literals don't reference any keys
        True_ {} | False_ {} | Null {} | StringLiteral { .. } | Int { .. } | Float { .. } => {}

        // Array literals: check each element
        Array { values } => {
            for v in values {
                merge(&mut set, collect_keys_from_filter(v), &mut unknown);
            }
        }

        // Logical NOT: check inner expression
        Not { expression } => merge(&mut set, collect_keys_from_filter(expression), &mut unknown),

        // Logical AND/OR: check both sides
        Logical {
            left,
            right,
            operator: _,
        } => {
            merge(&mut set, collect_keys_from_filter(left), &mut unknown);
            merge(&mut set, collect_keys_from_filter(right), &mut unknown);
        }

        // Comparisons (==, !=, <, >, etc.): check both operands
        Comparison { left, right, .. } => {
            merge(&mut set, collect_keys_from_filter(left), &mut unknown);
            merge(&mut set, collect_keys_from_filter(right), &mut unknown);
        }

        // Embedded queries (@.foo, $.bar): extract keys from the query path
        RelativeQuery { query } | RootQuery { query } => {
            merge(
                &mut set,
                collect_keys_from_segment(&query.segments),
                &mut unknown,
            );
        }

        // Function calls: check all arguments
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

/// Extract keys referenced in a path segment (used within filters).
///
/// ## Returns
///
/// - `Some(keys)`: Keys that could be accessed by this segment
/// - `None`: Segment uses wildcards/slices/recursion (unknown keys)
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
                    // Named key: add to known set
                    Selector::Name { name } => {
                        set.insert(name.clone());
                    }
                    // Nested filter: recursively extract keys
                    Selector::Filter { expression } => {
                        merge(&mut set, collect_keys_from_filter(expression), &mut unknown)
                    }
                    // Index/Slice/Wild can address arbitrary elements: mark unknown
                    Selector::Index { .. } | Selector::Slice { .. } | Selector::Wild {} => {
                        unknown = true;
                    }
                }
            }
        }

        // Recursive descent can reach arbitrary descendants: always unknown
        Segment::Recursive { left, selectors } => {
            merge(&mut set, collect_keys_from_segment(left), &mut unknown);
            unknown = true; // Recursive = unpredictable depth
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

/// Remove duplicate positions from the NFA state set.
///
/// This keeps the state set minimal for efficient processing.
/// Using SmallVec<[usize; 8]> avoids heap allocation for typical queries
/// with fewer than 8 concurrent NFA states.
fn dedup_positions(mut v: SmallVec<[usize; 8]>) -> SmallVec<[usize; 8]> {
    v.sort_unstable();
    v.dedup();
    v
}

// =============================================================================
// Public API
// =============================================================================

impl LoroDoc {
    /// Subscribe to updates that may affect the given JSONPath query.
    ///
    /// ## Overview
    ///
    /// This method allows you to receive notifications when changes to the document
    /// *might* alter the result of a JSONPath query. It's designed for efficiency:
    /// rather than re-evaluating the JSONPath on every change, it uses a lightweight
    /// matcher to detect potentially relevant changes.
    ///
    /// ## Parameters
    ///
    /// - `jsonpath`: A JSONPath query string (e.g., `$.books[*].title`, `$..price`)
    /// - `callback`: A function called when a relevant change is detected
    ///
    /// ## Behavior
    ///
    /// - **Conservative matching**: The callback may fire for changes that don't
    ///   actually affect the query result (false positives), but it will never
    ///   miss a change that does affect it (no false negatives).
    /// - **Lightweight notifications**: The callback receives no payload. Applications
    ///   should debounce/throttle and re-evaluate the JSONPath if needed.
    /// - **Per-commit deduplication**: Multiple changes in a single commit trigger
    ///   only one callback invocation.
    ///
    /// ## Example
    ///
    /// ```ignore
    /// let sub = doc.subscribe_jsonpath("$.users[*].name", Arc::new(|| {
    ///     println!("User names may have changed!");
    /// }))?;
    ///
    /// // Later: drop `sub` to unsubscribe
    /// ```
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
        // Parse the JSONPath query
        let query = JSONPathParser::new()
            .parse(jsonpath)
            .map_err(|e| LoroError::ArgErr(e.to_string().into_boxed_str()))?;

        // Compile into a matcher for efficient event filtering
        let matcher = Arc::new(JsonPathMatcher::new(&query));

        // Subscribe to all document events and filter based on the JSONPath
        let sub = self.subscribe_root(Arc::new(move |event| {
            if event.events.is_empty() {
                return;
            }

            let matcher = matcher.clone();
            let mut fired = false;

            // ─────────────────────────────────────────────────────────────────
            // Process each container diff in the event
            // ─────────────────────────────────────────────────────────────────
            for container_diff in event.events.iter() {
                if fired {
                    break;
                }

                // Convert the container's path to our PathElem representation
                let base_path: Vec<PathElem> = container_diff
                    .path
                    .iter()
                    .map(|(_, idx)| idx.clone().into())
                    .collect();

                // ─────────────────────────────────────────────────────────────
                // Check 1: Does the container path itself match the query?
                // ─────────────────────────────────────────────────────────────
                // This catches changes to containers that are direct targets
                // of the query (e.g., query `$.books` and change to books list)
                if matcher.may_match(&base_path) {
                    fired = true;
                    break;
                }

                // ─────────────────────────────────────────────────────────────
                // Check 2: Map-specific handling
                // ─────────────────────────────────────────────────────────────
                // For map changes, we need to check each changed key to see if
                // it could affect the query result.
                if let Diff::Map(map) = &container_diff.diff {
                    let mut should_fire = false;
                    let filter_keys = matcher.maybe_filter_keys();

                    // Early exit: if the map's path doesn't advance the matcher,
                    // none of its keys can match either
                    let base_positions = matcher.positions_after(&base_path);
                    if base_positions.is_empty() {
                        continue;
                    }

                    // Check if we're inside a filtered element (past a filter step).
                    // This determines whether filter-key changes are relevant here.
                    //
                    // Example: Query `$.a.b[?@.x].c`
                    // - At path `[a]` (positions={1}): NOT past filter → filter keys don't matter
                    // - At path `[a,b,item]` (positions={3}): past filter → filter keys matter
                    let past_filter = matcher.passed_through_filter(&base_positions);

                    // Check each changed key
                    for key in map.updated.keys() {
                        // Is this key referenced in a filter expression?
                        let key_in_filter = filter_keys
                            .map(|keys| keys.contains(key.as_ref()))
                            .unwrap_or(false);

                        // Build the full path including this key
                        let mut extended = base_path.clone();
                        extended.push(PathElem::Key(key.to_string()));

                        let may_match_path = matcher.may_match(&extended);
                        let positions_non_empty = !matcher.positions_after(&extended).is_empty();

                        // ─────────────────────────────────────────────────────
                        // Trigger condition 1: Key path directly matches query
                        // ─────────────────────────────────────────────────────
                        // Example: Query `$.books[0].title`, change to `title` key
                        if may_match_path {
                            should_fire = true;
                            break;
                        }

                        // ─────────────────────────────────────────────────────
                        // Trigger condition 2: Key affects filter evaluation
                        // ─────────────────────────────────────────────────────
                        // Only relevant when we're INSIDE a filtered element (past the filter).
                        // Example: Query `$.books[?@.price>5].title`, change to `price`
                        // at path `$.books[0]` (inside a book element).
                        //
                        // NOT triggered when we're BEFORE the filter level.
                        // Example: Query `$.a.b[?@.x].c`, change to `x` at path `$.a`
                        // We haven't reached the filter level yet, so this shouldn't fire.
                        if matcher.has_filter() && past_filter {
                            match filter_keys {
                                None => {
                                    // Filter keys unknown → conservative trigger
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

                        // ─────────────────────────────────────────────────────
                        // Trigger condition 3: Subtree may contain targets
                        // ─────────────────────────────────────────────────────
                        // For queries without filters, if the key path advances
                        // the matcher, the changed subtree may contain targets.
                        // Example: Query `$.data.users[*].name`, change to `users` map
                        if !matcher.has_filter() && positions_non_empty {
                            should_fire = true;
                            break;
                        }
                    }

                    if should_fire {
                        fired = true;
                        break;
                    }
                }

                // ─────────────────────────────────────────────────────────────
                // Check 3: List/Tree child mutations
                // ─────────────────────────────────────────────────────────────
                // For lists and trees, we don't know exactly which indices changed,
                // so we append `Seq(None)` ("some index") to check if any child
                // change could match the query.
                match &container_diff.diff {
                    Diff::List(_) | Diff::Tree(_) | Diff::Unknown => {
                        let mut extended = base_path.clone();
                        extended.push(PathElem::Seq(None)); // "some child changed"
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
                        // Text containers have no children to traverse;
                        // the base path check above is sufficient
                    }
                    _ => {}
                }
            }

            // ─────────────────────────────────────────────────────────────────
            // Fire the callback if any change matched
            // ─────────────────────────────────────────────────────────────────
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

    /// Helper to create a book entry in the document.
    ///
    /// Structure: `$.books[idx] = { title, available, price }`
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

    // ─────────────────────────────────────────────────────────────────────────
    // Test: Exact key matching
    // ─────────────────────────────────────────────────────────────────────────
    /// Verifies that changing a specific key triggers when the query targets that exact path.
    ///
    /// Query: `$.books[0].title`
    /// Change: Update `title` of the first book
    /// Expected: Callback fires
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

    // ─────────────────────────────────────────────────────────────────────────
    // Test: Wildcard matching on arrays
    // ─────────────────────────────────────────────────────────────────────────
    /// Verifies that wildcard `[*]` matches any array element.
    ///
    /// Query: `$.books[*].price`
    /// Change: Update `price` of the second book
    /// Expected: Callback fires (wildcard matches index 1)
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

    // ─────────────────────────────────────────────────────────────────────────
    // Test: Negative index matching
    // ─────────────────────────────────────────────────────────────────────────
    /// Verifies that negative indices (e.g., `[-1]` for last element) are handled
    /// conservatively by matching any index.
    ///
    /// Query: `$.books[-1].title`
    /// Change: Update `title` of the last book (index 1)
    /// Expected: Callback fires
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

    // ─────────────────────────────────────────────────────────────────────────
    // Test: Slice range matching
    // ─────────────────────────────────────────────────────────────────────────
    /// Verifies that slice selectors (e.g., `[0:2]`) match conservatively.
    ///
    /// Query: `$.books[0:2].title`
    /// Change: Update `title` of the second book (within slice range)
    /// Expected: Callback fires
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

    // ─────────────────────────────────────────────────────────────────────────
    // Test: Recursive descent matching
    // ─────────────────────────────────────────────────────────────────────────
    /// Verifies that recursive descent (`..`) matches at any depth.
    ///
    /// Query: `$..total`
    /// Structure: `$.store.inventory.total`
    /// Change: Update `total` in nested structure
    /// Expected: Callback fires
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

    // ─────────────────────────────────────────────────────────────────────────
    // Test: Filter expressions trigger on filter-relevant changes
    // ─────────────────────────────────────────────────────────────────────────
    /// Verifies that changes to filter-relevant fields trigger callbacks even
    /// when those fields aren't the final target.
    ///
    /// Query: `$.books[?@.available].title` (select title of available books)
    /// Change: Update `available` field (affects which books pass the filter)
    /// Expected: Callback fires
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

        // Changing `available` may cause this book to now pass the filter
        second_book.insert("available", true).unwrap();
        doc.commit_then_renew();

        assert!(hit.load(Ordering::SeqCst) >= 1);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Test: Filter key optimization
    // ─────────────────────────────────────────────────────────────────────────
    /// Verifies that changes to keys NOT referenced in filters DON'T trigger,
    /// while changes to filter-referenced keys DO trigger.
    ///
    /// Query: `$.books[?@.price>5].title`
    /// Filter keys: `{price, title}`
    ///
    /// Test case 1: Change `note` (not in filter) → No callback
    /// Test case 2: Change `price` (in filter) → Callback fires
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

        // Unrelated key: should NOT trigger
        // `note` is not in filter keys {price} and not in query path {title}
        book.insert("note", "ignored").unwrap();
        doc.commit_then_renew();
        assert_eq!(hit.load(Ordering::SeqCst), 0);

        // Filter-relevant key: SHOULD trigger
        // Changing `price` may change which books pass the `@.price>5` filter
        book.insert("price", 42).unwrap();
        doc.commit_then_renew();
        assert!(hit.load(Ordering::SeqCst) >= 1);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Test: Per-commit deduplication
    // ─────────────────────────────────────────────────────────────────────────
    /// Verifies that multiple changes within a single commit only trigger
    /// the callback once (not multiple times).
    ///
    /// Query: `$.books[0].title`
    /// Change: Update `title` twice in one commit
    /// Expected: Callback fires exactly once
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

        // Multiple updates in one commit should coalesce to a single callback
        book.insert("title", "X").unwrap();
        book.insert("title", "Y").unwrap();
        doc.commit_then_renew();

        // Exactly one callback, not two
        assert_eq!(hit.load(Ordering::SeqCst), 1);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Test: Filter keys only matter at/after filter level
    // ─────────────────────────────────────────────────────────────────────────
    /// Verifies that filter-key changes don't trigger when we haven't reached
    /// the filter level in the query path.
    ///
    /// Query: `$.store.items[?@.active].name`
    ///        Filter is at level 3 ($.store.items[?...])
    ///
    /// Test case 1: Change `active` at `$.store` level (before filter) → No callback
    /// Test case 2: Change `active` at `$.store.items[0]` level (at filter) → Callback fires
    #[test]
    fn jsonpath_subscribe_filter_keys_only_at_filter_level() {
        let doc = LoroDoc::new_auto_commit();

        // Build structure: $.store = { items: [{ name: "A", active: true }], active: false }
        let store = doc.get_map("store");
        let items = store
            .insert_container("items", crate::ListHandler::new_detached())
            .unwrap();
        let item = items
            .insert_container(0, MapHandler::new_detached())
            .unwrap();
        item.insert("name", "A").unwrap();
        item.insert("active", true).unwrap();

        // Also add an "active" key at the store level (before the filter)
        store.insert("active", false).unwrap();
        doc.commit_then_renew();

        let hit = Arc::new(AtomicUsize::new(0));
        let hit_ref = hit.clone();
        let _sub = doc
            .subscribe_jsonpath(
                "$.store.items[?@.active].name",
                Arc::new(move || {
                    hit_ref.fetch_add(1, Ordering::SeqCst);
                }),
            )
            .unwrap();

        // Change `active` at store level - should NOT trigger
        // because we haven't reached the filter level yet
        store.insert("active", true).unwrap();
        doc.commit_then_renew();
        assert_eq!(
            hit.load(Ordering::SeqCst),
            0,
            "Filter key change before filter level should not trigger"
        );

        // Change `active` at item level - SHOULD trigger
        // because we're at the filter level (inside items[0])
        item.insert("active", false).unwrap();
        doc.commit_then_renew();
        assert!(
            hit.load(Ordering::SeqCst) >= 1,
            "Filter key change at filter level should trigger"
        );
    }
}
