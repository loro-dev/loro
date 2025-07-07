# Undo System Operation Transformation Design

## Problem Statement

The current undo optimization stores pre-calculated inverse operations (undo diffs) to avoid expensive checkouts. However, this optimization breaks in collaborative scenarios because:

1. **Remote operations change document state**: Pre-calculated diffs assume a specific document state that gets invalidated by remote changes
2. **Operation transformation is needed**: The stored diffs need to be transformed against remote operations
3. **Complex interactions**: Grouped operations, excluded origins, and collaborative edits create complex scenarios

## Current Limitations

### 1. Position Invalidation
When a remote user inserts/deletes text, pre-calculated positions in undo diffs become incorrect:
```
Local: Insert "Hello" at position 0
Remote: Insert "Hi " at position 0
Undo diff: Delete at position 0, length 5 (incorrect - should be position 3)
```

### 2. Container State Changes
Remote operations can modify container contents that undo diffs reference:
```
Local: Set map key "foo" to "bar"
Remote: Delete key "foo"
Undo diff: Set key "foo" to old value (key no longer exists)
```

### 3. Grouped Operations
Composed diffs from grouped operations may have interdependencies broken by remote changes.

## Proposed Solution

### Core Principle: Transform Undo Diffs Against Remote Operations

Instead of disabling optimization when remote changes occur, transform the pre-calculated undo diffs to maintain correctness.

### Implementation Strategy

#### 1. Track Remote Operations Per Undo Entry
```rust
struct StackItem {
    id_span: IdSpan,
    undo_diff: DiffBatch,
    redo_diff: DiffBatch,
    remote_diff: DiffBatch,
    // New field: track remote ops that occurred after this entry
    subsequent_remote_ops: Vec<RemoteOp>,
}
```

#### 2. Transform Undo Diffs Before Application
```rust
impl UndoManager {
    fn transform_undo_diff(&self, 
        mut undo_diff: DiffBatch, 
        remote_ops: &[RemoteOp]
    ) -> DiffBatch {
        // Transform the undo diff against each remote operation
        for remote_op in remote_ops {
            match &remote_op.container_type {
                ContainerType::Text | ContainerType::List => {
                    // Position-based transformation
                    undo_diff.transform_positions(remote_op);
                }
                ContainerType::Map => {
                    // Key-based transformation
                    undo_diff.transform_keys(remote_op);
                }
                ContainerType::Tree => {
                    // Tree-specific transformation
                    undo_diff.transform_tree(remote_op);
                }
            }
        }
        undo_diff
    }
}
```

#### 3. Enhanced DiffBatch Transformation
```rust
impl DiffBatch {
    pub fn transform_positions(&mut self, remote_op: &RemoteOp) {
        if let Some(diff) = self.cid_to_events.get_mut(&remote_op.container_id) {
            match diff {
                Diff::List(list_diff) => {
                    // Transform list operations
                    for item in list_diff.iter_mut() {
                        match item {
                            DeltaItem::Insert { pos, .. } => {
                                *pos = transform_position(*pos, remote_op);
                            }
                            DeltaItem::Delete { pos, len } => {
                                (*pos, *len) = transform_delete_range(*pos, *len, remote_op);
                            }
                        }
                    }
                }
                Diff::Text(text_diff) => {
                    // Similar transformation for text
                }
                // ... other container types
            }
        }
    }
}
```

#### 4. Position Transformation Logic
```rust
fn transform_position(local_pos: usize, remote_op: &RemoteOp) -> usize {
    match remote_op.action {
        OpAction::Insert { pos, len } => {
            if remote_op.pos <= local_pos {
                // Remote insert before local position
                local_pos + remote_op.len
            } else {
                local_pos
            }
        }
        OpAction::Delete { pos, len } => {
            if remote_op.pos + remote_op.len <= local_pos {
                // Remote delete before local position
                local_pos - remote_op.len
            } else if remote_op.pos < local_pos {
                // Remote delete spans local position
                remote_op.pos
            } else {
                local_pos
            }
        }
    }
}
```

#### 5. Grouped Operations Handling
```rust
impl UndoManager {
    fn compose_and_transform_grouped(&self, 
        spans: Vec<StackItem>, 
        remote_ops: &[RemoteOp]
    ) -> DiffBatch {
        let mut composed = DiffBatch::default();
        
        // Compose in order, transforming each against accumulated remote ops
        for (i, span) in spans.iter().enumerate() {
            let mut diff = span.undo_diff.clone();
            
            // Transform against remote ops that occurred after this span
            let relevant_remote_ops = &remote_ops[span.remote_op_index..];
            diff = self.transform_undo_diff(diff, relevant_remote_ops);
            
            // Compose with previous diffs
            composed.compose(&diff);
        }
        
        composed
    }
}
```

### Special Cases

#### 1. Deleted Containers
If a remote operation deletes a container that the undo diff references:
- Mark the container operations as no-ops
- Track container recreation if needed

#### 2. Conflicting Map Keys
If remote operations modify the same map keys:
- Use last-write-wins semantics
- Transform based on operation timestamps

#### 3. Tree Operations
Tree moves require special handling:
- Track parent-child relationships
- Ensure moves don't create cycles
- Handle orphaned nodes

### Performance Considerations

1. **Lazy Transformation**: Only transform diffs when actually performing undo
2. **Batch Remote Ops**: Group consecutive remote operations for efficient transformation
3. **Cache Transformed Diffs**: Store transformed versions to avoid repeated computation

### Testing Strategy

1. **Unit Tests**: Test individual transformation functions
2. **Integration Tests**: Test complete undo/redo flows with remote operations
3. **Property Tests**: Use property-based testing for transformation correctness
4. **Fuzz Tests**: Ensure robustness with random operation sequences

### Migration Path

1. **Phase 1**: Implement basic position transformation for Text/List
2. **Phase 2**: Add Map and Tree transformation
3. **Phase 3**: Handle grouped operations
4. **Phase 4**: Optimize performance with caching

## Benefits

1. **Maintains Performance**: Keeps O(n) complexity for undo operations
2. **Correct Collaboration**: Properly handles all collaborative editing scenarios
3. **Extensible**: Easy to add new container types or transformation rules
4. **Predictable**: Users see expected results even with concurrent edits

## Implementation Timeline

- Week 1: Core transformation infrastructure
- Week 2: Text and List containers
- Week 3: Map and Tree containers
- Week 4: Testing and optimization