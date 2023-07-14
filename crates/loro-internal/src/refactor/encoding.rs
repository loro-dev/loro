#![allow(warnings)]

use fxhash::FxHashMap;
use serde::{Deserialize, Serialize};
use serde_columnar::{columnar, to_vec};

use crate::{
    change::Timestamp,
    container::ContainerID,
    id::{Counter, PeerID},
    InternalString, LoroError, LoroValue,
};

use super::oplog::OpLog;

type Containers = Vec<ContainerID>;
type ClientIdx = u32;
type Clients = Vec<PeerID>;

#[columnar(ser, de)]
#[derive(Debug, Serialize, Deserialize)]
pub(super) struct SnapshotEncoded {
    #[columnar(type = "vec")]
    pub(crate) changes: Vec<ChangeEncoding>,
    #[columnar(type = "vec")]
    ops: Vec<SnapshotOpEncoding>,
    #[columnar(type = "vec")]
    deps: Vec<DepsEncoding>,
    clients: Clients,
    containers: Containers,
    bytes: Vec<u8>,
    keys: Vec<InternalString>,
    values: Vec<LoroValue>,
}

#[columnar(vec, ser, de)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeEncoding {
    #[columnar(strategy = "Rle", original_type = "u32")]
    pub(super) client_idx: ClientIdx,
    #[columnar(strategy = "DeltaRle", original_type = "i64")]
    pub(super) timestamp: Timestamp,
    pub(super) op_len: u32,
    /// The length of deps that exclude the dep on the same client
    #[columnar(strategy = "Rle")]
    pub(super) deps_len: u32,
    /// Whether the change has a dep on the same client.
    /// It can save lots of space by using this field instead of [`DepsEncoding`]
    #[columnar(strategy = "BoolRle")]
    pub(super) dep_on_self: bool,
}

#[columnar(vec, ser, de)]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SnapshotOpEncoding {
    #[columnar(strategy = "Rle", original_type = "usize")]
    container: usize,
    /// key index or insert/delete pos
    #[columnar(strategy = "DeltaRle")]
    prop: usize,
    // list range start or del len or map value index, maybe negative
    value: i64,
    // List: the length of content when inserting, -2 when the inserted content is unknown, and -1 when deleting.
    // Map: always -1
    #[columnar(strategy = "Rle")]
    value2: i64,
}

#[columnar(vec, ser, de)]
#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub(super) struct DepsEncoding {
    #[columnar(strategy = "Rle", original_type = "u32")]
    pub(super) client_idx: ClientIdx,
    #[columnar(strategy = "DeltaRle", original_type = "i32")]
    pub(super) counter: Counter,
}

impl DepsEncoding {
    pub(super) fn new(client_idx: ClientIdx, counter: Counter) -> Self {
        Self {
            client_idx,
            counter,
        }
    }
}

// pub(super) fn encode_snapshot(store: &OpLog, gc: bool) -> Result<Vec<u8>, LoroError> {
//     let mut client_id_to_idx: FxHashMap<PeerID, ClientIdx> = FxHashMap::default();
//     let mut clients = Vec::with_capacity(store.changes.len());
//     let mut change_num = 0;
//     for (key, changes) in store.changes.iter() {
//         client_id_to_idx.insert(*key, clients.len() as ClientIdx);
//         clients.push(*key);
//         change_num += changes.merged_len();
//     }

//     let containers = store.arena.export_containers();
//     // During a transaction, we may create some containers which are deleted later. And these containers also need a unique ContainerIdx.
//     // So when we encode snapshot, we need to sort the containers by ContainerIdx and change the `container` of ops to the index of containers.
//     // An empty store decodes the snapshot, it will create these containers in a sequence of natural numbers so that containers and ops can correspond one-to-one
//     let container_to_new_idx: FxHashMap<_, _> = containers
//         .iter()
//         .enumerate()
//         .map(|(i, id)| (id, i))
//         .collect();

//     let mut changes = Vec::with_capacity(change_num);
//     let mut ops = Vec::with_capacity(change_num);
//     let mut keys = Vec::new();
//     let mut key_to_idx = FxHashMap::default();
//     let mut deps = Vec::with_capacity(change_num);
//     for (client_idx, (_, change_vec)) in store.changes.iter().enumerate() {
//         for change in change_vec.iter() {
//             let client_id = change.id.peer;
//             let mut op_len = 0;
//             let mut deps_len = 0;
//             let mut dep_on_self = false;
//             for dep in change.deps.iter() {
//                 // the first change will encode the self-client deps
//                 if dep.peer == client_id {
//                     dep_on_self = true;
//                 } else {
//                     deps.push(DepsEncoding::new(
//                         *client_id_to_idx.get(&dep.peer).unwrap(),
//                         dep.counter,
//                     ));
//                     deps_len += 1;
//                 }
//             }
//             for op in change.ops.iter() {
//                 let container_idx = op.container;
//                 let container_id = store.reg.get_id(container_idx).unwrap();
//                 let container = store.reg.get(container_id).unwrap();
//                 let new_ops = container
//                     .upgrade()
//                     .unwrap()
//                     .try_lock()
//                     .unwrap()
//                     .to_export_snapshot(&op.content, gc);
//                 let new_idx = *container_to_new_idx.get(container_id).unwrap();
//                 op_len += new_ops.len();
//                 for op_content in new_ops {
//                     let (prop, value, value2) =
//                         convert_inner_content(&op_content, &mut key_to_idx, &mut keys);
//                     ops.push(SnapshotOpEncoding {
//                         container: new_idx,
//                         prop,
//                         value,
//                         value2,
//                     });
//                 }
//             }

//             changes.push(ChangeEncoding {
//                 client_idx: client_idx as ClientIdx,
//                 timestamp: change.timestamp,
//                 deps_len,
//                 dep_on_self,
//                 op_len: op_len as u32,
//             });
//         }
//     }

//     todo!("compress bytes");
//     let encoded = SnapshotEncoded {
//         changes,
//         ops,
//         deps,
//         clients,
//         containers,
//         keys,
//         bytes: todo!(),
//         values: todo!(),
//     };
//     to_vec(&encoded).map_err(|e| LoroError::DecodeError(e.to_string().into()))
// }
