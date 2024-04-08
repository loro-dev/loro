#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[cfg(feature = "jitter")]
mod jitter;
#[cfg(feature = "jitter")]
use jitter::{new_after_jitter, new_before_jitter, new_between_jitter};

const TERMINATOR: u8 = 128;

#[derive(Debug, Clone, PartialEq, PartialOrd, Eq, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct FractionalIndex(Vec<u8>);

impl Default for FractionalIndex {
    fn default() -> Self {
        FractionalIndex(vec![TERMINATOR])
    }
}

fn new_before(bytes: &[u8]) -> Vec<u8> {
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

fn new_after(bytes: &[u8]) -> Vec<u8> {
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

fn new_between(left: &[u8], right: &[u8]) -> Option<FractionalIndex> {
    let shorter_len = left.len().min(right.len()) - 1;
    for i in 0..shorter_len {
        if left[i] < right[i] - 1 {
            let mut ans: Vec<u8> = left[0..=i].into();
            ans[i] += (left[i] + right[i]) / 2;
            return FractionalIndex::from_vec_unterminated(ans).into();
        }
        if left[i] == right[i] - 1 {
            let (prefix, suffix) = left.split_at(i + 1);
            let new_suffix = new_after(suffix);
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
            let new_suffix = new_before(suffix);
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
            let new_suffix = new_after(suffix);
            let mut ans = Vec::with_capacity(new_suffix.len() + prefix.len() + 1);
            ans.extend_from_slice(prefix);
            ans.extend_from_slice(&new_suffix);
            FractionalIndex::from_vec_unterminated(ans).into()
        }
    }
}

impl FractionalIndex {
    pub fn new(lower: Option<&FractionalIndex>, upper: Option<&FractionalIndex>) -> Option<Self> {
        match (lower, upper) {
            (Some(lower), Some(upper)) => Self::new_between(lower, upper),
            (Some(lower), None) => Self::new_after(lower).into(),
            (None, Some(upper)) => Self::new_before(upper).into(),
            (None, None) => FractionalIndex::default().into(),
        }
    }

    fn from_vec_unterminated(mut bytes: Vec<u8>) -> Self {
        bytes.push(TERMINATOR);
        FractionalIndex(bytes)
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, String> {
        if bytes.last() != Some(&TERMINATOR) {
            return Err(format!(
                "FractionalIndex must be terminated with {}",
                TERMINATOR
            ));
        }
        Ok(FractionalIndex(bytes))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn new_before(FractionalIndex(bytes): &FractionalIndex) -> Self {
        FractionalIndex::from_vec_unterminated(
            #[cfg(feature = "jitter")]
            new_before_jitter(bytes),
            #[cfg(not(feature = "jitter"))]
            new_before(bytes),
        )
    }

    pub fn new_after(FractionalIndex(bytes): &FractionalIndex) -> Self {
        FractionalIndex::from_vec_unterminated(
            #[cfg(feature = "jitter")]
            new_after_jitter(bytes),
            #[cfg(not(feature = "jitter"))]
            new_after(bytes),
        )
    }

    pub fn new_between(
        FractionalIndex(left): &FractionalIndex,
        FractionalIndex(right): &FractionalIndex,
    ) -> Option<Self> {
        #[cfg(feature = "jitter")]
        return new_between_jitter(left, right);
        #[cfg(not(feature = "jitter"))]
        new_between(left, right)
    }
}

impl ToString for FractionalIndex {
    fn to_string(&self) -> String {
        bytes_to_hex(&self.0)
    }
}

const HEX_CHARS: &[u8] = b"0123456789abcdef";

pub fn byte_to_hex(byte: u8) -> String {
    let mut s = String::new();
    s.push(HEX_CHARS[(byte >> 4) as usize] as char);
    s.push(HEX_CHARS[(byte & 0xf) as usize] as char);
    s
}

pub fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        s.push_str(&byte_to_hex(*byte));
    }
    s
}
