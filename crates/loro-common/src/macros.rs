/// A macro for creating `LoroValue`. It works just like the `json!` macro in
/// `serde_json`.
///
/// # Example
///
/// ```
/// use loro_common::loro_value;
/// loro_value!({
///     "name": "John",
///     "age": 12,
///     "collections": [
///         {
///             "binary-data": b"1,2,3"
///         }
///     ],
///     "float": 1.2,
///     "null": null,
///     "bool": true,
/// });
/// ```
///
#[macro_export(local_inner_macros)]
macro_rules! loro_value {
    // Hide distracting implementation details from the generated rustdoc.
    ($($json:tt)+) => {
        value_internal!($($json)+)
    };
}

// Rocket relies on this because they export their own `json!` with a different
// doc comment than ours, and various Rust bugs prevent them from calling our
// `json!` from their `json!` so they call `value_internal!` directly. Check with
// @SergioBenitez before making breaking changes to this macro.
//
// Changes are fine as long as `value_internal!` does not call any new helper
// macros and can still be invoked as `value_internal!($($json)+)`.
#[macro_export(local_inner_macros)]
#[doc(hidden)]
macro_rules! value_internal {
    //////////////////////////////////////////////////////////////////////////
    // TT muncher for parsing the inside of an array [...]. Produces a vec![...]
    // of the elements.
    //
    // Must be invoked as: value_internal!(@array [] $($tt)*)
    //////////////////////////////////////////////////////////////////////////

    // Done with trailing comma.
    (@array [$($elems:expr,)*]) => {
        json_internal_vec![$($elems,)*]
    };

    // Done without trailing comma.
    (@array [$($elems:expr),*]) => {
        json_internal_vec![$($elems),*]
    };

    // Next element is `null`.
    (@array [$($elems:expr,)*] null $($rest:tt)*) => {
        value_internal!(@array [$($elems,)* value_internal!(null)] $($rest)*)
    };

    // Next element is `true`.
    (@array [$($elems:expr,)*] true $($rest:tt)*) => {
        value_internal!(@array [$($elems,)* value_internal!(true)] $($rest)*)
    };

    // Next element is `false`.
    (@array [$($elems:expr,)*] false $($rest:tt)*) => {
        value_internal!(@array [$($elems,)* value_internal!(false)] $($rest)*)
    };

    // Next element is an array.
    (@array [$($elems:expr,)*] [$($array:tt)*] $($rest:tt)*) => {
        value_internal!(@array [$($elems,)* value_internal!([$($array)*])] $($rest)*)
    };

    // Next element is a map.
    (@array [$($elems:expr,)*] {$($map:tt)*} $($rest:tt)*) => {
        value_internal!(@array [$($elems,)* value_internal!({$($map)*})] $($rest)*)
    };

    // Next element is an expression followed by comma.
    (@array [$($elems:expr,)*] $next:expr, $($rest:tt)*) => {
        value_internal!(@array [$($elems,)* value_internal!($next),] $($rest)*)
    };

    // Last element is an expression with no trailing comma.
    (@array [$($elems:expr,)*] $last:expr) => {
        value_internal!(@array [$($elems,)* value_internal!($last)])
    };

    // Comma after the most recent element.
    (@array [$($elems:expr),*] , $($rest:tt)*) => {
        value_internal!(@array [$($elems,)*] $($rest)*)
    };

    // Unexpected token after most recent element.
    (@array [$($elems:expr),*] $unexpected:tt $($rest:tt)*) => {
        json_unexpected!($unexpected)
    };

    //////////////////////////////////////////////////////////////////////////
    // TT muncher for parsing the inside of an object {...}. Each entry is
    // inserted into the given map variable.
    //
    // Must be invoked as: value_internal!(@object $map () ($($tt)*) ($($tt)*))
    //
    // We require two copies of the input tokens so that we can match on one
    // copy and trigger errors on the other copy.
    //////////////////////////////////////////////////////////////////////////

    // Done.
    (@object $object:ident () () ()) => {};

    // Insert the current entry followed by trailing comma.
    (@object $object:ident [$($key:tt)+] ($value:expr) , $($rest:tt)*) => {
        let _ = $object.insert(($($key)+).into(), $value);
        value_internal!(@object $object () ($($rest)*) ($($rest)*));
    };

    // Current entry followed by unexpected token.
    (@object $object:ident [$($key:tt)+] ($value:expr) $unexpected:tt $($rest:tt)*) => {
        json_unexpected!($unexpected);
    };

    // Insert the last entry without trailing comma.
    (@object $object:ident [$($key:tt)+] ($value:expr)) => {
        let _ = $object.insert(($($key)+).into(), $value);
    };

    // Next value is `null`.
    (@object $object:ident ($($key:tt)+) (: null $($rest:tt)*) $copy:tt) => {
        value_internal!(@object $object [$($key)+] (value_internal!(null)) $($rest)*);
    };

    // Next value is `true`.
    (@object $object:ident ($($key:tt)+) (: true $($rest:tt)*) $copy:tt) => {
        value_internal!(@object $object [$($key)+] (value_internal!(true)) $($rest)*);
    };

    // Next value is `false`.
    (@object $object:ident ($($key:tt)+) (: false $($rest:tt)*) $copy:tt) => {
        value_internal!(@object $object [$($key)+] (value_internal!(false)) $($rest)*);
    };

    // Next value is an array.
    (@object $object:ident ($($key:tt)+) (: [$($array:tt)*] $($rest:tt)*) $copy:tt) => {
        value_internal!(@object $object [$($key)+] (value_internal!([$($array)*])) $($rest)*);
    };

    // Next value is a map.
    (@object $object:ident ($($key:tt)+) (: {$($map:tt)*} $($rest:tt)*) $copy:tt) => {
        value_internal!(@object $object [$($key)+] (value_internal!({$($map)*})) $($rest)*);
    };

    // Next value is an expression followed by comma.
    (@object $object:ident ($($key:tt)+) (: $value:expr , $($rest:tt)*) $copy:tt) => {
        value_internal!(@object $object [$($key)+] (value_internal!($value)) , $($rest)*);
    };

    // Last value is an expression with no trailing comma.
    (@object $object:ident ($($key:tt)+) (: $value:expr) $copy:tt) => {
        value_internal!(@object $object [$($key)+] (value_internal!($value)));
    };

    // Missing value for last entry. Trigger a reasonable error message.
    (@object $object:ident ($($key:tt)+) (:) $copy:tt) => {
        // "unexpected end of macro invocation"
        value_internal!();
    };

    // Missing colon and value for last entry. Trigger a reasonable error
    // message.
    (@object $object:ident ($($key:tt)+) () $copy:tt) => {
        // "unexpected end of macro invocation"
        value_internal!();
    };

    // Misplaced colon. Trigger a reasonable error message.
    (@object $object:ident () (: $($rest:tt)*) ($colon:tt $($copy:tt)*)) => {
        // Takes no arguments so "no rules expected the token `:`".
        json_unexpected!($colon);
    };

    // Found a comma inside a key. Trigger a reasonable error message.
    (@object $object:ident ($($key:tt)*) (, $($rest:tt)*) ($comma:tt $($copy:tt)*)) => {
        // Takes no arguments so "no rules expected the token `,`".
        json_unexpected!($comma);
    };

    // Key is fully parenthesized. This avoids clippy double_parens false
    // positives because the parenthesization may be necessary here.
    (@object $object:ident () (($key:expr) : $($rest:tt)*) $copy:tt) => {
        value_internal!(@object $object ($key) (: $($rest)*) (: $($rest)*));
    };

    // Refuse to absorb colon token into key expression.
    (@object $object:ident ($($key:tt)*) (: $($unexpected:tt)+) $copy:tt) => {
        json_expect_expr_comma!($($unexpected)+);
    };

    // Munch a token into the current key.
    (@object $object:ident ($($key:tt)*) ($tt:tt $($rest:tt)*) $copy:tt) => {
        value_internal!(@object $object ($($key)* $tt) ($($rest)*) ($($rest)*));
    };

    //////////////////////////////////////////////////////////////////////////
    // The main implementation.
    //
    // Must be invoked as: value_internal!($($json)+)
    //////////////////////////////////////////////////////////////////////////

    (null) => {
        $crate::LoroValue::Null
    };

    (true) => {
        $crate::LoroValue::Bool(true)
    };

    (false) => {
        $crate::LoroValue::Bool(false)
    };

    ([]) => {
        $crate::LoroValue::List(std::sync::Arc::new(json_internal_vec![]))
    };

    ([ $($tt:tt)+ ]) => {
        $crate::LoroValue::List(std::sync::Arc::new(value_internal!(@array [] $($tt)+)))
    };

    ({}) => {
        $crate::LoroValue::Map(std::sync::Arc::new(Default::default()))
    };

    ({ $($tt:tt)+ }) => {
        ({
            let mut object = $crate::FxHashMap::default();
            value_internal!(@object object () ($($tt)+) ($($tt)+));
            $crate::LoroValue::Map(std::sync::Arc::new(object))
        })
    };

    // Any Serialize type: numbers, strings, struct literals, variables etc.
    // Must be below every other rule.
    ($other:expr) => {
        $crate::to_value($other)
    };
}

