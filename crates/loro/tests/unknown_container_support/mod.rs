//! Forging containers of a type unknown to this version (written by a newer
//! Loro) for tests: Counters stand in for them, and their ids and ops are
//! rewritten to `Unknown(9)`.

#![allow(dead_code)]

use loro::{
    ContainerID, ContainerType, JsonFutureOp, JsonListOp, JsonMapOp, JsonMovableListOp,
    JsonOpContent, LoroCounter, LoroDoc, LoroError, LoroResult, LoroValue, ToJson, ID,
};

pub const UNKNOWN: ContainerType = ContainerType::Unknown(9);

pub fn forge_value(v: &mut LoroValue) {
    match v {
        LoroValue::Container(c) => forge_id(c),
        LoroValue::List(l) => l.make_mut().iter_mut().for_each(forge_value),
        LoroValue::Map(m) => m.make_mut().values_mut().for_each(forge_value),
        _ => {}
    }
}

pub fn forge_id(id: &mut ContainerID) {
    if id.container_type() == ContainerType::Counter {
        *id = match id {
            ContainerID::Root { name, .. } => ContainerID::new_root(name, UNKNOWN),
            ContainerID::Normal { peer, counter, .. } => {
                ContainerID::new_normal(ID::new(*peer, *counter), UNKNOWN)
            }
        };
    }
}

/// Replays `build`'s history into a new doc with every Counter turned into an
/// `Unknown(9)` container, as if a newer Loro had written it. Mergeable
/// markers survive because the typed JSON is edited in memory.
pub fn forge(build: impl FnOnce(&LoroDoc)) -> LoroDoc {
    forge_as(build, true)
}

pub fn forge_as(build: impl FnOnce(&LoroDoc), unknown: bool) -> LoroDoc {
    let src = LoroDoc::new();
    src.set_peer_id(1).unwrap();
    build(&src);
    src.commit();
    let mut json = src
        .export_json_updates_without_peer_compression(&Default::default(), &src.oplog_vv())
        .unwrap();
    for change in json.changes.iter_mut().filter(|_| unknown) {
        for op in change.ops.iter_mut() {
            forge_id(&mut op.container);
            match &mut op.content {
                JsonOpContent::Future(f) => {
                    if let JsonFutureOp::Counter(v) = &f.value {
                        f.value = JsonFutureOp::Unknown(v.clone());
                    }
                }
                JsonOpContent::Map(JsonMapOp::Insert { value, .. }) => forge_value(value),
                JsonOpContent::List(JsonListOp::Insert { value, .. })
                | JsonOpContent::MovableList(JsonMovableListOp::Insert { value, .. }) => {
                    value.iter_mut().for_each(forge_value)
                }
                JsonOpContent::MovableList(JsonMovableListOp::Set { value, .. }) => {
                    forge_value(value)
                }
                _ => {}
            }
        }
    }
    let doc = LoroDoc::new();
    doc.set_peer_id(2).unwrap();
    doc.import_json_updates(json).unwrap();
    doc
}

pub fn counter() -> LoroCounter {
    LoroCounter::new()
}

pub fn edited_counter(c: LoroCounter) {
    c.increment(1.0).unwrap();
}

pub fn state(doc: &LoroDoc) -> (serde_json::Value, loro::VersionVector) {
    doc.commit();
    (doc.get_deep_value().to_json_value(), doc.oplog_vv())
}

pub fn is_unknown_err(e: &LoroError) -> bool {
    matches!(e, LoroError::ArgErr(msg) if msg.contains("Unknown(9)") && msg.contains("unknown to this version"))
}

/// Runs `f` and checks it either succeeds or fails without changing `doc`.
/// Returns whether it failed on an unknown container.
pub fn atomic<T>(doc: &LoroDoc, f: impl FnOnce(&LoroDoc) -> LoroResult<T>) -> bool {
    let before = state(doc);
    match f(doc) {
        Ok(_) => false,
        Err(e) => {
            assert_eq!(state(doc), before, "partially applied before {e:?}");
            is_unknown_err(&e)
        }
    }
}
