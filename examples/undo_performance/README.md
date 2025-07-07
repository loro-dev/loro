# Undo Performance Benchmark

This is a comprehensive standalone benchmark to demonstrate and profile multiple performance issues in Loro's undo system.

## Running the Benchmark

```bash
cd examples/undo_performance
cargo run --release
```

## Profiling

This benchmark is designed to be used with profiling tools:

```bash
# Using cargo-flamegraph
cargo flamegraph

# Using perf
cargo build --release
perf record --call-graph=dwarf target/release/undo_performance
perf report

# Using samply
cargo build --release
samply record target/release/undo_performance
```

## Benchmark Tests

The benchmark includes 6 comprehensive tests:

1. **Operation Overhead**: Measures the performance overhead of having UndoManager enabled during insert operations
2. **Consecutive Undo Performance**: Demonstrates O(n²) behavior when performing many consecutive undo operations
3. **Document Size Scaling**: Shows how undo performance degrades as document size increases
4. **Optimized vs Fallback Path**: Compares performance between pre-calculated diffs and the fallback checkout-based path
5. **Memory Overhead Analysis**: Estimates memory usage from storing pre-calculated undo diffs
6. **Profiling Scenario**: A realistic mixed workload suitable for profiling tools

## Key Findings

The benchmark reveals several performance issues:

1. **Operation Overhead**: Insert operations are 1.2-2x slower with UndoManager enabled
2. **O(n²) Behavior**: Consecutive undo operations can exhibit quadratic time complexity
3. **Memory Overhead**: Pre-calculated diffs consume significant memory (grows with content size × operation count)
4. **Path Performance**: The "optimized" path with pre-calculated diffs can sometimes be slower than the fallback path
5. **Cumulative Impact**: In real-world scenarios with mixed operations, the overhead accumulates significantly

## Implementation Issues

The performance problems stem from several design decisions:

1. **Eager Diff Generation**: The `apply_local_op` method generates undo diffs immediately during operations
2. **Content Copying**: Delete operations copy all deleted content via `get_text_slice_by_event_index`
3. **Memory Retention**: All diffs are kept in memory even if undo is never used
4. **Transform Overhead**: Diffs must be transformed when remote operations occur

## Optimization Opportunities

- Lazy diff generation (only when undo is performed)
- Reference-based storage instead of copying content
- Hybrid approach based on operation size
- Compression for large diffs
- Incremental state tracking to avoid full checkouts