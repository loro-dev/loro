# Undo Code Architecture Decision

## Current State

We have two code paths in the undo implementation:
1. **Optimized path**: Uses precalculated diffs, avoids checkouts - O(n) complexity
2. **Fallback path**: Uses `undo_internal` which performs multiple checkouts - O(n²) complexity

## Why We Keep Both Paths

After investigation, we've decided to keep both paths for the following reasons:

### Backward Compatibility Requirements

1. **Operations before UndoManager initialization**: When a document has operations performed before the UndoManager was created, those operations don't have precalculated undo diffs.

2. **Imported operations from other peers**: When importing changes from other peers via synchronization, those operations won't have undo diffs calculated for the local peer's undo stack.

3. **Legacy documents**: Existing documents created before this optimization was implemented need to continue working.

### Complexity of Lazy Calculation

We explored lazy diff calculation but found it introduces significant complexity:
- Transaction state management becomes complex during lazy calculation
- The checkout operations require specific locking guarantees
- Error handling becomes more difficult
- The implementation would be harder to maintain and debug

## Architecture Decision

We will maintain both paths with clear documentation:

```rust
if use_precalculated_diff {
    // Optimized path: O(n) complexity, no checkouts
    // Used for all operations created after UndoManager initialization
    apply_precalculated_diff(...)
} else {
    // Fallback path: O(n²) complexity, performs checkouts
    // Used for backward compatibility scenarios
    undo_internal(...)
}
```

## Future Improvements

1. **Monitoring**: Add metrics to track how often each path is used
2. **Migration tools**: Provide utilities to precalculate diffs for existing documents
3. **Documentation**: Clearly document when each path is used
4. **Performance warnings**: Log warnings when the slow path is used frequently

## Summary

While removing the unoptimized code would simplify the codebase, maintaining backward compatibility is more important. The dual-path approach ensures:
- New operations get optimal performance
- Existing documents continue to work
- The implementation remains maintainable
- Users don't experience breaking changes