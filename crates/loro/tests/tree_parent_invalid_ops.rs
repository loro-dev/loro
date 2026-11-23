//! A tree `Create`/`Move` op whose parent node is not in the op's causal history used
//! to be applied on the import fast path without the parent check. The orphan entered
//! the tree state, and with a subscriber the event computation panicked while the doc
//! held its locks (`is_node_deleted(..).unwrap()` in `tree_state.rs`), leaving the doc
//! unusable. Such imports are now rejected with `Err` and leave the doc untouched and
//! usable, whichever way they arrive and whatever the importing doc has seen.

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use base64::Engine;
use loro::{
    ExportMode, LoroDoc, LoroError, LoroValue, Subscription, TreeID, TreeParentId, UndoManager,
    VersionVector,
};
use serde_json::{json, Value};

/// Peer 1 creates the root node `0@1` in root tree "tree" (counter 0, lamport 0).
fn base() -> LoroDoc {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    doc.get_tree("tree").create(None).unwrap();
    doc.commit();
    doc
}

const TREE: &str = "cid:root-tree:Tree";

fn forged(peers: Vec<&str>, id: &str, lamport: u64, deps: Vec<&str>, ops: Vec<Value>) -> String {
    json!({
        "schema_version": 1,
        "start_version": {},
        "peers": peers,
        "changes": [{
            "id": id,
            "timestamp": 0,
            "deps": deps,
            "lamport": lamport,
            "msg": null,
            "ops": ops,
        }]
    })
    .to_string()
}

fn tree_op(counter: u32, content: Value) -> Value {
    json!({"container": TREE, "counter": counter, "content": content})
}

/// The shapes of an op naming a parent that exists in no history.
#[derive(Clone, Copy, Debug)]
#[allow(clippy::enum_variant_names)]
enum Shape {
    /// Peer 1 extends its own head: the import fast path (`Linear`).
    CreateSamePeer,
    /// A new peer: the "greater updates" path.
    CreateNewPeer,
    /// Peer 1 moves an existing node under the unknown parent.
    MoveSamePeer,
}

const SHAPES: [Shape; 3] = [
    Shape::CreateSamePeer,
    Shape::CreateNewPeer,
    Shape::MoveSamePeer,
];

impl Shape {
    /// The change as JSON updates. The peer index 1 / 2 is the peer that holds the
    /// unknown parent id `999999@<peer>`.
    fn json(self) -> String {
        match self {
            Shape::CreateSamePeer => forged(
                vec!["1", "4242"],
                "1@0",
                1,
                vec!["0@0"],
                vec![tree_op(
                    1,
                    json!({"type": "create", "target": "1@0", "parent": "999999@1",
                           "fractional_index": "80"}),
                )],
            ),
            Shape::CreateNewPeer => forged(
                vec!["1", "2", "4242"],
                "0@1",
                1,
                vec!["0@0"],
                vec![tree_op(
                    0,
                    json!({"type": "create", "target": "0@1", "parent": "999999@2",
                           "fractional_index": "80"}),
                )],
            ),
            Shape::MoveSamePeer => forged(
                vec!["1", "4242"],
                "1@0",
                1,
                vec!["0@0"],
                vec![tree_op(
                    1,
                    json!({"type": "move", "target": "0@0", "parent": "999999@1",
                           "fractional_index": "80"}),
                )],
            ),
        }
    }

    /// The same change as a binary update on top of `base()`, and a snapshot of a doc
    /// that holds it. Both were exported by a build that accepted the change: the
    /// current build cannot produce them through its public API.
    fn blobs(self) -> (Vec<u8>, Vec<u8>) {
        let (update, snapshot) = match self {
            Shape::CreateSamePeer => (SAME_PEER_UPDATE, SAME_PEER_SNAPSHOT),
            Shape::CreateNewPeer => (NEW_PEER_UPDATE, NEW_PEER_SNAPSHOT),
            Shape::MoveSamePeer => (MOVE_UPDATE, MOVE_SNAPSHOT),
        };
        let engine = base64::engine::general_purpose::STANDARD;
        (
            engine.decode(update).unwrap(),
            engine.decode(snapshot).unwrap(),
        )
    }
}

