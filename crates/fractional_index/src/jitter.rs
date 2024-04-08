use crate::{FractionalIndex, TERMINATOR};
use rand::Rng;

const MAX_JITTER: u8 = 3;

pub(super) fn new_before_jitter(bytes: &[u8]) -> Vec<u8> {
    let mut rng = rand::thread_rng();
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

pub(super) fn new_after_jitter(bytes: &[u8]) -> Vec<u8> {
    let mut rng = rand::thread_rng();

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

pub(super) fn new_between_jitter(left: &[u8], right: &[u8]) -> Option<FractionalIndex> {
    let shorter_len = left.len().min(right.len()) - 1;
    let mut rng = rand::thread_rng();
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
            let new_suffix = new_after_jitter(suffix);
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
            let new_suffix = new_before_jitter(suffix);
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
            let new_suffix = new_after_jitter(suffix);
            let mut ans = Vec::with_capacity(new_suffix.len() + prefix.len() + 1);
            ans.extend_from_slice(prefix);
            ans.extend_from_slice(&new_suffix);
            FractionalIndex::from_vec_unterminated(ans).into()
        }
    }
}
