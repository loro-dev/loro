use bytes::Bytes;
use std::ops::Bound;

pub trait KvStore: std::fmt::Debug + Send + Sync {
    fn get(&self, key: &[u8]) -> Option<Bytes>;
    fn set(&mut self, key: &[u8], value: Bytes);
    fn compare_and_swap(&mut self, key: &[u8], old: Option<Bytes>, new: Bytes) -> bool;
    fn remove(&mut self, key: &[u8]);
    fn contains_key(&self, key: &[u8]) -> bool;
    fn scan(
        &self,
        start: Bound<&[u8]>,
        end: Bound<&[u8]>,
    ) -> Box<dyn Iterator<Item = (Bytes, Bytes)>>;
    fn len(&self) -> usize;
    fn size(&self) -> usize;
    fn export_all(&self) -> Bytes;
    fn import_all(&mut self, bytes: Bytes) -> Result<(), String>;
}

mod mem {
    use super::*;
    use std::collections::BTreeMap;
    pub type MemKvStore = BTreeMap<Bytes, Bytes>;

    impl KvStore for MemKvStore {
        fn get(&self, key: &[u8]) -> Option<Bytes> {
            self.get(key).cloned()
        }

        fn set(&mut self, key: &[u8], value: Bytes) {
            self.insert(Bytes::copy_from_slice(key), value);
        }

        fn compare_and_swap(&mut self, key: &[u8], old: Option<Bytes>, new: Bytes) -> bool {
            let key = Bytes::copy_from_slice(key);
            match self.get_mut(&key) {
                Some(v) => {
                    if old.as_ref() == Some(v) {
                        self.insert(key, new);
                        true
                    } else {
                        false
                    }
                }
                None => {
                    if old.is_none() {
                        self.insert(key, new);
                        true
                    } else {
                        false
                    }
                }
            }
        }

        fn remove(&mut self, key: &[u8]) {
            self.remove(key);
        }

        fn contains_key(&self, key: &[u8]) -> bool {
            self.contains_key(key)
        }

        fn scan(
            &self,
            start: Bound<&[u8]>,
            end: Bound<&[u8]>,
        ) -> Box<dyn Iterator<Item = (Bytes, Bytes)>> {
            todo!()
        }

        fn len(&self) -> usize {
            self.len()
        }

        fn size(&self) -> usize {
            self.iter().fold(0, |acc, (k, v)| acc + k.len() + v.len())
        }

        fn export_all(&self) -> Bytes {
            todo!()
        }

        fn import_all(&mut self, bytes: Bytes) -> Result<(), String> {
            todo!()
        }
    }
}
