use append_only_bytes::{AppendOnlyBytes, BytesSlice};

/// It's just a wrapper around [BytesSlice] that makes sure
/// the content of the bytes is a valid utf-8 string
pub(crate) struct StrSlice(BytesSlice);

impl StrSlice {
    #[allow(unused)]
    pub fn new(bytes: BytesSlice) -> Option<Self> {
        let _str = std::str::from_utf8(&bytes).ok()?;
        Some(Self(bytes))
    }

    pub fn new_from_str(str: &str) -> Self {
        let mut a = AppendOnlyBytes::new();
        a.push_str(str);
        Self(a.slice(..))
    }

    pub fn bytes(&self) -> &BytesSlice {
        &self.0
    }

    pub fn as_str(&self) -> &str {
        // SAFETY: We ensure that the content is always valid utf8
        unsafe { std::str::from_utf8_unchecked(&self.0) }
    }

    pub fn split_at_unicode_pos(&self, pos: usize) -> (Self, Self) {
        let s = self.as_str();
        let mut split_at = self.0.len();
        for (u, (i, _)) in s.char_indices().enumerate() {
            if u == pos {
                split_at = i;
                break;
            }
        }

        (
            Self(self.0.slice_clone(..split_at)),
            Self(self.0.slice_clone(split_at..)),
        )
    }
}
