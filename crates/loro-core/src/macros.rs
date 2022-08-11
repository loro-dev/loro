/// ```no_run
/// use fxhash::FxHashMap;
/// use loro_core::fx_map;
///
/// let mut expected = FxHashMap::default();
/// expected.insert("test".to_string(), "test".to_string());
/// expected.insert("test2".to_string(), "test2".to_string());
/// let actual = fx_map!("test".into() => "test".into(), "test2".into() => "test2".into());
/// assert_eq!(expected, actual);
/// ```
#[macro_export]
macro_rules! fx_map {
    ($($key:expr => $value:expr),*) => {
        {
            let mut m = FxHashMap::default();
            $(
                m.insert($key, $value);
            )*
            m
        }
    };
}

#[macro_export]
macro_rules! unsafe_array_mut_ref {
    ($arr:expr, [$($idx:expr),*]) => {
        {
            unsafe {
                (
                    $(
                        {  &mut *(&mut $arr[$idx] as *mut _) }
                    ),*,
                )
            }
        }
    }
}

#[macro_export]
macro_rules! array_mut_ref {
    ($arr:expr, [$a0:expr, $a1:expr]) => {{
        #[inline]
        fn borrow_mut_ref<T>(arr: &mut [T], a0: usize, a1: usize) -> (&mut T, &mut T) {
            debug_assert!(a0 != a1);
            unsafe {
                (
                    &mut *(&mut arr[a0] as *mut _),
                    &mut *(&mut arr[a1] as *mut _),
                )
            }
        }

        borrow_mut_ref($arr, $a0, $a1)
    }};
    ($arr:expr, [$a0:expr, $a1:expr, $a2:expr]) => {{
        #[inline]
        fn borrow_mut_ref<T>(
            arr: &mut [T],
            a0: usize,
            a1: usize,
            a2: usize,
        ) -> (&mut T, &mut T, &mut T) {
            debug_assert!(a0 != a1 && a1 != a2 && a0 != a2);
            unsafe {
                (
                    &mut *(&mut arr[a0] as *mut _),
                    &mut *(&mut arr[a1] as *mut _),
                    &mut *(&mut arr[a2] as *mut _),
                )
            }
        }

        borrow_mut_ref($arr, $a0, $a1, $a2)
    }};
}

#[test]
fn test_macro() {
    let mut arr = vec![100, 101, 102, 103];
    let (a, b, _c) = array_mut_ref!(&mut arr, [1, 2, 3]);
    assert_eq!(*a, 101);
    assert_eq!(*b, 102);
    *a = 50;
    *b = 51;
    assert!(arr[1] == 50);
    assert!(arr[2] == 51);
}
