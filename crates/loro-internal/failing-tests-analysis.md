# Failing Tests Analysis

## Summary
Total failing tests: 15

## Categorized Failing Tests

### 1. Undo/Redo Tests (12 tests)
These tests are related to the undo/redo functionality and are likely affected by the recent undo optimization changes.

#### Core Undo Tests (10 tests in loro::loro_rust_test::integration_test::undo_test)
1. `exclude_certain_local_ops_from_undo` - Tests excluding specific operations from undo
2. `test_remote_merge_transform` - Tests undo behavior with remote merge transforms
3. `test_undo_container_deletion` - Tests undoing container deletion operations
4. `undo_id_span_that_contains_remote_deps_inside_many_times` - Complex undo scenario with remote dependencies
5. `undo_list_move` - Tests undoing list move operations
6. `undo_redo_when_collab` - Tests undo/redo in collaborative scenarios
7. `undo_richtext_conflict_set_style` - Tests undo with conflicting rich text style operations
8. `undo_sub_sub_container` - Tests undo with nested containers
9. `undo_transform_cursor_position` - Tests cursor position transformation during undo
10. `undo_with_custom_commit_options` - Tests undo with custom commit configurations

#### Detached Editing Undo Test (1 test)
11. `undo_still_works_after_detached_editing` (in loro::loro_rust_test::integration_test::detached_editing_test)
    - Tests that undo functionality remains intact after detached editing

#### Undo Consistency Test (1 test)
12. `test_undo_consistency_movable_list` (in loro-internal::undo_consistency)
    - Tests undo consistency specifically for movable lists

### 2. Performance Tests (1 test)
1. `test_performance_improvement_from_avoiding_checkouts` (in loro-internal::undo_perf_verification)
   - Error: "Redo took too long: 1.25567644s"
   - This test verifies that the undo optimization provides performance improvements

### 3. Fuzz Tests (2 tests)
1. `fast_snapshot_5` (in fuzz::test)
   - Panic: "called `Result::unwrap()` on an `Err` value: OutOfBound { pos: 103, len: 86, info: "Position: crates/loro-internal/src/handler.rs:1863" }"
   - Location: crates/loro-internal/src/undo.rs:992
   
2. `undo_movable_list_0` (in fuzz::test)
   - Related to undo operations on movable lists

## Priority Recommendations

### High Priority
1. **Core Undo Tests** - These are fundamental to the undo functionality and affect user experience directly
2. **Fuzz Test `fast_snapshot_5`** - Contains a panic/crash that could affect stability

### Medium Priority
3. **Performance Test** - Important for validating the optimization work, but not blocking functionality
4. **Undo Consistency Test** - Important for data integrity but specific to movable lists

### Lower Priority
5. **Fuzz Test `undo_movable_list_0`** - Specific edge case found through fuzzing

## Root Cause Analysis

Based on the recent commits and the concentration of failures in undo-related tests, these failures are likely related to:

1. The recent undo optimization work that aims to avoid checkouts
2. The temporary disabling of undo optimization for grouped operations (commit c5399fd7)
3. Issues with the UndoManager's integration with the subscribe_undo_diffs feature

The `OutOfBound` error in `fast_snapshot_5` suggests there might be an issue with position tracking or buffer management in the undo system.