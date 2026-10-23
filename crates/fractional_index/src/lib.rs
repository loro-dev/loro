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
        FractionalIndex(Arc::new(bytes))
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        FractionalIndex(Arc::new(bytes))
    }

    pub fn from_hex_string<T: AsRef<str>>(str: T) -> Self {
        let s = str.as_ref();
        let mut bytes = Vec::with_capacity(s.len() / 2);
        for i in 0..s.len() / 2 {
            let byte = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap();
            bytes.push(byte);
        }
        FractionalIndex::from_bytes(bytes)
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
    // Every byte is 0xFF, which a jittered key can end with. Any extension of
    // `bytes` sorts after it, and the caller appends the terminator.
    bytes.into()
}

pub(crate) fn new_between(left: &[u8], right: &[u8], extra_capacity: usize) -> Option<Vec<u8>> {
    let shorter_len = left.len().min(right.len()) - 1;
    for i in 0..shorter_len {
        let diff = i16::from(right[i]) - i16::from(left[i]);
        if diff > 0 {
            return new_between_at(left, right, i, extra_capacity).into();
        }
        if diff < 0 {
            return None;
        }
    }

    // The last byte of the shorter key. Without jitter it is the terminator,
    // but a jittered key ends with a random byte, so it has to be compared.
    let i = shorter_len;
    match left.len().cmp(&right.len()) {
        std::cmp::Ordering::Less => {
            if right[i] < left[i] {
                return None;
            }
            let (prefix, suffix) = right.split_at(shorter_len + 1);
            if suffix.iter().all(|&b| b == u8::MIN) {
                // No terminated key sorts before an all-zero suffix
                return if left[i] < right[i] {
                    new_between_at(left, right, i, extra_capacity).into()
                } else {
                    None
                };
            }
            let new_suffix = new_before(suffix);
            let mut ans = Vec::with_capacity(new_suffix.len() + prefix.len() + extra_capacity);
            ans.extend_from_slice(prefix);
            ans.extend_from_slice(&new_suffix);
            ans.into()
        }
        std::cmp::Ordering::Equal => {
            if left[i] < right[i] {
                new_between_at(left, right, i, extra_capacity).into()
            } else {
                None
            }
        }
        std::cmp::Ordering::Greater => {
            if left[i] >= right[i] {
                return None;
            }
            let (prefix, suffix) = left.split_at(shorter_len + 1);
            let new_suffix = new_after(suffix);
            let mut ans = Vec::with_capacity(new_suffix.len() + prefix.len() + extra_capacity);
            ans.extend_from_slice(prefix);
            ans.extend_from_slice(&new_suffix);
            ans.into()
        }
    }
}

