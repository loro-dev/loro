use rle::{HasLength, RleVec};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use tracing::{debug, instrument};

use crate::{
    change::{Change, Lamport, Timestamp},
    container::ContainerID,
    event::RawEvent,
    id::{ClientID, Counter, ID},
    op::{RemoteContent, RemoteOp},
    LogStore, LoroCore, LoroError, VersionVector,
};

use super::RemoteClientChanges;

#[derive(Serialize, Deserialize, Debug)]
struct Updates {
    changes: Vec<EncodedClientChanges>,
}

/// the continuous changes from the same client
#[derive(Serialize, Deserialize, Debug)]
struct EncodedClientChanges {
    meta: FirstChangeInfo,
    data: Vec<EncodedChange>,
}

#[derive(Serialize, Deserialize, Debug)]
struct FirstChangeInfo {
    pub(crate) client: ClientID,
    pub(crate) counter: Counter,
    pub(crate) lamport: Lamport,
    pub(crate) timestamp: Timestamp,
}

#[derive(Serialize, Deserialize, Debug)]
struct EncodedOp {
    pub(crate) container: ContainerID,
    pub(crate) contents: Vec<RemoteContent>,
}

#[derive(Serialize, Deserialize, Debug)]
struct EncodedChange {
    pub(crate) ops: Vec<EncodedOp>,
    pub(crate) deps_except_self: Vec<ID>,
    pub(crate) lamport_delta: u32,
    pub(crate) timestamp_delta: i64,
}

impl LogStore {
    #[instrument(skip_all)]
    pub fn export_updates(&self, from: &VersionVector) -> Result<Vec<u8>, LoroError> {
        let changes = self.export(from);
        let mut updates = Updates {
            changes: Vec::with_capacity(changes.len()),
        };
        for (_, changes) in changes {
            let encoded = convert_changes_to_encoded(changes.into_iter());
            updates.changes.push(encoded);
        }

        postcard::to_allocvec(&updates)
            .map_err(|err| LoroError::DecodeError(err.to_string().into_boxed_str()))
    }

    pub fn import_updates(&mut self, input: &[u8]) -> Result<Vec<RawEvent>, postcard::Error> {
        let updates: Updates = postcard::from_bytes(input)?;
        let mut changes: RemoteClientChanges = Default::default();
        for encoded in updates.changes {
            changes.insert(encoded.meta.client, convert_encoded_to_changes(encoded));
        }

        Ok(self.import(changes))
    }
}

fn convert_changes_to_encoded<I>(mut changes: I) -> EncodedClientChanges
where
    I: Iterator<Item = Change<RemoteOp>>,
{
    let first_change = changes.next().unwrap();
    let this_client_id = first_change.id.client_id;
    let mut data = Vec::with_capacity(changes.size_hint().0 + 1);
    let mut last_change = first_change.clone();
    data.push(EncodedChange {
        ops: first_change
            .ops
            .iter()
            .map(|op| EncodedOp {
                container: op.container.clone(),
                contents: op.contents.iter().cloned().collect(),
            })
            .collect(),
        deps_except_self: first_change
            .deps
            .iter()
            .filter(|x| x.client_id != this_client_id)
            .copied()
            .collect(),
        lamport_delta: 0,
        timestamp_delta: 0,
    });
    for change in changes {
        data.push(EncodedChange {
            ops: change
                .ops
                .iter()
                .map(|op| EncodedOp {
                    container: op.container.clone(),
                    contents: op.contents.iter().cloned().collect(),
                })
                .collect(),
            deps_except_self: change
                .deps
                .iter()
                .filter(|x| x.client_id != this_client_id)
                .copied()
                .collect(),
            lamport_delta: change.lamport - last_change.lamport,
            timestamp_delta: change.timestamp - last_change.timestamp,
        });
        last_change = change;
    }

    EncodedClientChanges {
        meta: FirstChangeInfo {
            client: this_client_id,
            counter: first_change.id.counter,
            lamport: first_change.lamport,
            timestamp: first_change.timestamp,
        },
        data,
    }
}

fn convert_encoded_to_changes(changes: EncodedClientChanges) -> Vec<Change<RemoteOp>> {
    let mut result = Vec::with_capacity(changes.data.len());
    let mut last_lamport = changes.meta.lamport;
    let mut last_timestamp = changes.meta.timestamp;
    let mut counter: Counter = changes.meta.counter;
    for encoded in changes.data {
        let start_counter = counter;
        let mut deps = SmallVec::with_capacity(encoded.deps_except_self.len() + 1);
        if start_counter > 0 {
            deps.push(ID {
                client_id: changes.meta.client,
                counter: start_counter - 1,
            });
        }

        for dep in encoded.deps_except_self {
            deps.push(dep);
        }

        let mut ops = RleVec::with_capacity(encoded.ops.len());
        for op in encoded.ops {
            let len: usize = op.contents.iter().map(|x| x.atom_len()).sum();
            ops.push(RemoteOp {
                counter,
                container: op.container,
                contents: op.contents.into_iter().collect(),
            });
            counter += len as Counter;
        }

        let change = Change {
            id: ID {
                client_id: changes.meta.client,
                counter: start_counter,
            },
            lamport: last_lamport + encoded.lamport_delta,
            timestamp: last_timestamp + encoded.timestamp_delta,
            ops,
            deps,
        };
        last_lamport = change.lamport;
        last_timestamp = change.timestamp;
        result.push(change);
    }

    result
}

impl LoroCore {
    #[instrument(skip_all)]
    pub fn export_updates(&self, from: &VersionVector) -> Result<Vec<u8>, LoroError> {
        debug!("Inner Export Updates");
        match self.log_store.try_read() {
            Ok(x) => {
                debug!("get lock!");
                x.export_updates(from)
            }
            Err(_) => {
                debug!("failed to get lock Err");
                Err(LoroError::LockError)
            }
        }
    }

    pub fn import_updates(&mut self, input: &[u8]) -> Result<(), LoroError> {
        debug_log::group!("Import at {}", self.client_id());
        let ans = self.log_store.write().unwrap().import_updates(input);
        let ans = match ans {
            Ok(events) => {
                self.notify(events);
                Ok(())
            }
            Err(err) => Err(LoroError::DecodeError(err.to_string().into_boxed_str())),
        };
        debug_log::group_end!();
        ans
    }
}
