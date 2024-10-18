use arbitrary::Arbitrary;
use bytes::Bytes;
use loro::kv_store::mem_store::MemKvConfig;
use loro::{KvStore, MemKvStore};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound;

#[derive(Clone, Arbitrary)]
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

impl std::fmt::Debug for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Action::Add { key, value } => {
                write!(
                    f,
                    "Add{{\n\tkey: vec!{:?}, \n\tvalue: vec!{:?}\n}}",
                    key, value
                )
            }
            Action::Get(index) => write!(f, "Get({})", index),
            Action::Remove(index) => write!(f, "Remove({})", index),
            Action::Scan {
                start,
                end,
                start_include,
                end_include,
            } => write!(
                f,
                "Scan{{\n\tstart: {:?}, \n\tend: {:?}, \n\tstart_include: {:?}, \n\tend_include: {:?}\n}}",
                start, end, start_include, end_include
            ),
            Action::ExportAndImport => write!(f, "ExportAndImport"),
            Action::Flush => write!(f, "Flush"),
        }
    }
}

pub struct MemKvFuzzer {
    kv: MemKvStore,
    btree: BTreeMap<Bytes, Bytes>,
    all_keys: BTreeSet<Bytes>,
    merged_kv: MemKvStore,
    merged_btree: BTreeMap<Bytes, Bytes>,
}

impl Default for MemKvFuzzer {
    fn default() -> Self {
        Self {
            kv: MemKvStore::new(MemKvConfig::new().should_encode_none(false)),
            btree: Default::default(),
            all_keys: Default::default(),
            merged_kv: MemKvStore::new(MemKvConfig::new().should_encode_none(false)),
            merged_btree: Default::default(),
        }
    }
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
                let key = self.all_keys.iter().nth(*index).unwrap();
                self.kv.remove(key);
                self.btree.insert(key.clone(), Bytes::new());
                self.all_keys.remove(&key.clone());
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
                let btree_scan: Vec<_> = self
                    .btree
                    .scan(start_bound, end_bound)
                    .filter(|(_, v)| !v.is_empty())
                    .collect();

                assert_eq!(kv_scan, btree_scan);

                let kv_scan: Vec<_> = self.kv.scan(start_bound, end_bound).rev().collect();
                let btree_scan: Vec<_> = self
                    .btree
                    .scan(start_bound, end_bound)
                    .filter(|(_, v)| !v.is_empty())
                    .rev()
                    .collect();

                assert_eq!(kv_scan, btree_scan);
            }
            Action::ExportAndImport => {
                let exported = self.kv.export_all();
                self.kv = MemKvStore::new(MemKvConfig::new().should_encode_none(true));
                self.merged_kv.import_all(exported).expect("import failed");
                self.merged_btree.extend(std::mem::take(&mut self.btree));

                for (key, value) in self.merged_btree.iter().filter(|(_, v)| !v.is_empty()) {
                    assert_eq!(
                        self.merged_kv.get(key),
                        Some(value.clone()),
                        "export and import failed key: {:?}",
                        key
                    );
                }
                self.all_keys.clear();
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
            .filter(|(_, v)| !v.is_empty())
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
            .filter(|(_, v)| !v.is_empty())
            .rev()
            .collect();

        assert_eq!(kv_scan, btree_scan);

        let merge_scan: Vec<_> = self
            .merged_kv
            .scan(Bound::Unbounded, Bound::Unbounded)
            .collect();
        let btree_scan: Vec<_> = self
            .merged_btree
            .scan(Bound::Unbounded, Bound::Unbounded)
            .filter(|(_, v)| !v.is_empty())
            .collect();
        assert_eq!(merge_scan, btree_scan);

        let merge_scan: Vec<_> = self
            .merged_kv
            .scan(Bound::Unbounded, Bound::Unbounded)
            .rev()
            .collect();
        let btree_scan: Vec<_> = self
            .merged_btree
            .scan(Bound::Unbounded, Bound::Unbounded)
            .filter(|(_, v)| !v.is_empty())
            .rev()
            .collect();

        assert_eq!(merge_scan, btree_scan);
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

pub fn test_random_bytes_import(bytes: &[u8]) {
    let mut kv = MemKvStore::new(MemKvConfig::new().should_encode_none(true));
    match kv.import_all(Bytes::from(bytes.to_vec())) {
        Ok(_) => {
            // do nothing
        }
        Err(_) => {
            // do nothing
        }
    }
}

pub fn minify_simple<T, F>(f: F, actions: Vec<T>)
where
    F: Fn(&mut [T]),
    T: Clone + std::fmt::Debug,
{
    std::panic::set_hook(Box::new(|_info| {
        // ignore panic output
        // println!("{:?}", _info);
    }));
    let f_ref: *const _ = &f;
    let f_ref: usize = f_ref as usize;
    #[allow(clippy::redundant_clone)]
    let mut actions_clone = actions.clone();
    let action_ref: usize = (&mut actions_clone) as *mut _ as usize;
    #[allow(clippy::blocks_in_conditions)]
    if std::panic::catch_unwind(|| {
        // SAFETY: test
        let f = unsafe { &*(f_ref as *const F) };
        // SAFETY: test
        let actions_ref = unsafe { &mut *(action_ref as *mut Vec<T>) };
        f(actions_ref);
    })
    .is_ok()
    {
        println!("No Error Found");
        return;
    }
    let mut minified = actions.clone();
    let mut current_index = minified.len() as i64 - 1;
    while current_index > 0 {
        let a = minified.remove(current_index as usize);
        let f_ref: *const _ = &f;
        let f_ref: usize = f_ref as usize;
        let mut actions_clone = minified.clone();
        let action_ref: usize = (&mut actions_clone) as *mut _ as usize;
        let mut re = false;
        #[allow(clippy::blocks_in_conditions)]
        if std::panic::catch_unwind(|| {
            // SAFETY: test
            let f = unsafe { &*(f_ref as *const F) };
            // SAFETY: test
            let actions_ref = unsafe { &mut *(action_ref as *mut Vec<T>) };
            f(actions_ref);
        })
        .is_err()
        {
            re = true;
        } else {
            minified.insert(current_index as usize, a);
        }
        println!(
            "{}/{} {}",
            actions.len() as i64 - current_index,
            actions.len(),
            re
        );
        current_index -= 1;
    }

    println!("{:?}", &minified);
    println!(
        "Old Length {}, New Length {}",
        actions.len(),
        minified.len()
    );
    if actions.len() > minified.len() {
        minify_simple(f, minified);
    }
}
