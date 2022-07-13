#![allow(dead_code, unused_imports)]
mod change;
mod id;
mod id_span;
mod op;
mod store;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
