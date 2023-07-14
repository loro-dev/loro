use serde::{Deserialize, Serialize};
use serde_columnar::columnar;

#[columnar(ser, de)]
#[derive(Debug, Serialize, Deserialize)]
pub(super) struct SnapshotEncoded {
    // #[columnar(type = "vec")]
    // pub(crate) changes: Vec<ChangeEncoding>,
    // #[columnar(type = "vec")]
    // ops: Vec<SnapshotOpEncoding>,
    // #[columnar(type = "vec")]
    // deps: Vec<DepsEncoding>,
    // clients: Clients,
    // containers: Containers,
    // bytes: Vec<u8>,
    // keys: Vec<InternalString>,
    // values: Vec<LoroValue>,
}
