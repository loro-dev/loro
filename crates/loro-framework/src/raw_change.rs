use rle::{HasLength, Mergable};
use sha2::{Digest, Sha256};

pub(crate) type ChangeHash = [u8; 32];

pub struct ChangeData {
    pub counter: usize,
    pub len: usize,
    pub data: Vec<u8>,
    pub hash: Option<ChangeHash>,
}

impl ChangeData {
    pub fn update_hash(&mut self, prev_hash: Option<ChangeHash>) {
        self.hash = Some(self.calc_hash(prev_hash));
    }

    pub fn calc_hash(&mut self, prev_hash: Option<ChangeHash>) -> ChangeHash {
        let mut hasher = Sha256::new();
        hasher.update(&self.data);
        if let Some(prev) = prev_hash {
            hasher.update(prev);
        }

        hasher.finalize()[..].try_into().unwrap()
    }

    pub fn match_hash(&self, prev_hash: Option<ChangeHash>) -> bool {
        if let Some(hash) = self.hash {
            if let Some(prev) = prev_hash {
                hash == prev
            } else {
                false
            }
        } else {
            false
        }
    }
}

impl HasLength for ChangeData {
    fn content_len(&self) -> usize {
        self.len
    }
}

impl Mergable for ChangeData {
    fn is_mergable(&self, _other: &Self, _conf: &()) -> bool
    where
        Self: Sized,
    {
        false
    }

    fn merge(&mut self, _other: &Self, _conf: &())
    where
        Self: Sized,
    {
        unreachable!()
    }
}
