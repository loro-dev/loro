use bytes::Bytes;
use std::{
    ops::Bound,
    sync::{Arc, Mutex},
};

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
    ) -> Box<dyn DoubleEndedIterator<Item = (Bytes, Bytes)> + '_>;
    fn len(&self) -> usize;
    fn size(&self) -> usize;
    fn export_all(&self) -> Bytes;
    fn import_all(&mut self, bytes: Bytes) -> Result<(), String>;
    fn clone_store(&self) -> Arc<Mutex<dyn KvStore>>;
}

mod default_binary_format {
    //! Default binary format for the key-value store.
    //!
    //! It will compress the prefix of the keys that are common with the previous key.

    use bytes::Bytes;

    pub fn export_by_scan(store: &impl super::KvStore) -> bytes::Bytes {
        let mut buf = Vec::new();
        let mut last_key: Option<Bytes> = None;
        for (k, v) in store.scan(std::ops::Bound::Unbounded, std::ops::Bound::Unbounded) {
            {
                // Write the key
                match last_key.take() {
                    None => {
                        leb128::write::unsigned(&mut buf, k.len() as u64).unwrap();
                        buf.extend_from_slice(&k);
                    }
                    Some(last) => {
                        let (common, suffix) = get_common_prefix_len_and_strip(&k, &last);
                        buf.push(common);
                        leb128::write::unsigned(&mut buf, suffix.len() as u64).unwrap();
                        buf.extend_from_slice(suffix);
                    }
                };

                last_key = Some(k);
            }

            // Write the value
            leb128::write::unsigned(&mut buf, v.len() as u64).unwrap();
            buf.extend_from_slice(&v);
        }

        buf.into()
    }

    fn get_common_prefix_len_and_strip<'a>(this: &'a Bytes, last: &Bytes) -> (u8, &'a [u8]) {
        let mut common_prefix_len = 0;
        for (i, (a, b)) in this.iter().zip(last.iter()).enumerate() {
            if a != b || i == 255 {
                common_prefix_len = i;
                break;
            }
        }

        let suffix = &this[common_prefix_len..];
        (common_prefix_len as u8, suffix)
    }

    pub fn import(store: &mut impl super::KvStore, bytes: bytes::Bytes) -> Result<(), String> {
        let mut bytes: &[u8] = &bytes;
        let mut last_key = Vec::new();

        while !bytes.is_empty() {
            // Read the key
            let mut key = Vec::new();
            if last_key.is_empty() {
                let key_len = leb128::read::unsigned(&mut bytes).map_err(|e| e.to_string())?;
                let key_len = key_len as usize;
                key.extend_from_slice(&bytes[..key_len]);
                bytes = &bytes[key_len..];
            } else {
                let common_prefix_len = bytes[0] as usize;
                bytes = &bytes[1..];
                key.extend_from_slice(&last_key[..common_prefix_len]);
                let suffix_len = leb128::read::unsigned(&mut bytes).map_err(|e| e.to_string())?;
                let suffix_len = suffix_len as usize;
                key.extend_from_slice(&bytes[..suffix_len]);
                bytes = &bytes[suffix_len..];
            }

            // Read the value
            let value_len = leb128::read::unsigned(&mut bytes).map_err(|e| e.to_string())?;
            let value_len = value_len as usize;
            let value = Bytes::copy_from_slice(&bytes[..value_len]);
            bytes = &bytes[value_len..];

            // Store the key-value pair
            store.set(&key, value);

            last_key = key;
        }

        Ok(())
    }
}

mod mem {
    use super::*;
    use std::{collections::BTreeMap, sync::Arc};
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
        ) -> Box<dyn DoubleEndedIterator<Item = (Bytes, Bytes)> + '_> {
            Box::new(
                self.range::<[u8], _>((start, end))
                    .map(|(k, v)| (k.clone(), v.clone())),
            )
        }

        fn len(&self) -> usize {
            self.len()
        }

        fn size(&self) -> usize {
            self.iter().fold(0, |acc, (k, v)| acc + k.len() + v.len())
        }

        fn export_all(&self) -> Bytes {
            default_binary_format::export_by_scan(self)
        }

        fn import_all(&mut self, bytes: Bytes) -> Result<(), String> {
            default_binary_format::import(self, bytes)
        }

        fn clone_store(&self) -> Arc<Mutex<dyn KvStore>> {
            Arc::new(Mutex::new(self.clone()))
        }
    }

    #[cfg(test)]
    mod test {
        use super::*;

        #[test]
        fn test_export_and_import_all() {
            let mut store1 = MemKvStore::default();
            store1.insert(Bytes::from("key1"), Bytes::from("value1"));
            store1.insert(Bytes::from("key2"), Bytes::from("value2"));

            let exported = store1.export_all();
            assert!(!exported.is_empty());

            let mut store2 = MemKvStore::default();
            let result = store2.import_all(exported);

            assert!(result.is_ok());
            assert_eq!(
                store2.get(&Bytes::from("key1")),
                Some(&Bytes::from("value1"))
            );
            assert_eq!(
                store2.get(&Bytes::from("key2")),
                Some(&Bytes::from("value2"))
            );
            assert_eq!(store1.len(), store2.len());
            assert_eq!(store1.size(), store2.size());
            assert_eq!(store1, store2);
        }
    }
}
