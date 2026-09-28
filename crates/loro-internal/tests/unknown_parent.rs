//! A snapshot from a newer Loro may store a child whose parent has a container
//! type unknown to this version. `parent()` used to hit `unreachable!()`.

use bytes::Bytes;
use loro_common::{ContainerID, ContainerType};
use loro_internal::{
    handler::Handler, loro::ExportMode, HandlerTrait, ListHandler, LoroDoc, ToJson,
};
use loro_kv_store::{mem_store::MemKvConfig, MemKvStore};

const UNKNOWN: ContainerType = ContainerType::Unknown(9);

/// Header of a stored container: kind, depth, parent. See
/// `ContainerWrapper::encode`.
fn header(kind: ContainerType, depth: u64, parent: &Option<ContainerID>) -> Vec<u8> {
    let mut out = vec![kind.to_u8()];
    leb128::write::unsigned(&mut out, depth).unwrap();
    postcard::to_io(parent, &mut out).unwrap();
    out
}

/// Rewrites the stored header of `child` so its parent is the root container
/// `parent`, which is added with an empty (unknown) state.
fn reparent_in_kv(section: &[u8], child: &ContainerID, parent: &ContainerID) -> Vec<u8> {
    let mut kv = MemKvStore::new(MemKvConfig::default());
    kv.import_all(Bytes::copy_from_slice(section)).unwrap();
    let key = child.to_bytes();
    let Some(value) = kv.get(&key) else {
        return section.to_vec();
    };
    let kind = ContainerType::try_from_u8(value[0]).unwrap();
    let mut reader = &value[1..];
    let depth = leb128::read::unsigned(&mut reader).unwrap();
    let (_, payload) = postcard::take_from_bytes::<Option<ContainerID>>(reader).unwrap();
    let mut new_value = header(kind, depth, &Some(parent.clone()));
    new_value.extend_from_slice(payload);
    kv.set(&key, new_value.into());
    kv.set(
        &parent.to_bytes(),
        header(parent.container_type(), 1, &None).into(),
    );
    kv.export_all().to_vec()
}

/// Rewrites every container state section of a snapshot.
fn forge_snapshot(bytes: &[u8], child: &ContainerID, parent: &ContainerID) -> Vec<u8> {
    // magic (4) + checksum (16) + encode mode (2), then the body
    let (head, mut body) = bytes.split_at(22);
    let mut sections = Vec::new();
    for _ in 0..3 {
        let len = u32::from_le_bytes(body[..4].try_into().unwrap()) as usize;
        sections.push(&body[4..4 + len]);
        body = &body[4 + len..];
    }
    assert!(body.is_empty());

    let mut out = head.to_vec();
    for (i, section) in sections.into_iter().enumerate() {
        // The oplog, and a state section of length 1 (a marker), stay as is
        let section = if i == 0 || section.len() <= 1 {
            section.to_vec()
        } else {
            reparent_in_kv(section, child, parent)
        };
        out.extend_from_slice(&(section.len() as u32).to_le_bytes());
        out.extend_from_slice(&section);
    }
    let checksum = xxhash_rust::xxh32::xxh32(&out[20..], u32::from_le_bytes(*b"LORO"));
    out[16..20].copy_from_slice(&checksum.to_le_bytes());
    out
}

#[test]
fn parent_of_a_child_of_an_unknown_container() {
    let doc = LoroDoc::new_auto_commit();
    doc.set_peer_id(1).unwrap();
    let list = doc
        .get_map("m")
        .insert_container("x", ListHandler::new_detached())
        .unwrap();
    list.push(loro_common::LoroValue::from(1)).unwrap();
    doc.commit_then_renew();
    let child = list.id();

    // The creating op is not in a shallow snapshot at the latest version, so
    // the stored header decides the parent
    let bytes = doc
        .export(ExportMode::shallow_snapshot(&doc.oplog_frontiers()))
        .unwrap();
    let parent = ContainerID::new_root("future", UNKNOWN);
    let forged = forge_snapshot(&bytes, &child, &parent);
    assert_ne!(forged, bytes);

    let doc = LoroDoc::new_auto_commit();
    doc.import(&forged).unwrap();
    let list = doc.get_list(child);
    let Some(Handler::Unknown(p)) = list.parent() else {
        panic!("expected an unknown parent, got {:?}", list.parent());
    };
    assert_eq!(p.id(), parent);
    assert_eq!(list.get_value().to_json_value(), serde_json::json!([1]));
}
