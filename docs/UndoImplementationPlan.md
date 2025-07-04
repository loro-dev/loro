# Undo Performance Optimization Implementation Plan

## Overview
This document tracks the implementation progress of the undo performance optimization for Loro. The goal is to pre-calculate undo diffs during operation application to avoid expensive checkout operations during undo/redo.

## Current Status

### Completed ✅
- Added `undo_diff` field to Transaction
- Added `undo_subs` emission in transaction commit  
- Updated `apply_local_op` signatures with `undo_diff` parameter
- Started implementations for Counter and Map states (with TODOs)

### In Progress 🚧
- Container ID access issue in `apply_local_op` implementations

### Not Started ❌
- Complete undo diff generation for all container types
- Comprehensive test suite
- UndoManager modification

## Implementation Steps

### Step 1: Fix Container ID Access Issue ✅
**Status**: Completed  
**Priority**: High  
**Problem**: Current implementations couldn't convert ContainerIdx to ContainerID due to lock ordering violations when accessing the arena.

**Solution Implemented**:
Used the `doc` parameter (Weak<LoroDocInner>) that's already passed to `apply_local_op`. By upgrading the weak reference, we can access the arena and convert ContainerIdx to ContainerID.

**Tasks**:
- [x] Analyze the call chain to understand where container ID is available
- [x] Implement solution to make container ID accessible in `apply_local_op`
- [x] Update Counter and Map implementations to use the solution
- [x] Verify no lock ordering violations

### Step 2: Complete Undo Diff Generation 🚧
**Status**: In Progress  
**Priority**: High  

**Container Implementations**:
- [x] **CounterState**: Generate diff for increment/decrement operations ✅
- [x] **MapState**: Generate diff for insert/update/delete operations ✅
- [ ] **ListState**: Generate diff for insert/delete/move operations
- [ ] **TreeState**: Generate diff for node creation/deletion/movement
- [ ] **RichTextState**: Generate diff for text insertion/deletion and style changes
- [ ] **MovableListState**: Generate diff similar to List with move support
- [ ] **UnknownState**: Handle unknown container types appropriately

### Step 3: Write Comprehensive Tests ❌
**Status**: Not Started  
**Priority**: High  

**Test Coverage**:
- [ ] Unit tests for each container type's undo diff generation
- [ ] Verify applying undo diff restores previous state
- [ ] Test edge cases (empty containers, batch operations)
- [ ] Test complex scenarios (nested containers, concurrent operations)
- [ ] Add tests in `crates/loro-internal/tests/undo.rs`

### Step 4: Modify UndoManager ❌
**Status**: Not Started  
**Priority**: High  

**Tasks**:
- [ ] Change undo/redo stack from storing IdSpan to DiffBatch
- [ ] Update undo logic to apply diffs directly instead of checkout
- [ ] Update redo logic similarly
- [ ] Handle subscription to collect undo diffs
- [ ] Maintain backward compatibility or migration path

### Step 5: Behavior Consistency Tests ❌
**Status**: Not Started  
**Priority**: Medium  

**Tasks**:
- [ ] Create tests comparing old and new undo behavior
- [ ] Ensure identical results for same operations
- [ ] Performance benchmarks to verify improvement
- [ ] Consider adding fuzzing tests

## Technical Challenges

### Container ID Access
The main blocker is accessing container IDs in `apply_local_op`. The arena that contains the mapping is locked during state operations, causing potential deadlocks.

### Diff Generation Complexity
Each container type has different semantics for undo:
- Map: Need to store previous values for updates
- List/MovableList: Need to track positions for moves
- Tree: Need to maintain parent-child relationships
- RichText: Need to handle both text and formatting

## Next Actions
1. Start with analyzing the container ID access issue
2. Find a clean solution that doesn't violate lock ordering
3. Implement the solution for Counter and Map first as proof of concept