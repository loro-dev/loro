# Undo Performance Benchmark

This is a standalone benchmark to demonstrate and profile the performance overhead of Loro's undo system when working with large content operations.

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

## Key Findings

The benchmark shows that:

1. Delete operations with UndoManager enabled can be significantly slower
2. The overhead is most noticeable for small-to-medium content (100B-5KB)
3. The root cause is that deleted content is eagerly copied into memory for potential undo operations

## Implementation Details

The performance overhead comes from the `apply_local_op` method in `richtext_state.rs`, which calls `get_text_slice_by_event_index` to copy all deleted text immediately when a delete operation occurs. This happens even if undo is never used.

For large documents or frequent delete operations, this can create significant performance bottlenecks.