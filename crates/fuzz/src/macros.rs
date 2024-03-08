#[macro_export]
macro_rules! array_mut_ref {
    ($arr:expr, [$a0:expr, $a1:expr]) => {{
        #[inline]
        fn borrow_mut_ref<T>(arr: &mut [T], a0: usize, a1: usize) -> (&mut T, &mut T) {
            assert!(a0 != a1);
            // SAFETY: this is safe because we know a0 != a1
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
            assert!(a0 != a1 && a1 != a2 && a0 != a2);
            // SAFETY: this is safe because we know there are not multiple mutable references to the same element
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
