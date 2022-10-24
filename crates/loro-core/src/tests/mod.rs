#![cfg(test)]

#[cfg(feature = "proptest")]
pub const PROPTEST_FACTOR_10: usize = 10;
#[cfg(not(feature = "proptest"))]
pub const PROPTEST_FACTOR_10: usize = 1;

#[cfg(feature = "proptest")]
pub const PROPTEST_FACTOR_1: usize = 1;
#[cfg(not(feature = "proptest"))]
pub const PROPTEST_FACTOR_1: usize = 0;
