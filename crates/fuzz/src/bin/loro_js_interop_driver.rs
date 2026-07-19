use std::{error::Error, io};

use loro::{
    ExportMode, Frontiers, LoroDoc, LoroValue, ToJson, TreeID, TreeParentId, VersionVector,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const SLOT_COUNT: usize = 8;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DriverInput {
    scenario: Scenario,
    #[serde(default)]
    external_blobs: Option<Vec<Vec<u8>>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Scenario {
    schema_version: u8,
    peer_count: usize,
    commands: Vec<Command>,
}

#[derive(Debug, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
enum Command {
    MapSet {
        peer: i64,
        key: String,
        value: Value,
    },
    MapDelete {
        peer: i64,
        key: String,
    },
    ListInsert {
        peer: i64,
        index: i64,
        value: Value,
    },
    ListDelete {
        peer: i64,
        index: i64,
        length: i64,
    },
    TextInsert {
        peer: i64,
        index: i64,
        text: String,
    },
    TextDelete {
        peer: i64,
        index: i64,
        length: i64,
    },
    TextMark {
        peer: i64,
        index: i64,
        length: i64,
        key: String,
        value: Value,
    },
    TextUnmark {
        peer: i64,
        index: i64,
        length: i64,
        key: String,
    },
    MovableInsert {
        peer: i64,
        index: i64,
        value: Value,
    },
    MovableDelete {
        peer: i64,
        index: i64,
        length: i64,
    },
    MovableSet {
        peer: i64,
        index: i64,
        value: Value,
    },
    MovableMove {
        peer: i64,
        from: i64,
        to: i64,
    },
    CounterIncrement {
        peer: i64,
        delta: i64,
    },
    TreeCreate {
        peer: i64,
        parent: Option<i64>,
        value: Value,
    },
    TreeMetaSet {
        peer: i64,
        node: i64,
        key: String,
        value: Value,
    },
    TreeMove {
        peer: i64,
        node: i64,
        parent: Option<i64>,
    },
    TreeDelete {
        peer: i64,
        node: i64,
    },
    Commit {
        peer: i64,
        message: i64,
    },
    Enqueue {
        source: i64,
        target: i64,
        slot: i64,
        mode: TransportMode,
    },
    Deliver {
        slot: i64,
        copies: i64,
    },
    Save {
        peer: i64,
        checkpoint: i64,
    },
    Checkout {
        peer: i64,
        checkpoint: i64,
    },
    Attach {
        peer: i64,
    },
    Roundtrip {
        peer: i64,
        mode: TransportMode,
    },
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum TransportMode {
    Update,
    Snapshot,
}

#[derive(Debug, Clone)]
struct Slot {
    blob: Vec<u8>,
    target: usize,
}

struct World {
    docs: Vec<LoroDoc>,
    slots: Vec<Option<Slot>>,
    checkpoints: Vec<Vec<Option<Frontiers>>>,
    external_blobs: Option<Vec<Vec<u8>>>,
    transport_blobs: Vec<Vec<u8>>,
    enqueue_index: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DriverOutput {
    observations: Vec<Observation>,
    transport_blobs: Vec<Vec<u8>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Observation {
    json: Value,
    deep_with_id: Value,
    version: Vec<(String, i32)>,
    oplog_version: Vec<(String, i32)>,
    frontiers: Vec<String>,
    oplog_frontiers: Vec<String>,
    detached: bool,
    shallow: bool,
    op_count: usize,
    change_count: usize,
}

fn main() -> Result<(), Box<dyn Error>> {
    let input: DriverInput = serde_json::from_reader(io::stdin().lock())?;
    if input.scenario.schema_version != 1 {
        return Err(format!(
            "unsupported scenario schema {}",
            input.scenario.schema_version
        )
        .into());
    }
    if input.scenario.peer_count < 2 {
        return Err("scenario peerCount must be at least two".into());
    }

    let mut world = World::new(input.scenario.peer_count, input.external_blobs);
    for command in &input.scenario.commands {
        world.execute(command);
    }
    let output = DriverOutput {
        observations: world.docs.iter().map(observe_doc).collect(),
        transport_blobs: world.transport_blobs,
    };
    serde_json::to_writer(io::stdout().lock(), &output)?;
    Ok(())
}

impl World {
    fn new(peer_count: usize, external_blobs: Option<Vec<Vec<u8>>>) -> Self {
        let docs = (0..peer_count)
            .map(|peer| {
                let doc = LoroDoc::new();
                doc.set_peer_id(peer as u64 + 1).unwrap();
                doc.set_record_timestamp(false);
                doc.set_change_merge_interval(0);
                doc.get_map("map");
                doc.get_list("list");
                doc.get_text("text");
                doc.get_movable_list("movable");
                doc.get_counter("counter");
                doc.get_tree("tree").enable_fractional_index(0);
                doc
            })
            .collect();
        Self {
            docs,
            slots: vec![None; SLOT_COUNT],
            checkpoints: vec![vec![None; SLOT_COUNT]; peer_count],
            external_blobs,
            transport_blobs: Vec::new(),
            enqueue_index: 0,
        }
    }

    fn execute(&mut self, command: &Command) {
        match command {
            Command::MapSet { peer, key, value } => {
                let _ = self
                    .doc(*peer)
                    .get_map("map")
                    .insert(key, loro_value(value));
            }
            Command::MapDelete { peer, key } => {
                let map = self.doc(*peer).get_map("map");
                let mut keys = map.keys().map(|key| key.to_string()).collect::<Vec<_>>();
                if !keys.is_empty() {
                    keys.sort();
                    let selected = if keys.iter().any(|candidate| candidate == key) {
                        key
                    } else {
                        &keys[0]
                    };
                    let _ = map.delete(selected);
                }
            }
            Command::ListInsert { peer, index, value } => {
                let list = self.doc(*peer).get_list("list");
                let _ = list.insert(modulo(*index, list.len() + 1), loro_value(value));
            }
            Command::ListDelete {
                peer,
                index,
                length,
            } => {
                let list = self.doc(*peer).get_list("list");
                if !list.is_empty() {
                    let index = modulo(*index, list.len());
                    let length = 1 + modulo(*length, list.len() - index);
                    let _ = list.delete(index, length);
                }
            }
            Command::TextInsert { peer, index, text } => {
                let text_container = self.doc(*peer).get_text("text");
                let boundaries = utf16_boundaries(&text_container.to_string());
                let _ =
                    text_container.insert_utf16(boundaries[modulo(*index, boundaries.len())], text);
            }
            Command::TextDelete {
                peer,
                index,
                length,
            } => {
                let text = self.doc(*peer).get_text("text");
                let (start, end) = selected_text_range(&text.to_string(), *index, *length);
                if start != end {
                    let _ = text.delete_utf16(start, end - start);
                }
            }
            Command::TextMark {
                peer,
                index,
                length,
                key,
                value,
            } => {
                let text = self.doc(*peer).get_text("text");
                let (start, end) = selected_text_range(&text.to_string(), *index, *length);
                if start != end {
                    let _ = text.mark_utf16(start..end, key, loro_value(value));
                }
            }
            Command::TextUnmark {
                peer,
                index,
                length,
                key,
            } => {
                let text = self.doc(*peer).get_text("text");
                let (start, end) = selected_text_range(&text.to_string(), *index, *length);
                if start != end {
                    let _ = text.unmark_utf16(start..end, key);
                }
            }
            Command::MovableInsert { peer, index, value } => {
                let list = self.doc(*peer).get_movable_list("movable");
                let _ = list.insert(modulo(*index, list.len() + 1), loro_value(value));
            }
            Command::MovableDelete {
                peer,
                index,
                length,
            } => {
                let list = self.doc(*peer).get_movable_list("movable");
                if !list.is_empty() {
                    let index = modulo(*index, list.len());
                    let length = 1 + modulo(*length, list.len() - index);
                    let _ = list.delete(index, length);
                }
            }
            Command::MovableSet { peer, index, value } => {
                let list = self.doc(*peer).get_movable_list("movable");
                if !list.is_empty() {
                    let index = modulo(*index, list.len());
                    let current = list
                        .get(index)
                        .map(|current| current.get_deep_value().to_json_value());
                    if current.as_ref() != Some(value) {
                        let _ = list.set(index, loro_value(value));
                    }
                }
            }
            Command::MovableMove { peer, from, to } => {
                let list = self.doc(*peer).get_movable_list("movable");
                if list.len() >= 2 {
                    let _ = list.mov(modulo(*from, list.len()), modulo(*to, list.len()));
                }
            }
            Command::CounterIncrement { peer, delta } => {
                if *delta != 0 {
                    let _ = self
                        .doc(*peer)
                        .get_counter("counter")
                        .increment(*delta as f64);
                }
            }
            Command::TreeCreate {
                peer,
                parent,
                value,
            } => {
                let tree = self.doc(*peer).get_tree("tree");
                let nodes = live_tree_nodes(&tree);
                let parent = parent
                    .and_then(|raw| nodes.get(modulo(raw, nodes.len())).copied())
                    .map(TreeParentId::Node)
                    .unwrap_or(TreeParentId::Root);
                if let Ok(node) = tree.create(parent) {
                    if let Ok(meta) = tree.get_meta(node) {
                        let _ = meta.insert("value", loro_value(value));
                    }
                }
            }
            Command::TreeMetaSet {
                peer,
                node,
                key,
                value,
            } => {
                let tree = self.doc(*peer).get_tree("tree");
                let nodes = live_tree_nodes(&tree);
                if let Some(node) = nodes.get(modulo(*node, nodes.len())).copied() {
                    if let Ok(meta) = tree.get_meta(node) {
                        let _ = meta.insert(key, loro_value(value));
                    }
                }
            }
            Command::TreeMove { peer, node, parent } => {
                let tree = self.doc(*peer).get_tree("tree");
                let nodes = live_tree_nodes(&tree);
                if let Some(target) = nodes.get(modulo(*node, nodes.len())).copied() {
                    let candidates = nodes
                        .iter()
                        .copied()
                        .filter(|candidate| {
                            candidate != &target && !is_tree_descendant(&tree, *candidate, target)
                        })
                        .collect::<Vec<_>>();
                    let selected_parent = parent
                        .and_then(|raw| candidates.get(modulo(raw, candidates.len())).copied())
                        .map(TreeParentId::Node)
                        .unwrap_or(TreeParentId::Root);
                    if tree.parent(target).as_ref() != Some(&selected_parent) {
                        let _ = tree.mov(target, selected_parent);
                    }
                }
            }
            Command::TreeDelete { peer, node } => {
                let tree = self.doc(*peer).get_tree("tree");
                let nodes = live_tree_nodes(&tree);
                if let Some(node) = nodes.get(modulo(*node, nodes.len())).copied() {
                    let _ = tree.delete(node);
                }
            }
            Command::Commit { peer, message } => {
                let doc = self.doc(*peer);
                doc.set_next_commit_message(&format!("interop-fuzz-{message}"));
                doc.commit();
            }
            Command::Enqueue {
                source,
                target,
                slot,
                mode,
            } => self.enqueue(*source, *target, *slot, *mode),
            Command::Deliver { slot, copies } => self.deliver(*slot, *copies),
            Command::Save { peer, checkpoint } => {
                let peer = peer_index(*peer, self.docs.len());
                self.checkpoints[peer][modulo(*checkpoint, SLOT_COUNT)] =
                    Some(self.docs[peer].state_frontiers());
            }
            Command::Checkout { peer, checkpoint } => {
                let peer = peer_index(*peer, self.docs.len());
                if let Some(frontiers) =
                    self.checkpoints[peer][modulo(*checkpoint, SLOT_COUNT)].as_ref()
                {
                    if &self.docs[peer].state_frontiers() != frontiers {
                        let _ = self.docs[peer].checkout(frontiers);
                    }
                }
            }
            Command::Attach { peer } => self.doc(*peer).attach(),
            Command::Roundtrip { peer, mode } => {
                let doc = self.doc(*peer);
                let bytes = match mode {
                    TransportMode::Update => doc.export(ExportMode::all_updates()),
                    TransportMode::Snapshot => doc.export(ExportMode::Snapshot),
                };
                if let Ok(bytes) = bytes {
                    let imported = LoroDoc::new();
                    let _ = imported.import(&bytes);
                }
            }
        }
    }

    fn enqueue(&mut self, source: i64, target: i64, slot: i64, mode: TransportMode) {
        let source = peer_index(source, self.docs.len());
        let mut target = peer_index(target, self.docs.len());
        if source == target {
            target = (target + 1) % self.docs.len();
        }
        let own_blob = match mode {
            TransportMode::Update => {
                self.docs[source].export(ExportMode::updates(&self.docs[target].oplog_vv()))
            }
            TransportMode::Snapshot => self.docs[source].export(ExportMode::Snapshot),
        };
        let Ok(own_blob) = own_blob else {
            return;
        };
        let blob = self
            .external_blobs
            .as_ref()
            .and_then(|blobs| blobs.get(self.enqueue_index))
            .cloned()
            .unwrap_or_else(|| own_blob.clone());
        self.transport_blobs.push(own_blob);
        self.enqueue_index += 1;
        self.slots[modulo(slot, SLOT_COUNT)] = Some(Slot { blob, target });
    }

    fn deliver(&mut self, slot: i64, copies: i64) {
        let Some(slot) = self.slots[modulo(slot, SLOT_COUNT)].clone() else {
            return;
        };
        let copies = 1 + modulo(copies, 3);
        for _ in 0..copies {
            let _ = self.docs[slot.target].import(&slot.blob);
        }
    }

    fn doc(&self, peer: i64) -> &LoroDoc {
        &self.docs[peer_index(peer, self.docs.len())]
    }
}

fn observe_doc(doc: &LoroDoc) -> Observation {
    Observation {
        json: doc.get_deep_value().to_json_value(),
        deep_with_id: doc.get_deep_value_with_id().to_json_value(),
        version: version_pairs(doc.state_vv()),
        oplog_version: version_pairs(doc.oplog_vv()),
        frontiers: frontier_strings(doc.state_frontiers()),
        oplog_frontiers: frontier_strings(doc.oplog_frontiers()),
        detached: doc.is_detached(),
        shallow: doc.is_shallow(),
        op_count: doc.len_ops(),
        change_count: doc.len_changes(),
    }
}

fn version_pairs(version: VersionVector) -> Vec<(String, i32)> {
    let mut pairs = version
        .iter()
        .map(|(peer, counter)| (peer.to_string(), *counter))
        .collect::<Vec<_>>();
    pairs.sort_by(|left, right| left.0.cmp(&right.0));
    pairs
}

fn frontier_strings(frontiers: Frontiers) -> Vec<String> {
    let mut values = frontiers
        .iter()
        .map(|id| format!("{}@{}", id.counter, id.peer))
        .collect::<Vec<_>>();
    values.sort();
    values
}

fn live_tree_nodes(tree: &loro::LoroTree) -> Vec<TreeID> {
    let mut nodes = tree
        .get_nodes(false)
        .into_iter()
        .map(|node| node.id)
        .collect::<Vec<_>>();
    nodes.sort_by_key(ToString::to_string);
    nodes
}

fn is_tree_descendant(tree: &loro::LoroTree, node: TreeID, ancestor: TreeID) -> bool {
    let mut parent = tree.parent(node);
    while let Some(TreeParentId::Node(parent_id)) = parent {
        if parent_id == ancestor {
            return true;
        }
        parent = tree.parent(parent_id);
    }
    false
}

fn utf16_boundaries(value: &str) -> Vec<usize> {
    let mut output = vec![0];
    let mut offset = 0;
    for character in value.chars() {
        offset += character.len_utf16();
        output.push(offset);
    }
    output
}

fn selected_text_range(value: &str, raw_index: i64, raw_length: i64) -> (usize, usize) {
    let boundaries = utf16_boundaries(value);
    let start_index = modulo(raw_index, boundaries.len());
    let remaining = boundaries.len() - start_index - 1;
    if remaining == 0 {
        return (boundaries[start_index], boundaries[start_index]);
    }
    let scalar_length = 1 + modulo(raw_length, remaining);
    (
        boundaries[start_index],
        boundaries[start_index + scalar_length],
    )
}

fn loro_value(value: &Value) -> LoroValue {
    LoroValue::from(value.clone())
}

fn peer_index(value: i64, peer_count: usize) -> usize {
    modulo(value, peer_count)
}

fn modulo(value: i64, length: usize) -> usize {
    if length == 0 {
        return 0;
    }
    value.rem_euclid(length as i64) as usize
}
