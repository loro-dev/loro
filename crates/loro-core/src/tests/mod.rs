#![cfg(test)]

#[cfg(feature = "fuzzing")]
pub const PROPTEST_FACTOR_10: usize = 10;
#[cfg(not(feature = "fuzzing"))]
pub const PROPTEST_FACTOR_10: usize = 1;

#[cfg(feature = "fuzzing")]
pub const PROPTEST_FACTOR_1: usize = 1;
#[cfg(not(feature = "fuzzing"))]
pub const PROPTEST_FACTOR_1: usize = 0;
