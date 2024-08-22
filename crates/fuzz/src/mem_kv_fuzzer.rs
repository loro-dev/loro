use arbitrary::Arbitrary;
use bytes::Bytes;
use loro::{KvStore, MemKvStore};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound;

#[derive(Debug, Clone, Arbitrary)]
pub enum Action {
    Add {
        key: Vec<u8>,
        value: Vec<u8>,
    },
    Get(usize),
    Remove(usize),
    Scan {
        start: usize,
        end: usize,
        start_include: bool,
        end_include: bool,
    },
    ExportAndImport,
    Flush,
}

#[derive(Default)]
pub struct MemKvFuzzer {
    kv: MemKvStore,
    btree: BTreeMap<Bytes, Bytes>,
    all_keys: BTreeSet<Bytes>,
}

impl MemKvFuzzer {
    fn prepare(&self, action: &mut Action) {
        match action {
            Action::Add { key, value } => {
                if key.is_empty() {
                    *key = vec![0];
                }
                if value.is_empty() {
                    *value = vec![0];
                }
            }
            Action::Get(index) | Action::Remove(index) => {
                if self.all_keys.is_empty() {
                    *action = Action::Add {
                        key: vec![0],
                        value: vec![0],
                    };
                } else {
                    *index %= self.all_keys.len();
                }
            }
            Action::Scan {
                start,
                end,
                start_include,
                end_include,
            } => {
                if self.all_keys.is_empty() {
                    *action = Action::Add {
                        key: vec![0],
                        value: vec![0],
                    };
                } else {
                    *start %= self.all_keys.len();
                    *end %= self.all_keys.len();
                    if *start > *end {
                        std::mem::swap(start, end);
                    } else if *start == *end && !*start_include && !*end_include {
                        *end_include = true;
                    }
                }
            }
            Action::ExportAndImport | Action::Flush => {}
        }
    }

    fn apply(&mut self, action: &Action) {
        match action {
            Action::Add { key, value } => {
                let key_bytes = Bytes::from(key.clone());
                let value_bytes = Bytes::from(value.clone());
                self.kv.set(&key_bytes, value_bytes.clone());
                self.btree.insert(key_bytes.clone(), value_bytes);
                self.all_keys.insert(key_bytes);
            }
            Action::Get(index) => {
                if let Some(key) = self.all_keys.iter().nth(*index) {
                    let kv_result = self.kv.get(key);
                    let btree_result = self.btree.get(key).cloned();
                    assert_eq!(kv_result, btree_result, "get failed");
                }
            }
            Action::Remove(index) => {
                if let Some(key) = self.all_keys.iter().nth(*index).cloned() {
                    self.kv.remove(&key);
                    self.btree.remove(&key);
                }
            }
            Action::Scan {
                start,
                end,
                start_include,
                end_include,
            } => {
                let keys: Vec<_> = self.all_keys.iter().collect();
                let start_bound = if *start_include {
                    Bound::Included(&keys[*start][..])
                } else {
                    Bound::Excluded(&keys[*start][..])
                };
                let end_bound = if *end_include {
                    Bound::Included(&keys[*end][..])
                } else {
                    Bound::Excluded(&keys[*end][..])
                };

                let kv_scan: Vec<_> = self.kv.scan(start_bound, end_bound).collect();
                let btree_scan: Vec<_> = self.btree.scan(start_bound, end_bound).collect();

                assert_eq!(kv_scan, btree_scan);

                let kv_scan: Vec<_> = self.kv.scan(start_bound, end_bound).rev().collect();
                let btree_scan: Vec<_> = self.btree.scan(start_bound, end_bound).rev().collect();

                assert_eq!(kv_scan, btree_scan);
            }
            Action::ExportAndImport => {
                let exported = self.kv.export_all();
                let mut new_kv = MemKvStore::default();
                new_kv.import_all(exported).expect("import failed");

                for (key, value) in self.btree.iter() {
                    assert_eq!(
                        new_kv.get(key),
                        Some(value.clone()),
                        "export and import failed"
                    );
                }

                self.kv = new_kv;
            }
            Action::Flush => {
                self.kv.export_all();
            }
        }
    }

    fn equal(&self) {
        let kv_scan: Vec<_> = self.kv.scan(Bound::Unbounded, Bound::Unbounded).collect();
        let btree_scan: Vec<_> = self
            .btree
            .scan(Bound::Unbounded, Bound::Unbounded)
            .collect();

        assert_eq!(kv_scan, btree_scan);
        let kv_scan: Vec<_> = self
            .kv
            .scan(Bound::Unbounded, Bound::Unbounded)
            .rev()
            .collect();
        let btree_scan: Vec<_> = self
            .btree
            .scan(Bound::Unbounded, Bound::Unbounded)
            .rev()
            .collect();

        assert_eq!(kv_scan, btree_scan);
    }
}

pub fn test_mem_kv_fuzzer(actions: &mut [Action]) {
    let mut fuzzer = MemKvFuzzer::default();
    let mut applied = Vec::new();
    for action in actions {
        fuzzer.prepare(action);
        applied.push(action.clone());
        tracing::info!("\n{:#?}", applied);
        fuzzer.apply(action);
    }
    tracing::info!("\n{:#?}", applied);
    fuzzer.equal();
}