const SAME_PEER_UPDATE: &str = "bG9ybwAAAAAAAAAAAAAAAK6zV9IABFUBAQEBARkCAQAAAAAAAACSEAAAAAAAAAABAQAAAAAABQEAAAEABgEEAQMAAAUEdHJlZQkBAgIBAAMBAYAOAQQCAQACAQACARACAQEACAABAAABv4Q9";
const SAME_PEER_SNAPSHOT: &str = "bG9ybwAAAAAAAAAAAAAAACjg3kIAA5wAAABMT1JPAAACAAIBGAIBAAAAAAAAAJIQAAAAAAAAAQEAAAAAAAUBAAABAAYBBAEDAAAFBHRyZWUJAQICAQADAQGADgEEAgQAAgQAAgQQAgQBAAwAAAABAAEAAAG/hD0AAgBmcgEBAgACAHZ2AQEEAABYAGAAAwAUw9z+AQAAAAUAAAAMAAAAAAAAAAABAAAAAAACAHZ2lijVh3kAAACMAAAATE9STwAEIk0YYECCTAAAAPkFAAIBAAR0cmVlBAAAAAkEAAEAAAAUAEEABgCDIQBBAwEAAR4AcAAAAAQCAgE6ABIFBwAFBgDwBAkBAgIBAAMBAYAAAAANACEAAwAAAAAAsBcRCAEAAAAFAAAADQAAAQAAAAAAAAAAAAAAAQYAgwR0cmVlcZYma2QAAAAAAAAA";
const NEW_PEER_UPDATE: &str = "bG9ybwAAAAAAAAAAAAAAAFQzRM4ABF8AAQEBASMDAgAAAAAAAACSEAAAAAAAAAEAAAAAAAAAAQEBAQIBAAAAAAUBAAABAAYBBAEDAAAFBHRyZWUJAQICAQADAQGADgEEAgEAAgEAAgEQAgEBAAgAAAAAAb+EPQ==";
const NEW_PEER_SNAPSHOT: &str = "bG9ybwAAAAAAAAAAAAAAAPeekiYAA8gAAABMT1JPAAQiTRhgQIKNAAAAkgABAAEBEAEBAAEAAwkA8ScFAQAAAQAGAQQBAwAABQR0cmVlCQECAgEAAwEBgA4BBAIBAAIBAAIBEAIBAQAEAAAAAQcFAAJDAGIBAQEBIwMMAEIAAJIQCQAGYAAxAQECDQAPYwAa8BYIAAAAAAG/hD0AAgBmcgECAAACAHZ2AgICAQIAAEgArwC3AAQAAAAAAPFXg0ABAAAABQAAAAwAAAAAAAAAAAEAAAAAAQIAdnZG62fcpQAAAIsAAABMT1JPAAQiTRhgQIJLAAAA9wMAAgEABHRyZWUEAAAAAQwAAgABAAgcAEEABgCDKQBTAwEAAQElAEAEAgIBQgASBQcABQYA8AQJAQICAQADAQGAAAAADQApAAMAAAAAAOUCLnMBAAAABQAAAA0AAAEAAAAAAAAAAAAAAAEGAIMEdHJlZXGWJmtjAAAAAAAAAA==";
const MOVE_UPDATE: &str = "bG9ybwAAAAAAAAAAAAAAAGeuUC4ABFUBAQEBARkCAQAAAAAAAACSEAAAAAAAAAABAQAAAAAABQEAAAEABgEEAQMAAAUEdHJlZQkBAgIBAAMBAYAOAQQCAQACAQACARACAQEACAAAAAABv4Q9";
const MOVE_SNAPSHOT: &str = "bG9ybwAAAAAAAAAAAAAAAHONYmEAA5wAAABMT1JPAAACAAIBGAIBAAAAAAAAAJIQAAAAAAAAAQEAAAAAAAUBAAABAAYBBAEDAAAFBHRyZWUJAQICAQADAQGADgEEAgQAAgQAAgQQAgQBAAwAAAABAAAAAAG/hD0AAgBmcgEBAgACAHZ2AQEEAABYAGAAAwC2bBUQAQAAAAUAAAAMAAAAAAAAAAABAAAAAAACAHZ2lijVh3kAAABjAAAATE9STwAAAgEABHRyZWUEAAAAAAYAgwR0cmVlAwEAAAQCAAAFAAAAAAEABQECAAEAAAAADQACANmL6eUBAAAABQAAAA0AAAEAAAAAAAAAAAAAAAAGAIMEdHJlZXHVz2Y7AAAAAAAAAA==";

/// A doc at `base()`, with an optional subscriber that counts events and an optional
/// local edit concurrent to every hostile change.
struct Victim {
    doc: LoroDoc,
    events: Arc<AtomicUsize>,
    _sub: Option<Subscription>,
}

impl Victim {
    fn new(subscribed: bool, concurrent_edit: bool) -> Self {
        let doc = LoroDoc::new();
        doc.set_peer_id(3).unwrap();
        doc.import(&base().export(ExportMode::Snapshot).unwrap())
            .unwrap();
        if concurrent_edit {
            doc.get_tree("tree").create(None).unwrap();
            doc.commit();
        }
        let events = Arc::new(AtomicUsize::new(0));
        let sub = subscribed.then(|| {
            let events = events.clone();
            doc.subscribe_root(Arc::new(move |_| {
                events.fetch_add(1, Ordering::SeqCst);
            }))
        });
        Self {
            doc,
            events,
            _sub: sub,
        }
    }
}

