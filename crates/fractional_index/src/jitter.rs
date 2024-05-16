use std::sync::Arc;

use crate::{new_after, new_before, new_between, FractionalIndex, TERMINATOR};
use rand::Rng;

impl FractionalIndex {
    pub fn jitter_default(rng: &mut impl Rng, jitter: u8) -> Self {
        Self::jitter(Vec::new(), rng, jitter)
    }

    pub fn new_jitter<R: Rng>(
        lower: Option<&FractionalIndex>,
        upper: Option<&FractionalIndex>,
        rng: &mut R,
        jitter: u8,
    ) -> Option<Self> {
        match (lower, upper) {
            (Some(lower), Some(upper)) => Self::new_between_jitter(lower, upper, rng, jitter),
            (Some(lower), None) => Self::new_after_jitter(lower, rng, jitter).into(),
            (None, Some(upper)) => Self::new_before_jitter(upper, rng, jitter).into(),
            (None, None) => FractionalIndex::jitter_default(rng, jitter).into(),
        }
    }

    fn jitter<R: Rng>(mut bytes: Vec<u8>, rng: &mut R, jitter: u8) -> FractionalIndex {
        bytes.push(TERMINATOR);
        bytes.extend((0..jitter).map(|_| rng.gen::<u8>()));
        FractionalIndex(Arc::new(bytes))
    }

    pub fn new_before_jitter<R: Rng>(
        FractionalIndex(bytes): &FractionalIndex,
        rng: &mut R,
        jitter: u8,
    ) -> Self {
        Self::jitter(new_before(bytes), rng, jitter)
    }

    pub fn new_after_jitter<R: Rng>(
        FractionalIndex(bytes): &FractionalIndex,
        rng: &mut R,
        jitter: u8,
    ) -> Self {
        Self::jitter(new_after(bytes), rng, jitter)
    }

    pub fn new_between_jitter<R: Rng>(
        FractionalIndex(left): &FractionalIndex,
        FractionalIndex(right): &FractionalIndex,
        rng: &mut R,
        jitter: u8,
    ) -> Option<Self> {
        new_between(left, right, jitter as usize + 1).map(|x| Self::jitter(x, rng, jitter))
    }

    pub fn generate_n_evenly_jitter<R: Rng>(
        lower: Option<&FractionalIndex>,
        upper: Option<&FractionalIndex>,
        n: usize,
        rng: &mut R,
        jitter: u8,
    ) -> Option<Vec<Self>> {
        fn gen(
            lower: Option<&FractionalIndex>,
            upper: Option<&FractionalIndex>,
            n: usize,
            push: &mut impl FnMut(FractionalIndex),
            rng: &mut impl Rng,
            jitter: u8,
        ) {
            if n == 0 {
                return;
            }

            let mid = n / 2;
            let mid_ans = FractionalIndex::new_jitter(lower, upper, rng, jitter).unwrap();
            if n == 1 {
                push(mid_ans);
                return;
            }

            gen(lower, Some(&mid_ans), mid, push, rng, jitter);
            push(mid_ans.clone());
            if n - mid - 1 == 0 {
                return;
            }
            gen(Some(&mid_ans), upper, n - mid - 1, push, rng, jitter);
        }

        if n == 0 {
            return Some(Vec::new());
        }

        match (lower, upper) {
            (Some(a), Some(b)) if a >= b => return None,
            _ => {}
        }

        let mut ans = Vec::with_capacity(n);
        gen(lower, upper, n, &mut |v| ans.push(v), rng, jitter);
        Some(ans)
    }
}
