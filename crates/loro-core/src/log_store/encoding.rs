use columnar::{columnar, to_vec};
use fxhash::FxHashMap;
use serde::{Deserialize, Serialize};

use crate::{
    change::{Lamport, Timestamp},
    container::{list::list_op::ListOp, map::MapSet, text::text_content::ListSlice, ContainerID},
    id::{ClientID, ContainerIdx, Counter},
    InternalString, LogStore, LoroValue,
};

type ClientIdx = u32;
type Clients = Vec<ClientID>;
type Containers = Vec<ContainerID>;

#[columnar(vec, ser, de)]
#[derive(Clone, Serialize, Deserialize)]
struct ChangeEncoding {
    #[columnar(strategy = "DeltaRle", original_type = "u32")]
    client_idx: ClientIdx,
    #[columnar(strategy = "DeltaRle", original_type = "i32")]
    counter: Counter,
    #[columnar(strategy = "DeltaRle", original_type = "u32")]
    lamport: Lamport,
    #[columnar(strategy = "DeltaRle", original_type = "i64")]
    timestamp: Timestamp,
    #[columnar(original_type = "u32")]
    op_len: u32,
}

#[columnar(vec, ser, de)]
#[derive(Clone, Serialize, Deserialize)]
struct OpEncoding {
    #[columnar(original_type = "u32")]
    container: ContainerIdx,
    /// key index or insert/delete pos
    #[columnar(strategy = "DeltaRle", original_type = "u32")]
    prop: usize,
    value: LoroValue,
}

// can use 0 to compress even further
const NO_A_CLIENT: ClientIdx = ClientIdx::MAX;

#[columnar(vec, ser, de)]
#[derive(Clone, Serialize, Deserialize)]
struct DepsEncoding {
    #[columnar(strategy = "Rle", original_type = "u32")]
    client_idx: ClientIdx,
    #[columnar(strategy = "DeltaRle", original_type = "i32")]
    counter_or_len: Counter,
}

impl DepsEncoding {
    fn new_len(len: usize) -> Self {
        Self {
            client_idx: NO_A_CLIENT,
            counter_or_len: len as Counter,
        }
    }

    fn new_id(client_idx: ClientIdx, counter: Counter) -> Self {
        Self {
            client_idx,
            counter_or_len: counter,
        }
    }
}

#[columnar(ser, de)]
#[derive(Serialize, Deserialize)]
struct Encoded {
    #[columnar(type = "vec")]
    changes: Vec<ChangeEncoding>,
    #[columnar(type = "vec")]
    ops: Vec<OpEncoding>,
    #[columnar(type = "vec")]
    deps: Vec<DepsEncoding>,
    clients: Clients,
    containers: Containers,
    keys: Vec<InternalString>,
}

fn encode_changes(store: &LogStore) -> Encoded {
    let mut client_id_to_idx: FxHashMap<ClientID, ClientIdx> = FxHashMap::default();
    let mut clients = Vec::with_capacity(store.changes.len());
    let mut change_num = 0;
    for (key, changes) in store.changes.iter() {
        client_id_to_idx.insert(*key, clients.len() as ClientIdx);
        clients.push(*key);
        change_num += changes.merged_len();
    }

    let (container_id_to_idx, containers) = store.reg.export();
    let mut changes = Vec::with_capacity(change_num);
    let mut ops = Vec::with_capacity(change_num);
    let mut keys = Vec::new();
    let mut key_to_idx = FxHashMap::default();
    let mut deps = Vec::with_capacity(change_num);
    for (client_idx, (_, change_vec)) in store.changes.iter().enumerate() {
        for change in change_vec.iter() {
            changes.push(ChangeEncoding {
                client_idx: client_idx as ClientIdx,
                counter: change.id.counter,
                lamport: change.lamport,
                timestamp: change.timestamp,
                op_len: change.ops.merged_len() as u32,
            });

            deps.push(DepsEncoding::new_len(change.deps.len()));
            for dep in change.deps.iter() {
                deps.push(DepsEncoding::new_id(
                    *client_id_to_idx.get(&dep.client_id).unwrap(),
                    dep.counter,
                ));
            }

            let change = store.change_to_export_format(change);
            for op in change.ops.into_iter() {
                let container = *container_id_to_idx.get(&op.container).unwrap();
                let content = op.content.into_normal().unwrap();
                let (prop, value) = match content {
                    crate::op::Content::Container(_) => {
                        todo!();
                    }
                    crate::op::Content::Map(MapSet { key, value }) => (
                        *key_to_idx.entry(key.clone()).or_insert_with(|| {
                            keys.push(key);
                            keys.len() - 1
                        }),
                        value,
                    ),
                    crate::op::Content::List(list) => match list {
                        ListOp::Insert { slice, pos } => (
                            pos as usize,
                            match slice {
                                ListSlice::RawData(v) => v.clone().into(),
                                ListSlice::RawStr(s) => s.to_string().into(),
                                _ => unreachable!(),
                            },
                        ),
                        ListOp::Delete(span) => {
                            (span.pos as usize, LoroValue::I32(span.len as i32))
                        }
                    },
                    crate::op::Content::Dyn(_) => unreachable!(),
                };
                ops.push(OpEncoding {
                    container,
                    prop,
                    value,
                })
            }
        }
    }

    Encoded {
        changes,
        ops,
        deps,
        clients,
        containers,
        keys,
    }
}

impl LogStore {
    pub fn encode_snapshot(&self) -> Vec<u8> {
        let encoded = encode_changes(self);
        to_vec(&encoded).unwrap()
    }
}

fn decode_changes(encoded: Encoded) -> LogStore {
    let Encoded {
        changes,
        ops,
        deps,
        clients,
        containers,
        keys,
    } = encoded;

    todo!()
}
