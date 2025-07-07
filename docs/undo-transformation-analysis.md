# Undo Transformation Analysis

## Problem Summary

The undo optimization fails in collaborative scenarios because the pre-calculated undo diffs become invalid when remote operations change the document state. While basic transformation is implemented, it doesn't correctly handle all cases.

## Specific Test Case Analysis: `undo_redo_when_collab`

### Sequence of Operations

1. **Initial State**: Empty document
2. **Doc A Operations**:
   - Insert "Hello " at position 0 → "Hello "
   - Insert "World" at position 6 → "Hello World"
3. **Sync A→B**: Both have "Hello World"
4. **Doc B Operations**:
   - Delete 5 chars at position 0 (delete "Hello") → " World"
   - Insert "Hi" at position 0 → "Hi World"
5. **Doc A Operation**:
   - Insert "Alice" at position 0 → "AliceHello World"
6. **Sync A↔B**: Both see all changes
7. **Doc B Operation**:
   - Delete 5 chars at position 0 (delete "Alice") → "Hi World"
8. **First Undo on A**: Undo "Alice" insertion → "Hi World" ✓ (works)
9. **Second Undo on A**: Undo "World" insertion → Expected "Hi " but got "Hi Worl" ✗

### Debug Analysis

The undo diff for "World" insertion is:
- Original: `Retain 6, Delete 5` (at position 6, delete "World")

The remote changes from B are:
- `Retain 5, Delete 5, Insert "Hi"` (keep first 5, delete "Hello", insert "Hi")

After transformation:
- Result: `Retain 7, Delete 1` (at position 7, delete 1 character)

This is incorrect because:
1. The original operation wanted to delete "World" (5 characters)
2. After B's changes, "World" is still there but at a different position
3. The transformation incorrectly calculated the new position and length

## Root Cause

The current transformation algorithm in `delta_rope.rs` has limitations:

1. **Position-only transformation**: It adjusts positions but doesn't properly track what content is being operated on
2. **Lost context**: When operations are transformed, they lose track of what specific text they were meant to affect
3. **Complex interaction**: Delete operations that span regions affected by remote operations need special handling

## Specific Issues in Current Implementation

### Issue 1: Delete Length Calculation

When a local delete operation spans a region that was modified by remote operations, the delete length needs adjustment:

```rust
// Current behavior (simplified):
Local: Delete 5 at position 6 ("World")
Remote: Delete 5 at position 0, Insert 2 at position 0
Result: Delete 1 at position 7 // Wrong!

// Should be:
Result: Delete 5 at position 3 // Delete "World" at its new position
```

### Issue 2: Content Tracking

The current system tracks positions but not content:

```rust
// What we have:
Delete { position: 6, length: 5 }

// What we need:
Delete { position: 6, length: 5, content_hash: hash("World") }
```

### Issue 3: Overlapping Operations

When remote operations overlap with local operations, the transformation needs to:
1. Identify the overlap
2. Adjust both position and length
3. Handle partial overlaps

## Solution Approaches

### Approach 1: Enhanced Transformation (Current Attempt)

Improve the existing transformation logic to handle these cases:
- Track content being operated on
- Implement overlap detection
- Adjust lengths based on content analysis

**Pros**: Maintains performance benefits
**Cons**: Complex to implement correctly

### Approach 2: Hybrid Approach

Use optimization for simple cases, fall back for complex ones:
- Detect scenarios that need complex transformation
- Use checkouts for those cases
- Keep optimization for simple single-user edits

**Pros**: Balanced approach
**Cons**: Need to detect complex cases reliably

### Approach 3: Content-Based Diffs

Store more information in undo diffs:
- Include content checksums
- Store operation intent (what text to affect)
- Use this for accurate transformation

**Pros**: Most accurate
**Cons**: Increases storage overhead

## Implementation Challenges

1. **Performance**: Transformation must remain fast
2. **Correctness**: Must handle all edge cases
3. **Storage**: Minimize additional data stored
4. **Compatibility**: Work with existing CRDT operations

## Next Steps

1. **Immediate**: Keep optimization disabled for collaborative scenarios (current fix)
2. **Short-term**: Implement detection of complex cases for hybrid approach
3. **Long-term**: Design content-aware transformation system

## Test Cases Needed

1. Overlapping deletes
2. Insert in deleted region
3. Delete spanning multiple remote changes
4. Concurrent formatting changes
5. Move operations (for movable list)