struct Before {
    value: LoroValue,
    vv: VersionVector,
    state_vv: VersionVector,
    events: usize,
}

impl Before {
    fn of(v: &Victim) -> Self {
        Self {
            value: v.doc.get_deep_value(),
            vv: v.doc.oplog_vv(),
            state_vv: v.doc.state_vv(),
            events: v.events.load(Ordering::SeqCst),
        }
    }

    fn assert_unchanged(&self, v: &Victim) {
        assert_eq!(v.doc.get_deep_value(), self.value);
        assert_eq!(v.doc.oplog_vv(), self.vv);
        assert_eq!(v.doc.state_vv(), self.state_vv);
        assert_eq!(
            v.events.load(Ordering::SeqCst),
            self.events,
            "no event fires"
        );
    }
}

/// The doc still accepts local edits, exports, and a valid remote update.
fn assert_usable(doc: &LoroDoc) {
    let tree = doc.get_tree("tree");
    let nodes = tree.nodes().len();
    let parent = tree.roots()[0];
    let local = tree.create(parent).unwrap();
    doc.commit();
    assert_eq!(tree.nodes().len(), nodes + 1);
    assert!(tree.contains(local));
    tree.get_value();
    doc.get_deep_value();

    let remote = LoroDoc::new();
    remote.set_peer_id(9).unwrap();
    remote
        .import(&doc.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    doc.export(ExportMode::updates(&VersionVector::default()))
        .unwrap();
    let remote_tree = remote.get_tree("tree");
    remote_tree.create(local).unwrap();
    remote.commit();
    doc.import(&remote.export(ExportMode::updates(&doc.oplog_vv())).unwrap())
        .unwrap();
    assert_eq!(doc.get_deep_value(), remote.get_deep_value());
}

fn assert_rejected(v: &Victim, result: Result<loro::ImportStatus, LoroError>, before: &Before) {
    let err = result.expect_err("the import must be rejected");
    assert!(
        matches!(err, LoroError::DecodeError(_)),
        "unexpected error: {err:?}"
    );
    before.assert_unchanged(v);
    if v.doc.is_detached() {
        v.doc.try_attach().unwrap();
    }
    assert_usable(&v.doc);
}

#[test]
fn update_is_rejected_with_and_without_subscriber() {
    for shape in SHAPES {
        for subscribed in [true, false] {
            for concurrent_edit in [false, true] {
                let v = Victim::new(subscribed, concurrent_edit);
                let before = Before::of(&v);
                let (update, _) = shape.blobs();
                let result = v.doc.import(&update);
                assert_rejected(&v, result, &before);
            }
        }
    }
}

#[test]
fn json_updates_are_rejected_with_and_without_subscriber() {
    for shape in SHAPES {
        for subscribed in [true, false] {
            for concurrent_edit in [false, true] {
                let v = Victim::new(subscribed, concurrent_edit);
                let before = Before::of(&v);
                let result = v.doc.import_json_updates(shape.json());
                assert_rejected(&v, result, &before);
            }
        }
    }
}

#[test]
fn snapshot_is_rejected_with_and_without_subscriber() {
    // The doc is not empty, so the snapshot is imported as changes.
    for shape in SHAPES {
        for subscribed in [true, false] {
            for concurrent_edit in [false, true] {
                let v = Victim::new(subscribed, concurrent_edit);
                let before = Before::of(&v);
                let (_, snapshot) = shape.blobs();
                let result = v.doc.import(&snapshot);
                assert_rejected(&v, result, &before);
            }
        }
    }
}

#[test]
fn rejected_when_detached() {
    for shape in SHAPES {
        let v = Victim::new(false, false);
        v.doc.detach();
        let before = Before::of(&v);
        let (update, _) = shape.blobs();
        let result = v.doc.import(&update);
        assert_rejected(&v, result, &before);

        let v = Victim::new(false, false);
        v.doc.detach();
        let before = Before::of(&v);
        let result = v.doc.import_json_updates(shape.json());
        assert_rejected(&v, result, &before);
    }
}

#[test]
fn rejected_batch_keeps_the_doc_unchanged() {
    for shape in SHAPES {
        for subscribed in [true, false] {
            let v = Victim::new(subscribed, false);
            let before = Before::of(&v);
            // A valid update from another peer, then the hostile one.
            let peer = LoroDoc::new();
            peer.set_peer_id(9).unwrap();
            peer.import(&v.doc.export(ExportMode::Snapshot).unwrap())
                .unwrap();
            peer.get_tree("tree").create(None).unwrap();
            peer.commit();
            let valid = peer.export(ExportMode::updates(&v.doc.oplog_vv())).unwrap();
            let (hostile, _) = shape.blobs();
            let result = v.doc.import_batch(&[valid, hostile]);
            assert_rejected(&v, result, &before);
        }
    }
}

#[test]
fn hostile_change_in_a_multi_change_update_rolls_everything_back() {
    let v = Victim::new(true, false);
    let before = Before::of(&v);
    // One change by peer 9: an honest create under peer 1's root node, then a create
    // under a parent that never existed.
    let json = forged(
        vec!["9", "1", "4242"],
        "0@0",
        1,
        vec!["0@1"],
        vec![
            tree_op(
                0,
                json!({"type": "create", "target": "0@0", "parent": "0@1",
                       "fractional_index": "80"}),
            ),
            tree_op(
                1,
                json!({"type": "create", "target": "1@0", "parent": "999999@2",
                       "fractional_index": "80"}),
            ),
        ],
    );
    let result = v.doc.import_json_updates(json);
    assert_rejected(&v, result, &before);
}

/// Whether the parent is acceptable is decided by the op's own causal history, never by
/// what the importing doc happens to have: the parent exists only in a change that is
/// concurrent with the hostile one.
#[test]
fn parent_known_only_through_a_concurrent_change_is_rejected_on_every_peer() {
    // Peer 2 creates `0@2` under the root; the hostile change (peer 1, depends on the
    // base only) creates a child of `0@2`.
    let peer2 = LoroDoc::new();
    peer2.set_peer_id(2).unwrap();
    peer2
        .import(&base().export(ExportMode::Snapshot).unwrap())
        .unwrap();
    let node = peer2
        .get_tree("tree")
        .create(TreeParentId::Node(TreeID::new(1, 0)))
        .unwrap();
    assert_eq!(node, TreeID::new(2, 0));
    peer2.commit();
    let concurrent = peer2
        .export(ExportMode::updates(&base().oplog_vv()))
        .unwrap();
    let hostile = forged(
        vec!["1", "2"],
        "1@0",
        1,
        vec!["0@0"],
        vec![tree_op(
            1,
            json!({"type": "create", "target": "1@0", "parent": "0@1",
                   "fractional_index": "80"}),
        )],
    );

    for subscribed in [true, false] {
        // Peer A already has the concurrent change; peer B does not.
        let a = Victim::new(subscribed, false);
        a.doc.import(&concurrent).unwrap();
        let before_a = Before::of(&a);
        let result = a.doc.import_json_updates(hostile.clone());
        assert_rejected(&a, result, &before_a);

        let b = Victim::new(subscribed, false);
        let before_b = Before::of(&b);
        let result = b.doc.import_json_updates(hostile.clone());
        assert_rejected(&b, result, &before_b);
        b.doc.import(&concurrent).unwrap();
        // Arriving in the other order changes nothing.
        let result = b.doc.import_json_updates(hostile.clone());
        assert!(result.is_err());
    }
}

/// Two peers get the same valid and hostile updates in different orders and end in
/// the same state.
#[test]
fn peers_converge_whatever_the_arrival_order() {
    let base_vv = base().oplog_vv();
    let snapshot = base().export(ExportMode::Snapshot).unwrap();

    let p2 = LoroDoc::new();
    p2.set_peer_id(2).unwrap();
    p2.import(&snapshot).unwrap();
    let n2 = p2
        .get_tree("tree")
        .create(TreeParentId::Node(TreeID::new(1, 0)))
        .unwrap();
    p2.commit();
    let valid1 = p2.export(ExportMode::updates(&base_vv)).unwrap();
    let p2_vv = p2.oplog_vv();

    let p4 = LoroDoc::new();
    p4.set_peer_id(4).unwrap();
    p4.import(&snapshot).unwrap();
    p4.import(&valid1).unwrap();
    p4.get_tree("tree").create(n2).unwrap();
    p4.commit();
    let valid2 = p4.export(ExportMode::updates(&p2_vv)).unwrap();

    let mut updates = vec![valid1, valid2];
    for shape in SHAPES {
        updates.push(shape.blobs().0);
    }

    fn permutations(items: &mut Vec<usize>, k: usize, out: &mut Vec<Vec<usize>>) {
        if k == items.len() {
            out.push(items.clone());
            return;
        }
        for i in k..items.len() {
            items.swap(k, i);
            permutations(items, k + 1, out);
            items.swap(k, i);
        }
    }
    let mut orders = Vec::new();
    permutations(&mut (0..updates.len()).collect(), 0, &mut orders);
    assert_eq!(orders.len(), 120);

    let mut expected: Option<(LoroValue, VersionVector)> = None;
    for (n, order) in orders.iter().enumerate() {
        let v = Victim::new(n % 2 == 0, false);
        for &i in order {
            let result = v.doc.import(&updates[i]);
            if i >= 2 {
                assert!(result.is_err(), "hostile update {i} must be rejected");
            } else {
                result.unwrap();
            }
        }
        let state = (v.doc.get_deep_value(), v.doc.oplog_vv());
        assert_eq!(v.doc.state_vv(), state.1);
        match &expected {
            None => expected = Some(state),
            Some(e) => assert_eq!(&state, e, "order {order:?}"),
        }
        assert_usable(&v.doc);
    }
    let (_, vv) = expected.unwrap();
    assert_eq!(vv.get(&2), Some(&1));
    assert_eq!(vv.get(&4), Some(&1));
}

/// Syncs `from` into a fresh subscribed doc and an unsubscribed one, by updates, by
/// snapshot, and by JSON when `json` is set, and checks all of them equal `from`.
fn assert_accepted_everywhere(from: &LoroDoc, json: bool) {
    let updates = from
        .export(ExportMode::updates(&VersionVector::default()))
        .unwrap();
    let snapshot = from.export(ExportMode::Snapshot).unwrap();
    for subscribed in [true, false] {
        let mut blobs = vec![updates.clone(), snapshot.clone()];
        if json {
            let schema = from
                .export_json_updates(&VersionVector::default(), &from.oplog_vv())
                .unwrap();
            let doc = LoroDoc::new();
            doc.set_peer_id(77).unwrap();
            let _sub = subscribed.then(|| doc.subscribe_root(Arc::new(|_| {})));
            doc.import_json_updates(schema).unwrap();
            assert_eq!(doc.get_deep_value(), from.get_deep_value());
        }
        for blob in blobs.drain(..) {
            let doc = LoroDoc::new();
            doc.set_peer_id(77).unwrap();
            let _sub = subscribed.then(|| doc.subscribe_root(Arc::new(|_| {})));
            doc.import(&blob).unwrap();
            assert_eq!(doc.get_deep_value(), from.get_deep_value());
            // And through the non-empty path.
            let doc = LoroDoc::new();
            doc.set_peer_id(78).unwrap();
            let _sub = subscribed.then(|| doc.subscribe_root(Arc::new(|_| {})));
            doc.get_tree("other").create(None).unwrap();
            doc.commit();
            doc.import(&blob).unwrap();
            assert_eq!(
                doc.get_tree("tree").get_value(),
                from.get_tree("tree").get_value()
            );
        }
    }
}

#[test]
fn parent_created_earlier_in_the_same_change_is_accepted() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let tree = doc.get_tree("tree");
    let a = tree.create(None).unwrap();
    let b = tree.create(a).unwrap();
    let c = tree.create(b).unwrap();
    // Create, then move under a node created in the same change.
    let d = tree.create(None).unwrap();
    tree.mov(d, c).unwrap();
    doc.commit();
    assert_accepted_everywhere(&doc, true);
}

