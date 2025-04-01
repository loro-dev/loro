use std::{
    fmt::{Display, Write},
    sync::Arc,
};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

mod jitter;

const TERMINATOR: u8 = 128;
static DEFAULT_FRACTIONAL_INDEX: once_cell::sync::Lazy<FractionalIndex> =
    once_cell::sync::Lazy::new(|| FractionalIndex(Arc::new(vec![TERMINATOR])));

#[derive(Debug, Clone, PartialEq, PartialOrd, Eq, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct FractionalIndex(Arc<Vec<u8>>);

impl Default for FractionalIndex {
    fn default() -> Self {
        DEFAULT_FRACTIONAL_INDEX.clone()
    }
}

impl FractionalIndex {
    fn from_vec_unterminated(mut bytes: Vec<u8>) -> Self {
        bytes.push(TERMINATOR);
        Self(Arc::new(bytes))
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self(Arc::new(bytes))
    }

    pub fn from_hex_string<T: AsRef<str>>(str: T) -> Self {
        let s = str.as_ref();
        let mut bytes = Vec::with_capacity(s.len() / 2);
        for i in 0..s.len() / 2 {
            let byte = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap();
            bytes.push(byte);
        }
        Self::from_bytes(bytes)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

pub(crate) fn new_before(bytes: &[u8]) -> Vec<u8> {
    for i in 0..bytes.len() {
        if bytes[i] > TERMINATOR {
            return bytes[..i].into();
        }
        if bytes[i] > u8::MIN {
            let mut ans: Vec<u8> = bytes[..=i].into();
            ans[i] -= 1;
            return ans;
        }
    }
    unreachable!()
}

pub(crate) fn new_after(bytes: &[u8]) -> Vec<u8> {
    for i in 0..bytes.len() {
        if bytes[i] < TERMINATOR {
            return bytes[0..i].into();
        }
        if bytes[i] < u8::MAX {
            let mut ans: Vec<u8> = bytes[0..=i].into();
            ans[i] += 1;
            return ans;
        }
    }
    unreachable!()
}

pub(crate) fn new_between(left: &[u8], right: &[u8], extra_capacity: usize) -> Option<Vec<u8>> {
    let shorter_len = left.len().min(right.len()) - 1;
    for i in 0..shorter_len {
        if left[i] < right[i] - 1 {
            let mut ans: Vec<u8> = left[0..=i].into();
            ans[i] += (right[i] - left[i]) / 2;
            return ans.into();
        }
        if left[i] == right[i] - 1 {
            let (prefix, suffix) = left.split_at(i + 1);
            let new_suffix = new_after(suffix);
            let mut ans = Vec::with_capacity(prefix.len() + new_suffix.len() + extra_capacity);
            ans.extend_from_slice(prefix);
            ans.extend_from_slice(&new_suffix);
            return ans.into();
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
            let new_suffix = new_before(suffix);
            let mut ans = Vec::with_capacity(new_suffix.len() + prefix.len() + extra_capacity);
            ans.extend_from_slice(prefix);
            ans.extend_from_slice(&new_suffix);
            ans.into()
        }
        std::cmp::Ordering::Equal => None,
        std::cmp::Ordering::Greater => {
            let (prefix, suffix) = left.split_at(shorter_len + 1);
            if prefix.last().unwrap() >= &TERMINATOR {
                return None;
            }
            let new_suffix = new_after(suffix);
            let mut ans = Vec::with_capacity(new_suffix.len() + prefix.len() + extra_capacity);
            ans.extend_from_slice(prefix);
            ans.extend_from_slice(&new_suffix);
            ans.into()
        }
    }
}
impl FractionalIndex {
    pub fn new(lower: Option<&Self>, upper: Option<&Self>) -> Option<Self> {
        match (lower, upper) {
            (Some(lower), Some(upper)) => Self::new_between(lower, upper),
            (Some(lower), None) => Self::new_after(lower).into(),
            (None, Some(upper)) => Self::new_before(upper).into(),
            (None, None) => Self::default().into(),
        }
    }

    pub fn new_before(Self(bytes): &Self) -> Self {
        Self::from_vec_unterminated(new_before(bytes))
    }

    pub fn new_after(Self(bytes): &Self) -> Self {
        Self::from_vec_unterminated(new_after(bytes))
    }

    pub fn new_between(Self(left): &Self, Self(right): &Self) -> Option<Self> {
        new_between(left, right, 1).map(Self::from_vec_unterminated)
    }

    pub fn generate_n_evenly(
        lower: Option<&Self>,
        upper: Option<&Self>,
        n: usize,
    ) -> Option<Vec<Self>> {
        fn generate(
            lower: Option<&FractionalIndex>,
            upper: Option<&FractionalIndex>,
            n: usize,
            push: &mut impl FnMut(FractionalIndex),
        ) {
            if n == 0 {
                return;
            }

            let mid = n / 2;
            let mid_ans = FractionalIndex::new(lower, upper).unwrap();
            if n == 1 {
                push(mid_ans);
                return;
            }

            generate(lower, Some(&mid_ans), mid, push);
            push(mid_ans.clone());
            if n - mid - 1 == 0 {
                return;
            }
            generate(Some(&mid_ans), upper, n - mid - 1, push);
        }

        if n == 0 {
            return Some(Vec::new());
        }

        match (lower, upper) {
            (Some(a), Some(b)) if a >= b => return None,
            _ => {}
        }

        let mut ans = Vec::with_capacity(n);
        generate(lower, upper, n, &mut |v| ans.push(v));
        Some(ans)
    }
}

impl Display for FractionalIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", bytes_to_hex(&self.0))
    }
}

pub fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut output, b| {
        let _ = write!(output, "{b:02X}");
        output
    })
}
