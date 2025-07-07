# Undo Transformation Implementation Plan

## Current Status

### Working
- Basic transformation for simple insertions and deletions
- Position shifting when remote insertions occur before local operations
- Single-user undo/redo with optimization
- Grouped operations (with optimization disabled)

### Not Working
- Overlapping delete operations
- Complex collaborative scenarios with multiple transformations
- Content-aware transformations

## Implementation Phases

### Phase 1: Detection and Fallback (Immediate)
**Goal**: Detect complex cases and fall back to safe path

1. **Implement complexity detection**:
   ```rust
   fn is_complex_transformation(local_diff: &DiffBatch, remote_diff: &DiffBatch) -> bool {
       // Check for:
       // - Overlapping deletes
       // - Multiple containers affected
       // - Large position shifts
       // - Delete operations in both diffs
   }
   ```

2. **Use detection in undo path**:
   ```rust
   let use_optimized_path = !span.undo_diff.cid_to_events.is_empty() 
       && !has_excluded_origins
       && (!has_remote_changes || !is_complex_transformation(&span.undo_diff, &remote_diff));
   ```

### Phase 2: Enhanced Position Tracking (Short-term)
**Goal**: Improve position transformation accuracy

1. **Track operation ranges**:
   ```rust
   struct OperationRange {
       start: usize,
       end: usize,
       operation_id: OpId,
   }
   ```

2. **Implement range-based transformation**:
   - Track which positions are affected by each operation
   - Transform ranges, not just positions
   - Handle overlaps properly

### Phase 3: Content-Aware Transformation (Medium-term)
**Goal**: Track content to ensure correct transformations

1. **Enhance undo diff storage**:
   ```rust
   struct ContentAwareUndo {
       position: usize,
       content: Option<ContentHash>,
       operation: UndoOp,
   }
   ```

2. **Implement content verification**:
   - Verify content before applying undo
   - Adjust operation if content has moved
   - Fall back if content is not found

### Phase 4: Full Transformation System (Long-term)
**Goal**: Complete operational transformation for all cases

1. **Implement transformation algebra**:
   - Define transformation rules for all operation pairs
   - Handle all container types
   - Support all edge cases

2. **Optimize for common cases**:
   - Cache transformation results
   - Batch transformations
   - Minimize overhead

## Testing Strategy

### Unit Tests
1. **Transformation primitives**:
   - Position transformation
   - Range overlap detection
   - Content tracking

2. **Edge cases**:
   - Empty operations
   - Boundary conditions
   - Maximum position values

### Integration Tests
1. **Collaborative scenarios**:
   - Two users editing same region
   - Three or more users
   - Rapid concurrent edits

2. **Complex operations**:
   - Grouped operations with remote changes
   - Undo/redo cycles with collaboration
   - Container deletions and recreations

### Property-Based Tests
1. **Invariants**:
   - Undo always reverses the operation
   - Transformation maintains document consistency
   - No data loss

2. **Properties**:
   - Commutativity where applicable
   - Associativity of transformations
   - Convergence in collaborative editing

## Performance Considerations

1. **Optimization targets**:
   - Simple single-user: O(1) with pre-calculated diffs
   - Simple collaborative: O(n) with transformation
   - Complex collaborative: O(n²) fallback acceptable

2. **Memory usage**:
   - Minimize additional storage in undo diffs
   - Share common data structures
   - Compress where possible

## Risk Mitigation

1. **Backward compatibility**:
   - Maintain existing API
   - Support old undo entries
   - Gradual rollout with feature flags

2. **Correctness over performance**:
   - Always fall back to safe path when uncertain
   - Extensive testing before enabling optimization
   - Monitor for issues in production

## Success Metrics

1. **Correctness**:
   - All tests pass
   - No data corruption
   - Predictable behavior

2. **Performance**:
   - 90% of undo operations use optimized path
   - <10ms for typical undo operation
   - No noticeable lag in collaborative scenarios

3. **Maintainability**:
   - Clear separation of concerns
   - Well-documented transformation rules
   - Easy to add new operation types