#[test]
fn parent_created_by_another_peer_earlier_in_history_is_accepted() {
    let doc1 = base();
    let doc2 = LoroDoc::new();
    doc2.set_peer_id(2).unwrap();
    doc2.import(&doc1.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    let n2 = doc2
        .get_tree("tree")
        .create(TreeParentId::Node(TreeID::new(1, 0)))
        .unwrap();
    doc2.commit();
    let doc3 = LoroDoc::new();
    doc3.set_peer_id(3).unwrap();
    doc3.import(&doc2.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    let n3 = doc3.get_tree("tree").create(n2).unwrap();
    doc3.commit();
    // Peer 1 moves its own node under a node created by peer 3.
    doc1.import(&doc3.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    doc1.get_tree("tree").create(n3).unwrap();
    doc1.commit();
    assert_accepted_everywhere(&doc1, true);
}

#[test]
fn moves_to_root_and_into_deleted_nodes_are_accepted() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let tree = doc.get_tree("tree");
    let a = tree.create(None).unwrap();
    let b = tree.create(a).unwrap();
    let c = tree.create(b).unwrap();
    tree.mov(c, None).unwrap();
    tree.mov(c, b).unwrap();
    doc.commit();
    tree.delete(a).unwrap();
    doc.commit();
    // A node moved under a deleted parent keeps its current semantics.
    let _moved_into_deleted = tree.mov(c, a);
    doc.commit();
    assert_accepted_everywhere(&doc, true);
}

#[test]
fn concurrent_delete_of_the_parent_is_accepted() {
    let doc1 = base();
    let doc2 = LoroDoc::new();
    doc2.set_peer_id(2).unwrap();
    doc2.import(&doc1.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    let doc3 = LoroDoc::new();
    doc3.set_peer_id(3).unwrap();
    doc3.import(&doc1.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    let root = TreeID::new(1, 0);

    // Peer 2 deletes the parent while peer 3 creates a child in it.
    doc2.get_tree("tree").delete(root).unwrap();
    doc2.commit();
    doc3.get_tree("tree").create(root).unwrap();
    doc3.commit();

    for subscribed in [true, false] {
        let docs = [&doc2, &doc3];
        let fresh: Vec<LoroDoc> = docs
            .iter()
            .map(|d| {
                let f = LoroDoc::new();
                f.set_peer_id(50 + f.peer_id() % 7).unwrap();
                f.import(&doc1.export(ExportMode::Snapshot).unwrap())
                    .unwrap();
                f.import(&d.export(ExportMode::updates(&doc1.oplog_vv())).unwrap())
                    .unwrap();
                f
            })
            .collect();
        let subs: Vec<Subscription> = if subscribed {
            fresh
                .iter()
                .map(|f| f.subscribe_root(Arc::new(|_| {})))
                .collect()
        } else {
            vec![]
        };
        // Cross-import in both directions.
        fresh[0]
            .import(&doc3.export(ExportMode::updates(&doc1.oplog_vv())).unwrap())
            .unwrap();
        fresh[1]
            .import(&doc2.export(ExportMode::updates(&doc1.oplog_vv())).unwrap())
            .unwrap();
        assert_eq!(fresh[0].get_deep_value(), fresh[1].get_deep_value());
        drop(subs);
    }
}

/// A node revived by undo gets a new `Create` op that carries its old id. A later
/// change naming it as its parent is honest.
#[test]
fn parent_revived_by_undo_is_accepted() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let mut undo = UndoManager::new(&doc);
    let tree = doc.get_tree("tree");
    let a = tree.create(None).unwrap();
    let b = tree.create(a).unwrap();
    doc.commit();
    tree.delete(a).unwrap();
    doc.commit();
    assert!(undo.undo().unwrap());
    assert!(tree.contains(b));
    let child = tree.create(b).unwrap();
    doc.commit();
    tree.create(a).unwrap();
    doc.commit();
    assert_eq!(tree.parent(child), Some(TreeParentId::Node(b)));
    assert_accepted_everywhere(&doc, false);
}

/// `apply_diff` creates nodes with ids of its own ops.
#[test]
fn parent_created_by_apply_diff_is_accepted() {
    let src = LoroDoc::new();
    src.set_peer_id(1).unwrap();
    let tree = src.get_tree("tree");
    let n1 = tree.create(None).unwrap();
    tree.create(n1).unwrap();
    src.commit();
    let diff = src
        .diff(&Default::default(), &src.state_frontiers())
        .unwrap();

    let doc = LoroDoc::new();
    doc.set_peer_id(20).unwrap();
    doc.apply_diff(diff).unwrap();
    doc.commit();
    let tree = doc.get_tree("tree");
    let created = tree.nodes()[1];
    // A separate change, so the parent is resolved through the history.
    tree.create(created).unwrap();
    doc.commit();
    assert_accepted_everywhere(&doc, false);
}

/// A parent created before the shallow root is only known through the root state.
#[test]
fn parent_before_a_shallow_root_is_accepted_and_an_unknown_one_is_rejected() {
    let src = base();
    let tree = src.get_tree("tree");
    let root = TreeID::new(1, 0);
    let kept = tree.create(root).unwrap();
    let deleted = tree.create(root).unwrap();
    tree.delete(deleted).unwrap();
    src.commit();
    let shallow = src
        .export(ExportMode::shallow_snapshot(&src.oplog_frontiers()))
        .unwrap();

    let doc = LoroDoc::new();
    doc.set_peer_id(5).unwrap();
    doc.import(&shallow).unwrap();
    let _sub = doc.subscribe_root(Arc::new(|_| {}));

    // Honest changes after the shallow root, under nodes created before it.
    let editor = LoroDoc::new();
    editor.set_peer_id(6).unwrap();
    editor
        .import(&src.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    editor.get_tree("tree").create(kept).unwrap();
    editor.get_tree("tree").create(root).unwrap();
    editor.commit();
    doc.import(&editor.export(ExportMode::updates(&src.oplog_vv())).unwrap())
        .unwrap();
    assert_eq!(
        doc.get_tree("tree").get_value(),
        editor.get_tree("tree").get_value()
    );

    // The same change shape under a parent that never existed.
    let last = doc.oplog_frontiers();
    let counter = doc.oplog_vv().get(&6).copied().unwrap_or(0);
    let hostile = forged(
        vec!["6", "4242"],
        &format!("{counter}@0"),
        100,
        vec![&format!("{}@{}", counter - 1, "0")],
        vec![tree_op(
            counter as u32,
            json!({"type": "create", "target": format!("{counter}@0"),
                   "parent": "999999@1", "fractional_index": "80"}),
        )],
    );
    let before = doc.get_deep_value();
    let err = doc.import_json_updates(hostile).unwrap_err();
    assert!(matches!(err, LoroError::DecodeError(_)), "{err:?}");
    assert_eq!(doc.get_deep_value(), before);
    assert_eq!(doc.oplog_frontiers(), last);
    doc.get_tree("tree").create(root).unwrap();
    doc.commit();
}

/// Builds before loro-dev/loro#468 made `apply_diff` (and so undo) re-create a node
/// that the doc had never seen with the diff's own `TreeID`, by a `Create` whose op id
/// is not that id. The encoding stores it as a `Move` of a target with no `Create` in
/// history. A node is therefore also known through any earlier `Create`/`Move` on it,
/// not only through the op that has its id.
///
/// No subscriber: the event of such a `Move` panics on its own, before and after the
/// parent check (its metadata container has no known parent).
fn legacy_reused_target_doc() -> LoroDoc {
    let doc = LoroDoc::new();
    doc.set_peer_id(7).unwrap();
    // Peer 2 brings back node `5@99`, whose original `Create` it never had.
    let revive = forged(
        vec!["2", "99"],
        "0@0",
        0,
        vec![],
        vec![tree_op(
            0,
            json!({"type": "move", "target": "5@1", "parent": null,
                   "fractional_index": "80"}),
        )],
    );
    doc.import_json_updates(revive).unwrap();
    // A later change of peer 2 creates a child under it.
    let child = forged(
        vec!["2", "99"],
        "1@0",
        1,
        vec!["0@0"],
        vec![tree_op(
            1,
            json!({"type": "create", "target": "1@0", "parent": "5@1",
                   "fractional_index": "80"}),
        )],
    );
    doc.import_json_updates(child).unwrap();
    assert_eq!(
        doc.get_tree("tree").parent(TreeID::new(2, 1)),
        Some(TreeParentId::Node(TreeID::new(99, 5)))
    );
    doc
}

/// Imports `from` into fresh docs by updates, snapshot, JSON, a batch, and detached,
/// without subscribers, and checks they equal `from`.
fn assert_accepted_unsubscribed(from: &LoroDoc) {
    let updates = from
        .export(ExportMode::updates(&VersionVector::default()))
        .unwrap();
    let snapshot = from.export(ExportMode::Snapshot).unwrap();
    let json = from
        .export_json_updates(&VersionVector::default(), &from.oplog_vv())
        .unwrap();
    let fresh = |peer| {
        let doc = LoroDoc::new();
        doc.set_peer_id(peer).unwrap();
        doc
    };
    let check = |doc: &LoroDoc| {
        assert_eq!(
            doc.get_tree("tree").get_value(),
            from.get_tree("tree").get_value()
        )
    };
    for blob in [&updates, &snapshot] {
        let doc = fresh(77);
        doc.import(blob).unwrap();
        check(&doc);
        let doc = fresh(78);
        doc.get_tree("other").create(None).unwrap();
        doc.commit();
        doc.import(blob).unwrap();
        check(&doc);
        let doc = fresh(79);
        doc.detach();
        doc.import(blob).unwrap();
        doc.try_checkout_to_latest().unwrap();
        check(&doc);
    }
    let doc = fresh(80);
    doc.import_json_updates(json).unwrap();
    check(&doc);
    let doc = fresh(81);
    doc.import_batch(std::slice::from_ref(&updates)).unwrap();
    check(&doc);
    // One op at a time, in reverse: everything waits as pending until the first.
    let doc = fresh(82);
    let mut parts = Vec::new();
    for (&peer, &end) in from.oplog_vv().iter() {
        for c in 0..end {
            parts.push(
                from.export(ExportMode::updates_in_range(vec![loro::IdSpan::new(
                    peer,
                    c,
                    c + 1,
                )]))
                .unwrap(),
            );
        }
    }
    for part in parts.iter().rev() {
        doc.import(part).unwrap();
    }
    check(&doc);
}

#[test]
fn parent_known_only_through_an_earlier_op_on_it_is_accepted() {
    let doc = legacy_reused_target_doc();
    assert_accepted_unsubscribed(&doc);
    // A third peer, which has the history, moves a node under it and creates one.
    let editor = LoroDoc::new();
    editor.set_peer_id(8).unwrap();
    editor
        .import(&doc.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    let tree = editor.get_tree("tree");
    let n = tree.create(TreeID::new(99, 5)).unwrap();
    tree.mov(TreeID::new(2, 1), n).unwrap();
    editor.commit();
    assert_accepted_unsubscribed(&editor);
    doc.import(&editor.export(ExportMode::updates(&doc.oplog_vv())).unwrap())
        .unwrap();
    assert_eq!(doc.get_deep_value(), editor.get_deep_value());
}

/// An op on the parent that the naming op cannot see does not make it valid.
#[test]
fn parent_known_only_through_a_concurrent_op_on_it_is_rejected() {
    let doc = legacy_reused_target_doc();
    // Peer 3, concurrent to all of peer 2, names `5@99` as its parent.
    let hostile = forged(
        vec!["3", "99"],
        "0@0",
        0,
        vec![],
        vec![tree_op(
            0,
            json!({"type": "create", "target": "0@0", "parent": "5@1",
                   "fractional_index": "80"}),
        )],
    );
    let before = doc.get_deep_value();
    let vv = doc.oplog_vv();
    let err = doc.import_json_updates(hostile).unwrap_err();
    assert!(matches!(err, LoroError::DecodeError(_)), "{err:?}");
    assert_eq!(doc.get_deep_value(), before);
    assert_eq!(doc.oplog_vv(), vv);
    doc.get_tree("tree").create(None).unwrap();
    doc.commit();
}

/// Undo, redo and `revert_to` bring deleted nodes back with ops whose ids are not the
/// nodes' ids (`TreeHandler::revive_for_apply_diff`). Changes under the revived nodes,
/// and under their revived children, are honest on every peer.
#[test]
fn parents_revived_by_undo_redo_and_revert_are_accepted() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let mut undo = UndoManager::new(&doc);
    let tree = doc.get_tree("tree");
    let a = tree.create(None).unwrap();
    let b = tree.create(a).unwrap();
    let c = tree.create(b).unwrap();
    doc.commit();
    let kept = doc.state_frontiers();
    tree.delete(a).unwrap();
    doc.commit();
    assert!(undo.undo().unwrap());
    assert!(undo.redo().unwrap());
    assert!(undo.undo().unwrap());
    tree.create(c).unwrap();
    doc.commit();
    tree.delete(a).unwrap();
    doc.commit();
    doc.revert_to(&kept).unwrap();
    doc.commit();
    assert!(tree.contains(c));

    // Another peer that has only the exported history writes under them.
    let other = LoroDoc::new();
    other.set_peer_id(2).unwrap();
    other
        .import(
            &doc.export(ExportMode::updates(&VersionVector::default()))
                .unwrap(),
        )
        .unwrap();
    other.get_tree("tree").create(b).unwrap();
    other.get_tree("tree").mov(a, c).unwrap_err();
    other.get_tree("tree").create(c).unwrap();
    other.commit();
    assert_accepted_everywhere(&doc, true);
    assert_accepted_everywhere(&other, true);
    doc.import(&other.export(ExportMode::updates(&doc.oplog_vv())).unwrap())
        .unwrap();
    assert_eq!(doc.get_deep_value(), other.get_deep_value());
}

/// A `Delete` of a node that exists in no history used to panic in the event
/// conversion (`is_node_deleted(&target).unwrap()` in `tree_state.rs`). It is rejected
/// like an unknown parent.
#[test]
fn delete_of_an_unknown_node_is_rejected() {
    for (subscribed, detached) in [(true, false), (false, false), (false, true)] {
        let v = Victim::new(subscribed, false);
        if detached {
            v.doc.detach();
        }
        let before = Before::of(&v);
        let hostile = forged(
            vec!["1", "4242"],
            "1@0",
            1,
            vec!["0@0"],
            vec![tree_op(1, json!({"type": "delete", "target": "999999@1"}))],
        );
        let err = v.doc.import_json_updates(hostile).unwrap_err();
        assert!(matches!(err, LoroError::DecodeError(_)), "{err:?}");
        before.assert_unchanged(&v);
        if v.doc.is_detached() {
            v.doc.try_attach().unwrap();
        }
        assert_usable(&v.doc);
    }
}

/// Deletes of nodes created earlier in the change, by another peer, or before a
/// shallow root are honest.
#[test]
fn deletes_of_known_nodes_are_accepted() {
    let src = base();
    let root = TreeID::new(1, 0);
    let tree = src.get_tree("tree");
    let a = tree.create(root).unwrap();
    let b = tree.create(root).unwrap();
    tree.delete(b).unwrap();
    src.commit();
    let shallow = src
        .export(ExportMode::shallow_snapshot(&src.oplog_frontiers()))
        .unwrap();

    let editor = LoroDoc::new();
    editor.set_peer_id(6).unwrap();
    editor
        .import(&src.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    let tree = editor.get_tree("tree");
    tree.delete(a).unwrap();
    let c = tree.create(None).unwrap();
    tree.delete(c).unwrap();
    editor.commit();
    assert_accepted_everywhere(&editor, true);

    let doc = LoroDoc::new();
    doc.set_peer_id(5).unwrap();
    doc.import(&shallow).unwrap();
    let _sub = doc.subscribe_root(Arc::new(|_| {}));
    doc.import(&editor.export(ExportMode::updates(&src.oplog_vv())).unwrap())
        .unwrap();
    assert_eq!(
        doc.get_tree("tree").get_value(),
        editor.get_tree("tree").get_value()
    );
}
