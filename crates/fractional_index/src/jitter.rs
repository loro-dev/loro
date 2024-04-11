use crate::{FractionalIndex, TERMINATOR};
use rand::Rng;

const MAX_JITTER: u8 = 3;

pub(super) fn new_before_jitter<R: Rng>(bytes: &[u8], rng: &mut R) -> Vec<u8> {
    for i in 0..bytes.len() {
        if bytes[i] > TERMINATOR {
            return bytes[..i].into();
        }
        if bytes[i] > u8::MIN {
            let mut ans: Vec<u8> = bytes[0..=i].into();
            ans[i] -= rng.gen_range(1..=ans[i].min(MAX_JITTER));
            return ans;
        }
    }
    unreachable!()
}

pub(super) fn new_after_jitter<R: Rng>(bytes: &[u8], rng: &mut R) -> Vec<u8> {
    for i in 0..bytes.len() {
        if bytes[i] < TERMINATOR {
            return bytes[0..i].into();
        }
        if bytes[i] < u8::MAX {
            let mut ans: Vec<u8> = bytes[0..=i].into();
            ans[i] += rng.gen_range(1..=MAX_JITTER.min(u8::MAX - ans[i]));
            return ans;
        }
    }
    unreachable!()
}

pub(super) fn new_between_jitter<R: Rng>(
    left: &[u8],
    right: &[u8],
    rng: &mut R,
) -> Option<FractionalIndex> {
    let shorter_len = left.len().min(right.len()) - 1;
    for i in 0..shorter_len {
        if left[i] < right[i] - 1 {
            let mut ans: Vec<u8> = left[0..=i].into();
            let mid = (left[i] + right[i]) / 2;
            ans[i] += rng.gen_range(
                mid.saturating_sub(MAX_JITTER / 2).max(1)
                    ..=mid
                        .saturating_add(MAX_JITTER / 2)
                        .min(right[i] - ans[i] - 1),
            );
            return FractionalIndex::from_vec_unterminated(ans).into();
        }
        if left[i] == right[i] - 1 {
            let (prefix, suffix) = left.split_at(i + 1);
            let new_suffix = new_after_jitter(suffix, rng);
            let mut ans = Vec::with_capacity(prefix.len() + new_suffix.len() + 1);
            ans.extend_from_slice(prefix);
            ans.extend_from_slice(&new_suffix);
            return FractionalIndex::from_vec_unterminated(ans).into();
        }
        if left[i] > right[i] {
            return None;
        }
    }

    match left.len().cmp(&right.len()) {
        std::cmp::Ordering::Less => {
            let (prefix, suffix) = right.split_at(shorter_len + 1);
            if prefix.last().unwrap() < &TERMINATOR {
                return None;
            }
            let new_suffix = new_before_jitter(suffix, rng);
            let mut ans = Vec::with_capacity(new_suffix.len() + prefix.len() + 1);
            ans.extend_from_slice(prefix);
            ans.extend_from_slice(&new_suffix);
            FractionalIndex::from_vec_unterminated(ans).into()
        }
        std::cmp::Ordering::Equal => None,
        std::cmp::Ordering::Greater => {
            let (prefix, suffix) = left.split_at(shorter_len + 1);
            if prefix.last().unwrap() >= &TERMINATOR {
                return None;
            }
            let new_suffix = new_after_jitter(suffix, rng);
            let mut ans = Vec::with_capacity(new_suffix.len() + prefix.len() + 1);
            ans.extend_from_slice(prefix);
            ans.extend_from_slice(&new_suffix);
            FractionalIndex::from_vec_unterminated(ans).into()
        }
    }
}

impl FractionalIndex {
    pub fn new<R: Rng>(
        lower: Option<&FractionalIndex>,
        upper: Option<&FractionalIndex>,
        rng: &mut R,
    ) -> Option<Self> {
        match (lower, upper) {
            (Some(lower), Some(upper)) => Self::new_between(lower, upper, rng),
            (Some(lower), None) => Self::new_after(lower, rng).into(),
            (None, Some(upper)) => Self::new_before(upper, rng).into(),
            (None, None) => FractionalIndex::default().into(),
        }
    }

    pub fn new_before<R: Rng>(FractionalIndex(bytes): &FractionalIndex, rng: &mut R) -> Self {
        FractionalIndex::from_vec_unterminated(new_before_jitter(bytes, rng))
    }

    pub fn new_after<R: Rng>(FractionalIndex(bytes): &FractionalIndex, rng: &mut R) -> Self {
        FractionalIndex::from_vec_unterminated(new_after_jitter(bytes, rng))
    }

    pub fn new_between<R: Rng>(
        FractionalIndex(left): &FractionalIndex,
        FractionalIndex(right): &FractionalIndex,
        rng: &mut R,
    ) -> Option<Self> {
        new_between_jitter(left, right, rng)
    }

    pub fn generate_n_evenly<R: Rng>(
        lower: Option<&FractionalIndex>,
        upper: Option<&FractionalIndex>,
        n: usize,
        rng: &mut R,
    ) -> Option<Vec<Self>> {
        fn gen(
            lower: Option<&FractionalIndex>,
            upper: Option<&FractionalIndex>,
            n: usize,
            push: &mut impl FnMut(FractionalIndex),
            rng: &mut impl Rng,
        ) {
            if n == 0 {
                return;
            }

            let mid = n / 2;
            let mid_ans = FractionalIndex::new(lower, upper, rng).unwrap();
            if n == 1 {
                push(mid_ans);
                return;
            }

            gen(lower, Some(&mid_ans), mid, push, rng);
            push(mid_ans.clone());
            if n - mid - 1 == 0 {
                return;
            }
            gen(Some(&mid_ans), upper, n - mid - 1, push, rng);
        }

        if n == 0 {
            return Some(Vec::new());
        }

        match (lower, upper) {
            (Some(a), Some(b)) if a >= b => return None,
            _ => {}
        }

        let mut ans = Vec::with_capacity(n);
        gen(lower, upper, n, &mut |v| ans.push(v), rng);
        Some(ans)
    }
}
