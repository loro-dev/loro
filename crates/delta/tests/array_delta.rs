use loro_delta::{array_vec::ArrayVec, DeltaRope, DeltaRopeBuilder};

type TestArrayDelta = DeltaRope<ArrayVec<i32, 10>, ()>;

#[test]
fn delete_eq() {
    let a: TestArrayDelta = DeltaRopeBuilder::new().delete(5).build();
    let b = DeltaRopeBuilder::new().delete(10).build();
    assert_ne!(a, b);
}

#[test]
fn test_delete() {
    let mut a: TestArrayDelta = DeltaRopeBuilder::new()
        .insert(ArrayVec::from([1, 2, 3, 4]), ())
        .build();
    let b = DeltaRopeBuilder::new().retain(1, ()).delete(1).build();

    let expected: TestArrayDelta = DeltaRopeBuilder::new()
        .insert(ArrayVec::from([1]), ())
        .build();
    assert_eq!(a.len(), 4);
    a.compose(&b);
    assert_eq!(a.len(), 3);
    a.compose(&b);
    assert_eq!(a.len(), 2);
    a.compose(&b);
    assert_eq!(a.len(), 1);
    assert_eq!(a, expected);
}

#[test]
fn test_basic_deletion() {
    let mut delta: TestArrayDelta = DeltaRopeBuilder::new()
        .insert(ArrayVec::from([1, 2, 3, 4, 5]), ())
        .build();
    let delete_op = DeltaRopeBuilder::new().delete(2).build();
    delta.compose(&delete_op);
    let expected: TestArrayDelta = DeltaRopeBuilder::new()
        .insert(ArrayVec::from([3, 4, 5]), ())
        .build();
    assert_eq!(delta, expected);
}

#[test]
fn test_composition_of_operations() {
    let mut delta: TestArrayDelta = DeltaRopeBuilder::new()
        .insert(ArrayVec::from([1, 2, 3]), ())
        .build();
    let ops = DeltaRopeBuilder::new()
        .retain(1, ())
        .delete(1)
        .insert(ArrayVec::from([4, 5]), ())
        .build();
    delta.compose(&ops);
    let expected: TestArrayDelta = DeltaRopeBuilder::new()
        .insert(ArrayVec::from([1, 4, 5, 3]), ())
        .build();
    assert_eq!(delta, expected);
}

#[test]
fn test_complex_composition() {
    let mut delta: TestArrayDelta = DeltaRopeBuilder::new()
        .insert(ArrayVec::from([1, 2, 3, 4]), ())
        .build();
    let ops1 = DeltaRopeBuilder::new().retain(1, ()).delete(1).build();
    let ops2 = DeltaRopeBuilder::new()
        .retain(1, ())
        .insert(ArrayVec::from([5, 6]), ())
        .build();
    delta.compose(&ops1);
    delta.compose(&ops2);
    let expected: TestArrayDelta = DeltaRopeBuilder::new()
        .insert(ArrayVec::from([1, 5, 6, 3, 4]), ())
        .build();
    assert_eq!(delta, expected);
}

#[test]
fn test_retain_operation() {
    let delta: TestArrayDelta = DeltaRopeBuilder::new()
        .insert(ArrayVec::from([1, 2, 3, 4]), ())
        .build();
    let expected: TestArrayDelta = DeltaRopeBuilder::new()
        .insert(ArrayVec::from([1, 2, 3, 4]), ())
        .build();
    assert_eq!(delta, expected);
}

#[test]
fn test_edge_cases_insertion() {
    let mut delta: TestArrayDelta = DeltaRopeBuilder::new()
        .insert(ArrayVec::from([2, 3]), ())
        .build();
    let insert_begin = DeltaRopeBuilder::new()
        .insert(ArrayVec::from([1]), ())
        .build();
    let insert_end = DeltaRopeBuilder::new()
        .retain(3, ())
        .insert(ArrayVec::from([4]), ())
        .build();
    delta.compose(&insert_begin);
    delta.compose(&insert_end);
    let expected: TestArrayDelta = DeltaRopeBuilder::new()
        .insert(ArrayVec::from([1, 2, 3, 4]), ())
        .build();
    assert_eq!(delta, expected);
}

// Test case to verify behavior when attempting to insert beyond capacity
// It should split the insertion into multiple parts to avoid overflow.
#[test]
fn test_insert_overflow_handled() {
    let mut delta: TestArrayDelta = DeltaRopeBuilder::new()
        .insert(ArrayVec::from([1, 2, 3, 4, 5, 6, 7, 8, 9]), ())
        .build();
    // Attempt to insert more elements than the capacity allows, expecting it to handle gracefully
    let overflow_op = DeltaRopeBuilder::new()
        .insert(ArrayVec::from([10, 11]), ())
        .build();
    delta.compose(&overflow_op);

    // Expected behavior: split the insertion to avoid overflow
    let expected: TestArrayDelta = DeltaRopeBuilder::new()
        .insert(ArrayVec::from([10, 11]), ())
        .insert(ArrayVec::from([1, 2, 3, 4, 5, 6, 7, 8, 9]), ())
        .build();
    assert_eq!(delta, expected);
}

// Test case to verify behavior when multiple operations lead to overflow
// It should handle the overflow by creating new insertions as needed.
#[test]
fn test_cumulative_insert_overflow_handled() {
    let mut delta: TestArrayDelta = DeltaRopeBuilder::new()
        .insert(ArrayVec::from([1, 2, 3, 4, 5]), ())
        .build();
    // First operation that fits within capacity
    let op1 = DeltaRopeBuilder::new()
        .retain(5, ())
        .insert(ArrayVec::from([6, 7, 8]), ())
        .build();
    // Second operation that would normally cause overflow
    let op2 = DeltaRopeBuilder::new()
        .retain(8, ())
        .insert(ArrayVec::from([9, 10, 11]), ())
        .build();
    delta.compose(&op1);
    delta.compose(&op2); // Expect it to handle overflow gracefully

    // Expected behavior: handle overflow by creating new insertions
    let expected: TestArrayDelta = DeltaRopeBuilder::new()
        .insert(ArrayVec::from([1, 2, 3, 4, 5, 6, 7, 8, 9]), ())
        .insert(ArrayVec::from([10, 11]), ())
        .build();
    assert_eq!(delta, expected);
}

// Test case for handling deletion followed by insertion that exceeds capacity
// It should correctly handle the overflow by creating a new insertion.
#[test]
fn test_delete_then_insert_overflow_handled() {
    let mut delta: TestArrayDelta = DeltaRopeBuilder::new()
        .insert(ArrayVec::from([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]), ())
        .build();
    // Delete some elements
    let delete_op = DeltaRopeBuilder::new().retain(5, ()).delete(5).build();
    // Attempt to insert more elements than the remaining capacity allows
    let insert_op = DeltaRopeBuilder::new()
        .retain(5, ())
        .insert(ArrayVec::from([11, 12, 13, 14, 15, 16]), ())
        .build();
    delta.compose(&delete_op);
    delta.compose(&insert_op); // Expect it to handle overflow gracefully

    // Expected behavior: handle overflow by creating new insertions
    let expected: TestArrayDelta = DeltaRopeBuilder::new()
        .insert(ArrayVec::from([1, 2, 3, 4, 5]), ())
        .insert(ArrayVec::from([11, 12, 13, 14, 15, 16]), ())
        .build();
    assert_eq!(delta, expected);
}
