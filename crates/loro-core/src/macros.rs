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
