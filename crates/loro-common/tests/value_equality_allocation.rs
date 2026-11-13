use loro_common::{ContainerID, ContainerType, LoroValue};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    hint::black_box,
};

struct CountAllocations;

thread_local! {
    // Only count the current test thread, excluding the test runner and other tests.
    static ALLOCATIONS: Cell<Option<usize>> = const { Cell::new(None) };
}

fn record_allocation() {
    let _ = ALLOCATIONS.try_with(|count| {
        if let Some(n) = count.get() {
            count.set(Some(n + 1));
        }
    });
}

unsafe impl GlobalAlloc for CountAllocations {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc(layout);
        if !ptr.is_null() {
            record_allocation();
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let result = System.realloc(ptr, layout, size);
        if !result.is_null() {
            record_allocation();
        }
        result
    }
}

#[global_allocator]
static ALLOCATOR: CountAllocations = CountAllocations;

fn compare_and_count(a: &LoroValue, b: &LoroValue) -> (bool, usize) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            ALLOCATIONS.with(|count| count.set(None));
        }
    }
    ALLOCATIONS.with(|count| assert_eq!(count.replace(Some(0)), None));
    let _reset = Reset;
    let equal = black_box(a) == black_box(b);
    (equal, ALLOCATIONS.with(|count| count.get().unwrap()))
}

#[test]
fn scalar_equality_does_not_allocate() {
    let values = [
        LoroValue::Null,
        true.into(),
        false.into(),
        LoroValue::I64(0),
        LoroValue::I64(i64::MAX),
        LoroValue::Double(0.0),
        LoroValue::Double(-0.0),
        LoroValue::Double(f64::NAN),
        LoroValue::Double(f64::from_bits(0x7ff8000000000001)),
        LoroValue::Double(f64::INFINITY),
        "same".into(),
        "different".into(),
        LoroValue::Binary(vec![0, 255].into()),
        LoroValue::Binary(vec![0, 254].into()),
        LoroValue::Container(ContainerID::new_root("text", ContainerType::Text)),
        LoroValue::Container(ContainerID::new_root("map", ContainerType::Map)),
    ];
    for a in &values {
        // Independently construct the strings/buffers to avoid only testing Arc identity.
        let b = match a {
            LoroValue::String(s) => LoroValue::String(s.to_string().into()),
            LoroValue::Binary(b) => LoroValue::Binary(b.to_vec().into()),
            _ => a.clone(),
        };
        let (equal, allocations) = compare_and_count(a, &b);
        assert!(equal);
        assert_eq!(allocations, 0, "scalar equality allocated for {a:?}");
    }
    for (i, a) in values.iter().enumerate() {
        for b in &values[i + 1..] {
            let (equal, allocations) = compare_and_count(a, b);
            let expected = matches!((a, b), (LoroValue::Double(x), LoroValue::Double(y)) if (x.is_nan() && y.is_nan()) || x == y);
            assert_eq!(equal, expected, "{a:?} vs {b:?}");
            assert_eq!(
                allocations, 0,
                "scalar comparison allocated: {a:?} vs {b:?}"
            );
        }
    }
}

#[test]
fn wide_collections_keep_equality_and_early_return_semantics() {
    let make = |last| {
        LoroValue::List(
            (0..64)
                .map(|i| {
                    LoroValue::Map(
                        vec![(
                            "value".into(),
                            LoroValue::I64(if i == 63 { last } else { i }),
                        )]
                        .into(),
                    )
                })
                .collect::<Vec<_>>()
                .into(),
        )
    };
    let a = make(63);
    assert_eq!(a, make(63));
    assert_eq!(a, a.clone());
    assert_ne!(a, make(-1));
    let map = |key: &str| LoroValue::Map(vec![(key.into(), a.clone())].into());
    assert_eq!(map("items"), map("items"));
    assert_ne!(map("items"), map("other"));
}