/// `left` and `right` share their first `i` bytes and `left[i] < right[i]`.
fn new_between_at(left: &[u8], right: &[u8], i: usize, extra_capacity: usize) -> Vec<u8> {
    let left_byte = left[i];
    let right_byte = right[i];
    if right_byte - left_byte > 1 {
        let mut ans: Vec<u8> = left[0..=i].into();
        ans[i] += (right_byte - left_byte) / 2;
        return ans;
    }
    let (prefix, suffix) = left.split_at(i + 1);
    let new_suffix = new_after(suffix);
    let mut ans = Vec::with_capacity(prefix.len() + new_suffix.len() + extra_capacity);
    ans.extend_from_slice(prefix);
    ans.extend_from_slice(&new_suffix);
    ans
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

    pub fn new_before(FractionalIndex(bytes): &FractionalIndex) -> Self {
        FractionalIndex::from_vec_unterminated(new_before(bytes))
    }

    pub fn new_after(FractionalIndex(bytes): &FractionalIndex) -> Self {
        FractionalIndex::from_vec_unterminated(new_after(bytes))
    }

    pub fn new_between(
        FractionalIndex(left): &FractionalIndex,
        FractionalIndex(right): &FractionalIndex,
    ) -> Option<Self> {
        new_between(left, right, 1).map(FractionalIndex::from_vec_unterminated)
    }

    pub fn generate_n_evenly(
        lower: Option<&FractionalIndex>,
        upper: Option<&FractionalIndex>,
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

#[cfg(test)]
mod tests {
    use super::FractionalIndex;

    #[test]
    fn new_between_handles_zero_shared_prefix() {
        let lower = FractionalIndex::from_hex_string("0080");
        let upper = FractionalIndex::from_hex_string("008180");
        let index = FractionalIndex::new_between(&lower, &upper).unwrap();

        assert!(lower < index);
        assert!(index < upper);
        assert_eq!(index.to_string(), "00817F80");
    }

    /// Every key `new_between_jitter` can make between `lower` and `upper`
    /// with `jitter` random bytes, at both extremes of the random bytes
    fn assert_jitter_between(lower: &str, upper: &str, jitter: u8) {
        let lower = FractionalIndex::from_hex_string(lower);
        let upper = FractionalIndex::from_hex_string(upper);
        for byte in [u8::MIN, u8::MAX] {
            let mut rng = ConstantByteRng(byte);
            let index = FractionalIndex::new_between_jitter(&lower, &upper, &mut rng, jitter)
                .unwrap_or_else(|| panic!("no index between {lower} and {upper}"));
            assert!(lower < index, "{lower} < {index}");
            assert!(index < upper, "{index} < {upper}");
        }
    }

    struct ConstantByteRng(u8);

    impl rand::RngCore for ConstantByteRng {
        fn next_u32(&mut self) -> u32 {
            u32::from(self.0) * 0x0101_0101
        }

        fn next_u64(&mut self) -> u64 {
            u64::from(self.0) * 0x0101_0101_0101_0101
        }

        fn fill_bytes(&mut self, dest: &mut [u8]) {
            dest.fill(self.0);
        }

        fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand::Error> {
            self.fill_bytes(dest);
            Ok(())
        }
    }

    #[test]
    fn new_between_jittered_keys() {
        // loro-dev/loro#1211: the bytes after the first difference are all 0xFF
        assert_jitter_between("8180FF", "818180CB", 1);
        assert_jitter_between("80FF", "8180", 1);
        // Equal length, differing only in the last (random) byte
        assert_jitter_between("8180AA", "8180CC", 1);
        assert_jitter_between("8180AA", "8180AB", 1);
        // The shorter key's last byte is a random byte above or below the terminator
        assert_jitter_between("809011", "80A0", 1);
        assert_jitter_between("8010", "8020", 1);
        assert_jitter_between("8010", "80200000", 1);
        assert_jitter_between("10", "1080", 1);
    }

    #[test]
    fn new_between_random_jittered_keys() {
        use rand::{Rng, SeedableRng};
        let mut rng = rand::rngs::StdRng::seed_from_u64(1211);
        let key = |rng: &mut rand::rngs::StdRng| {
            let mut bytes: Vec<u8> = (0..rng.gen_range(0..3)).map(|_| rng.gen()).collect();
            bytes.push(super::TERMINATOR);
            bytes.extend((0..rng.gen_range(0..3)).map(|_| rng.gen::<u8>()));
            FractionalIndex::from_bytes(bytes)
        };
        for _ in 0..100_000 {
            let (a, b) = (key(&mut rng), key(&mut rng));
            let (lower, upper) = match a.cmp(&b) {
                std::cmp::Ordering::Less => (a, b),
                std::cmp::Ordering::Equal => continue,
                std::cmp::Ordering::Greater => (b, a),
            };
            let (l, u) = (lower.as_bytes(), upper.as_bytes());
            if u.starts_with(l) && u[l.len()..].iter().all(|&b| b == 0) {
                // Nothing that ends with a terminator lies between them
                assert_eq!(FractionalIndex::new_between(&lower, &upper), None);
                continue;
            }
            assert_jitter_between(&lower.to_string(), &upper.to_string(), 2);
            assert!(FractionalIndex::new_between(&upper, &lower).is_none());
        }
    }
}
