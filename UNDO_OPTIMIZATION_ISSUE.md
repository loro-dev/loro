# Undo Optimization Issue with Grouped Operations

## Problem Description

The undo optimization feature that stores pre-calculated undo diffs has a correctness issue when used with grouped operations. The issue manifests as `OutOfBound` errors when trying to undo grouped operations.

## Root Cause

When operations are grouped using `undo.group_start()` and `undo.group_end()`, multiple operations are combined into a single undo item. The undo diffs for these operations are generated at different points in the document's history and assume they'll be applied to specific states.

### Example

Consider these grouped operations:
1. Insert "Hello" at position 0 → Document: "Hello"
2. Insert " World" at position 5 → Document: "Hello World"  
3. Delete 6 characters at position 0 → Document: "World"

The undo diffs generated would be:
1. Delete 5 at position 0 (to undo "Hello")
2. Delete 6 at position 5 (to undo " World")
3. Insert "Hello " at position 0 (to undo the delete)

When we try to compose these diffs into a single undo operation:
- After applying the first diff (delete 5 at 0), the document is empty
- The second diff tries to delete at position 5, which no longer exists
- This causes an `OutOfBound` error

## Current Workaround

The current fix (commit c5399fd7) disables the optimization for grouped operations by clearing the `undo_diff` when operations are grouped. This forces the system to use the fallback path that performs checkouts, which correctly handles the sequential nature of grouped operations but has O(n²) complexity.

## Proper Fix

A proper fix would require implementing position transformation when composing diffs from grouped operations. This would involve:

1. **Sequential Application**: Instead of composing the diffs into a single diff, store them separately and apply them sequentially during undo/redo.

2. **Position Transformation**: When composing diffs, transform the positions in later diffs based on the effects of earlier diffs. This is complex because:
   - Insertions shift positions forward
   - Deletions shift positions backward
   - The transformation depends on the order and nature of operations

3. **Redo Handling**: Ensure that redo operations can properly reconstruct the original operations from the undo diffs.

## Impact

- **Performance**: Grouped operations don't benefit from the undo optimization and fall back to O(n²) complexity
- **Correctness**: The current workaround ensures correctness at the cost of performance
- **User Experience**: Most users won't notice the performance difference unless they're working with very large documents or many grouped operations

## Future Work

1. Implement proper position transformation for diff composition
2. Consider storing grouped diffs separately and applying them sequentially
3. Add more comprehensive tests for edge cases in grouped undo operations
4. Investigate alternative approaches like operation transformation (OT) techniques

## Related Code

- `crates/loro-internal/src/undo.rs`: Main undo implementation
- `crates/loro-internal/src/event.rs`: Diff composition logic
- `crates/delta/src/delta_rope/compose.rs`: Delta composition implementation