# Undo Performance Optimization

## Overview

This document describes the undo performance optimization implemented in Loro that reduces the time complexity of consecutive undo operations from O(n²) to O(n).

## Problem Statement

In the original implementation (v1.5.9 and earlier), each undo operation required:
1. Checkout to the state before the operation
2. Checkout to the state after the operation
3. Calculate the diff between these two states
4. Apply the inverted diff

This resulted in O(n²) time complexity when undoing n consecutive operations, as each checkout operation has O(n) complexity where n is the number of operations in the document.

## Solution: Pre-calculated Diffs

The optimization pre-calculates and stores the inverse operations (undo diffs) during the original operation execution. This eliminates the need for checkouts during undo.

### Architecture

1. **Diff Generation**: When operations are applied in `apply_local_op`, each state handler generates the inverse operation:
   - Text insertions generate delete operations
   - Text deletions capture the deleted content for restoration
   - List/Map insertions generate corresponding deletions
   - Style changes capture previous style values

2. **Diff Storage**: The generated diffs are:
   - Collected in the transaction's `undo_diff` field
   - Emitted through the `undo_subs` subscription system
   - Received by UndoManager and stored in `pending_undo_diff`
   - Associated with the operation span when checkpoint is recorded

3. **Diff Usage**: During undo:
   - The pre-calculated diff is retrieved from the undo stack
   - Applied directly without any checkouts
   - Transformed if necessary based on concurrent operations

## Performance Improvements

Based on benchmark results comparing the optimized version with v1.5.9:

### Small Operations (10 ops)
- **Before**: ~110µs per undo
- **After**: ~12µs per undo
- **Speedup**: 9x faster

### Medium Operations (50 ops)
- **Before**: ~1.14ms per undo
- **After**: ~27µs per undo
- **Speedup**: 42x faster

### Large Operations (100 ops)
- **Before**: ~3.58ms per undo
- **After**: ~75µs per undo
- **Speedup**: 47x faster

### Real-world Workload (mixed operations)
- **Before**: ~1.5ms per undo
- **After**: ~6.7µs per undo
- **Speedup**: 225x faster

### Time Complexity
- **Before**: O(n²) for n consecutive undos
- **After**: O(n) for n consecutive undos

## Memory Trade-offs

The optimization trades memory for speed:
- Each operation stores its inverse diff
- Memory usage grows linearly with operation count and content size
- For text operations: ~1 byte per character deleted
- For container operations: metadata + value size

## Fallback Path

The system maintains a fallback path for compatibility:
- Operations performed before UndoManager creation
- Imported operations from remote peers
- Legacy documents without pre-calculated diffs

## Running Benchmarks

To compare performance between versions:

```bash
cd crates/fuzz
cargo bench --bench undo_comparison
```

The benchmark compares:
- Text operations (10-500 ops)
- List operations (10-500 ops)
- Map operations (10-200 ops)
- Large content scenarios
- Mixed operation workloads

## Implementation Details

### Key Components

1. **DiffBatch** (`undo.rs`): Stores container ID to diff mappings
2. **State Handlers**: Generate inverse operations in `apply_local_op`
3. **UndoManager**: Subscribes to `undo_diffs` to receive pre-calculated diffs
4. **Transaction**: Collects and emits undo diffs after commit

### Critical Connection

The optimization requires UndoManager to subscribe to undo diffs:

```rust
let undo_diff_sub = doc.subscribe_undo_diffs(Box::new(move |diff_batch| {
    inner.pending_undo_diff.compose(diff_batch);
    true
}));
```

Without this subscription, the system falls back to the slow checkout-based path.

## Future Optimizations

Potential improvements:
1. Lazy diff generation (only when undo is performed)
2. Compression for large diffs
3. Reference-based storage for large deleted content
4. Hybrid approach based on operation size thresholds