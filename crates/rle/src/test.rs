#![cfg(test)]

#[cfg(proptest)]
pub const PROPTEST_FACTOR_10: usize = 10;
#[cfg(not(proptest))]
pub const PROPTEST_FACTOR_10: usize = 1;

#[cfg(proptest)]
pub const PROPTEST_FACTOR_1: usize = 1;
#[cfg(not(proptest))]
pub const PROPTEST_FACTOR_1: usize = 0;