#[macro_export]
#[doc(hidden)]
macro_rules! json_unexpected {
    () => {};
}

// The json_internal macro above cannot invoke vec directly because it uses
// local_inner_macros. A vec invocation there would resolve to $crate::vec.
// Instead invoke vec here outside of local_inner_macros.
#[macro_export]
#[doc(hidden)]
macro_rules! json_internal_vec {
    ($($content:tt)*) => {
        vec![$($content)*]
    };
}

#[cfg(test)]
mod test {
    #[test]
    fn test_value_macro() {
        let v = loro_value!([1, 2, 3]);
        let list = v.into_list().unwrap();
        assert_eq!(&*list, &[1.into(), 2.into(), 3.into()]);

        let map = loro_value!({
            "hi": true,
            "false": false,
            "null": null,
            "list": [],
            "integer": 123,
            "float": 123.123,
            "map": {
                "a": "1"
            },
            "binary": b"123",
        });

        let map = map.into_map().unwrap();
        assert_eq!(map.len(), 8);
        assert!(*map.get("hi").unwrap().as_bool().unwrap());
        assert!(!(*map.get("false").unwrap().as_bool().unwrap()));
        assert!(map.get("null").unwrap().is_null());
        assert_eq!(map.get("list").unwrap().as_list().unwrap().len(), 0);
        assert_eq!(*map.get("integer").unwrap().as_i64().unwrap(), 123);
        assert_eq!(*map.get("float").unwrap().as_double().unwrap(), 123.123);
        assert_eq!(map.get("map").unwrap().as_map().unwrap().len(), 1);
        assert_eq!(
            &**map
                .get("map")
                .unwrap()
                .as_map()
                .unwrap()
                .get("a")
                .unwrap()
                .as_string()
                .unwrap(),
            "1"
        );
        assert_eq!(
            &**map.get("binary").unwrap().as_binary().unwrap(),
            &b"123".to_vec()
        );
    }
}
