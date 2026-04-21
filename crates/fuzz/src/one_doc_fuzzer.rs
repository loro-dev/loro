use loro::{ContainerType, Frontiers, LoroDoc, LoroError, TreeID, UndoManager};
use tabled::TableIteratorExt;
use tracing::{info, info_span};

use crate::{actions::ActionWrapper, crdt_fuzzer::FuzzValue, Action};

#[derive(Default)]
struct Branch {
    frontiers: Frontiers,
}

struct OneDocFuzzer {
    doc: LoroDoc,
    branches: Vec<Branch>,
    undo_managers: Vec<UndoManager>,
}

impl OneDocFuzzer {
    pub fn new(site_num: usize) -> Self {
        let doc = LoroDoc::new();
        doc.set_detached_editing(true);
        Self {
            doc: doc.clone(),
            branches: (0..site_num).map(|_| Branch::default()).collect(),
            undo_managers: (0..site_num).map(|_| UndoManager::new(&doc)).collect(),
        }
    }

    fn site_num(&self) -> usize {
        self.branches.len()
    }

    fn pre_process(&mut self, action: &mut Action) {
        let max_users = self.site_num() as u8;
        match action {
            Action::Sync { from, to } => {
                *from %= max_users;
                *to %= max_users;
                if to == from {
                    *to = (*to + 1) % max_users;
                }
            }
            Action::SyncAll => {}
            Action::Checkout { .. } => {}
            Action::Handle {
                site,
                target,
                action,
                ..
            } => {
                if matches!(action, ActionWrapper::Action(_)) {
                    return;
                }
                *site %= max_users;
                let branch = &mut self.branches[*site as usize];
                let valid_targets = [
                    ContainerType::Text,
                    ContainerType::List,
                    ContainerType::Map,
                    ContainerType::MovableList,
                    ContainerType::Tree,
                    ContainerType::Counter,
                ];
                *target %= valid_targets.len() as u8;
                action.convert_to_inner(&valid_targets[*target as usize]);
                self.doc.checkout(&branch.frontiers).unwrap();
                if let Some(action) = action.as_action_mut() {
                    match action {
                        crate::actions::ActionInner::Map(..) => {}
                        crate::actions::ActionInner::Counter(..) => {}
                        crate::actions::ActionInner::List(list_action) => match list_action {
                            crate::container::list::ListAction::Insert { pos, .. } => {
                                let len = self.doc.get_list("list").len();
                                *pos %= (len as u8).saturating_add(1);
                            }
                            crate::container::list::ListAction::Delete { pos, len } => {
                                let length = self.doc.get_list("list").len();
                                if length == 0 {
                                    *pos = 0;
                                    *len = 0;
                                } else {
                                    *pos %= length as u8;
                                    let mut end = pos.saturating_add(*len);
                                    end = end % (length as u8) + 1;
                                    if *pos > end {
                                        *pos = end - 1;
                                    }
                                    *len = end - *pos;
                                }
                            }
                            crate::container::list::ListAction::Push { .. } => {}
                            crate::container::list::ListAction::Pop => {
                                let len = self.doc.get_list("list").len();
                                if len == 0 {
                                    *list_action = crate::actions::ListAction::Insert {
                                        pos: 0,
                                        value: FuzzValue::I32(0),
                                    };
                                }
                            }
                            crate::container::list::ListAction::Clear => {}
                        },
                        crate::actions::ActionInner::MovableList(movable_list_action) => {
                            match movable_list_action {
                                crate::actions::MovableListAction::Insert { pos, .. } => {
                                    let len = self.doc.get_movable_list("movable_list").len();
                                    *pos %= (len as u8).saturating_add(1);
                                }
                                crate::actions::MovableListAction::Delete { pos, len } => {
                                    let length = self.doc.get_movable_list("movable_list").len();
                                    if length == 0 {
                                        *pos = 0;
                                        *len = 0;
                                    } else {
                                        *pos %= length as u8;
                                        let mut end = pos.saturating_add(*len);
                                        end = end % (length as u8) + 1;
                                        if *pos > end {
                                            *pos = end - 1;
                                        }
                                        *len = end - *pos;
                                    }
                                }
                                crate::actions::MovableListAction::Move { from, to } => {
                                    let len = self.doc.get_movable_list("movable_list").len();
                                    if len == 0 {
                                        *movable_list_action =
                                            crate::actions::MovableListAction::Insert {
                                                pos: 0,
                                                value: FuzzValue::I32(0),
                                            };
                                    } else {
                                        *from %= len as u8;
                                        *to %= len as u8;
                                    }
                                }
                                crate::actions::MovableListAction::Set { pos, value } => {
                                    let len = self.doc.get_movable_list("movable_list").len();
                                    if len == 0 {
                                        *movable_list_action =
                                            crate::actions::MovableListAction::Insert {
                                                pos: 0,
                                                value: *value,
                                            };
                                    } else {
                                        *pos %= len as u8;
                                    }
                                }
                                crate::actions::MovableListAction::Push { .. } => {}
                                crate::actions::MovableListAction::Pop => {
                                    let len = self.doc.get_movable_list("movable_list").len();
                                    if len == 0 {
                                        *movable_list_action = crate::actions::MovableListAction::Insert {
                                            pos: 0,
                                            value: FuzzValue::I32(0),
                                        };
                                    }
                                }
                                crate::actions::MovableListAction::Clear => {}
                            }
                        }
                        crate::actions::ActionInner::Text(text_action) => {
                            match text_action.action {
                                crate::container::TextActionInner::Insert => {
                                    let len = self.doc.get_text("text").len_unicode();
                                    text_action.pos %= len.saturating_add(1);
                                }
                                crate::container::TextActionInner::Delete => {
                                    let len = self.doc.get_text("text").len_unicode();
                                    if len == 0 {
                                        text_action.action =
                                            crate::container::TextActionInner::Insert;
                                    }
                                    text_action.pos %= len.saturating_add(1);
                                    let mut end = text_action.pos.wrapping_add(text_action.len);
                                    if end > len {
                                        end %= len + 1;
                                    }
                                    if end < text_action.pos {
                                        end = len;
                                    }
                                    text_action.len = end - text_action.pos;
                                }
                                crate::container::TextActionInner::Mark(_) => {}
                                crate::container::TextActionInner::Update => {}
                                crate::container::TextActionInner::InsertUtf8 => {
                                    let len = self.doc.get_text("text").len_utf8();
                                    text_action.pos %= len.saturating_add(1);
                                }
                                crate::container::TextActionInner::DeleteUtf8 => {
                                    let len = self.doc.get_text("text").len_utf8();
                                    if len == 0 {
                                        text_action.action =
                                            crate::container::TextActionInner::InsertUtf8;
                                    }
                                    text_action.pos %= len.saturating_add(1);
                                    let mut end = text_action.pos.wrapping_add(text_action.len);
                                    if end > len {
                                        end %= len + 1;
                                    }
                                    if end < text_action.pos {
                                        end = len;
                                    }
                                    text_action.len = end - text_action.pos;
                                }
                                crate::container::TextActionInner::MarkUtf8(_) => {}
                                crate::container::TextActionInner::Splice => {
                                    let len = self.doc.get_text("text").len_unicode();
                                    if len == 0 {
                                        text_action.action =
                                            crate::container::TextActionInner::Insert;
                                    }
                                    text_action.pos %= len.saturating_add(1);
                                    let mut end = text_action.pos.wrapping_add(text_action.len);
                                    if end > len {
                                        end %= len + 1;
                                    }
                                    if end < text_action.pos {
                                        end = len;
                                    }
                                    text_action.len = end - text_action.pos;
                                }
                                crate::container::TextActionInner::Unmark(_) => {}
                            }
                        }
                        crate::actions::ActionInner::Tree(tree_action) => {
                            let tree = self.doc.get_tree("tree");
                            tree.enable_fractional_index(0);
                            let nodes = tree
                                .nodes()
                                .into_iter()
                                .filter(|x| !tree.is_node_deleted(x).unwrap())
                                .collect::<Vec<_>>();
                            let node_num = nodes.len();
                            let crate::container::TreeAction { target, action } = tree_action;
                            if node_num == 0
                                || node_num < 2
                                    && (matches!(
                                        action,
                                        crate::container::TreeActionInner::Move { .. }
                                            | crate::container::TreeActionInner::MoveBefore { .. }
                                            | crate::container::TreeActionInner::MoveAfter { .. }
                                            | crate::container::TreeActionInner::Meta { .. }
                                            | crate::container::TreeActionInner::MetaDelete { .. }
                                            | crate::container::TreeActionInner::MetaClear
                                            | crate::container::TreeActionInner::Mov { .. }
                                    ))
                            {
                                *action = crate::container::TreeActionInner::Create { index: 0 };
                            }
                            match action {
                                crate::container::TreeActionInner::Create { index } => {
                                    let id = tree.__internal__next_tree_id();
                                    let len = tree.children_num(None).unwrap_or(0);
                                    *index %= len + 1;
                                    *target = (id.peer, id.counter);
                                }
                                crate::container::TreeActionInner::Delete => {
                                    let target_index = target.0 as usize % node_num;
                                    *target =
                                        (nodes[target_index].peer, nodes[target_index].counter);
                                }
                                crate::container::TreeActionInner::Move { parent, index } => {
                                    let target_index = target.0 as usize % node_num;
                                    *target =
                                        (nodes[target_index].peer, nodes[target_index].counter);
                                    let mut parent_idx = parent.0 as usize % node_num;
                                    while target_index == parent_idx {
                                        parent_idx = (parent_idx + 1) % node_num;
                                    }
                                    *parent = (nodes[parent_idx].peer, nodes[parent_idx].counter);
                                    *index %= tree
                                        .children_num(TreeID::new(parent.0, parent.1))
                                        .unwrap_or(0)
                                        + 1;
                                }
                                crate::container::TreeActionInner::MoveBefore {
                                    target,
                                    before: (p, c),
                                }
                                | crate::container::TreeActionInner::MoveAfter {
                                    target,
                                    after: (p, c),
                                } => {
                                    let target_index = target.0 as usize % node_num;
                                    *target =
                                        (nodes[target_index].peer, nodes[target_index].counter);
                                    let mut other_idx = *p as usize % node_num;
                                    while target_index == other_idx {
                                        other_idx = (other_idx + 1) % node_num;
                                    }
                                    *p = nodes[other_idx].peer;
                                    *c = nodes[other_idx].counter;
                                }
                                crate::container::TreeActionInner::Meta { meta: (_, v) } => {
                                    let target_index = target.0 as usize % node_num;
                                    *target =
                                        (nodes[target_index].peer, nodes[target_index].counter);
                                    if matches!(v, FuzzValue::Container(_)) {
                                        *v = FuzzValue::I32(0);
                                    }
                                }
                                crate::container::TreeActionInner::MetaDelete { key } => {
                                    let target_index = target.0 as usize % node_num;
                                    *target =
                                        (nodes[target_index].peer, nodes[target_index].counter);
                                    if key.is_empty() {
                                        *key = "0".to_string();
                                    }
                                }
                                crate::container::TreeActionInner::MetaClear => {
                                    let target_index = target.0 as usize % node_num;
                                    *target =
                                        (nodes[target_index].peer, nodes[target_index].counter);
                                }
                                crate::container::TreeActionInner::CreateWithoutIndex { parent } => {
                                    if node_num == 0 {
                                        *parent = (0, 0);
                                    } else {
                                        let parent_idx = parent.0 as usize % node_num;
                                        *parent = (nodes[parent_idx].peer, nodes[parent_idx].counter);
                                    }
                                    let id = tree.__internal__next_tree_id();
                                    *target = (id.peer, id.counter);
                                }
                                crate::container::TreeActionInner::Mov { parent } => {
                                    let target_index = target.0 as usize % node_num;
                                    *target =
                                        (nodes[target_index].peer, nodes[target_index].counter);
                                    if node_num < 2 {
                                        *action = crate::container::TreeActionInner::Create { index: 0 };
                                    } else {
                                        let mut parent_idx = parent.0 as usize % node_num;
                                        while target_index == parent_idx {
                                            parent_idx = (parent_idx + 1) % node_num;
                                        }
                                        *parent = (nodes[parent_idx].peer, nodes[parent_idx].counter);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Action::Undo { site, op_len: _ } => {
                *site %= max_users;
            }
            Action::SyncAllUndo { site, op_len: _ } => {
                *site %= max_users;
            }
            Action::ForkAt { site, to: _ } => {
                *site %= max_users;
            }
            Action::DiffApply { from, to } => {
                *from %= max_users;
                *to %= max_users;
                if from == to {
                    *to = (*to + 1) % max_users;
                }
            }
            Action::Query { site, target, .. } => {
                *site %= max_users;
                // target maps to container type index
                let valid_targets = [
                    ContainerType::Text,
                    ContainerType::List,
                    ContainerType::Map,
                    ContainerType::MovableList,
                    ContainerType::Tree,
                    ContainerType::Counter,
                ];
                *target %= valid_targets.len() as u8;
            }
            Action::ExportShallow { site } => {
                *site %= max_users;
            }
            Action::ImportShallow { site, from } => {
                *site %= max_users;
                *from %= max_users;
                if site == from {
                    *from = (*from + 1) % max_users;
                }
            }
            Action::StateOnlyRoundTrip { site } => {
                *site %= max_users;
            }
            Action::Commit { site } => {
                *site %= max_users;
            }
            Action::SetCommitOptions { site, .. } => {
                *site %= max_users;
            }
        }
    }

    fn apply_action(&mut self, action: &mut Action) {
        match action {
            Action::Handle { site, action, .. } => {
                let doc = &mut self.doc;
                let branch = &mut self.branches[*site as usize];
                doc.checkout(&branch.frontiers).unwrap();
                match action {
                    ActionWrapper::Action(action_inner) => match action_inner {
                        crate::actions::ActionInner::Map(map_action) => match map_action {
                            crate::actions::MapAction::Insert { key, value } => {
                                let map = doc.get_map("map");
                                map.insert(&key.to_string(), value.to_string()).unwrap();
                            }
                            crate::actions::MapAction::Delete { key } => {
                                let map = doc.get_map("map");
                                map.delete(&key.to_string()).unwrap();
                            }
                            crate::actions::MapAction::Clear => {
                                let map = doc.get_map("map");
                                map.clear().unwrap();
                            }
                        },
                        crate::actions::ActionInner::Counter(counter_action) => {
                            let counter = doc.get_counter("counter");
                            if counter_action.decrement {
                                counter.decrement(counter_action.value as f64).unwrap();
                            } else {
                                counter.increment(counter_action.value as f64).unwrap();
                            }
                        },
                        crate::actions::ActionInner::List(list_action) => match list_action {
                            crate::actions::ListAction::Insert { pos, value } => {
                                let list = doc.get_list("list");
                                list.insert(*pos as usize, value.to_string()).unwrap();
                            }
                            crate::actions::ListAction::Delete { pos, len } => {
                                let list = doc.get_list("list");
                                list.delete(*pos as usize, *len as usize).unwrap();
                            }
                            crate::actions::ListAction::Push { value } => {
                                let list = doc.get_list("list");
                                list.push(value.to_string()).unwrap();
                            }
                            crate::actions::ListAction::Pop => {
                                let list = doc.get_list("list");
                                list.pop().unwrap();
                            }
                            crate::actions::ListAction::Clear => {
                                let list = doc.get_list("list");
                                list.clear().unwrap();
                            }
                        },
                        crate::actions::ActionInner::MovableList(movable_list_action) => {
                            match movable_list_action {
                                crate::actions::MovableListAction::Insert { pos, value } => {
                                    let list = doc.get_movable_list("movable_list");
                                    list.insert(*pos as usize, value.to_string()).unwrap();
                                }
                                crate::actions::MovableListAction::Delete { pos, len } => {
                                    let list = doc.get_movable_list("movable_list");
                                    list.delete(*pos as usize, *len as usize).unwrap();
                                }
                                crate::actions::MovableListAction::Move { from, to } => {
                                    let list = doc.get_movable_list("movable_list");
                                    list.mov(*from as usize, *to as usize).unwrap();
                                }
                                crate::actions::MovableListAction::Set { pos, value } => {
                                    let list = doc.get_movable_list("movable_list");
                                    list.set(*pos as usize, value.to_string()).unwrap();
                                }
                                crate::actions::MovableListAction::Push { value } => {
                                    let list = doc.get_movable_list("movable_list");
                                    list.push(value.to_string()).unwrap();
                                }
                                crate::actions::MovableListAction::Pop => {
                                    let list = doc.get_movable_list("movable_list");
                                    list.pop().unwrap();
                                }
                                crate::actions::MovableListAction::Clear => {
                                    let list = doc.get_movable_list("movable_list");
                                    list.clear().unwrap();
                                }
                            }
                        }
                        crate::actions::ActionInner::Text(text_action) => {
                            let text = doc.get_text("text");
                            match text_action.action {
                                crate::container::TextActionInner::Insert => {
                                    text.insert(text_action.pos, &text_action.len.to_string())
                                        .unwrap();
                                }
                                crate::container::TextActionInner::Delete => {
                                    text.delete(text_action.pos, text_action.len).unwrap();
                                }
                                crate::container::TextActionInner::Mark(_) => {}
                                crate::container::TextActionInner::Update => {
                                    let s = text_action.len.to_string();
                                    text.update(&s, loro::UpdateOptions::default()).unwrap();
                                }
                                crate::container::TextActionInner::InsertUtf8 => {
                                    let s = text_action.len.to_string();
                                    text.insert_utf8(text_action.pos, &s).unwrap();
                                }
                                crate::container::TextActionInner::DeleteUtf8 => {
                                    text.delete_utf8(text_action.pos, text_action.len).unwrap();
                                }
                                crate::container::TextActionInner::MarkUtf8(_) => {}
                                crate::container::TextActionInner::Splice => {
                                    let s = text_action.len.to_string();
                                    text.splice(text_action.pos, text_action.len, &s).unwrap();
                                }
                                crate::container::TextActionInner::Unmark(_) => {}
                            }
                        }
                        crate::actions::ActionInner::Tree(tree_action) => {
                            let tree = self.doc.get_tree("tree");
                            let crate::container::TreeAction { target, action } = tree_action;
                            let target = TreeID {
                                peer: target.0,
                                counter: target.1,
                            };
                            match action {
                                crate::container::TreeActionInner::Create { index } => {
                                    tree.create_at(None, *index).unwrap();
                                }
                                crate::container::TreeActionInner::Delete => {
                                    tree.delete(target).unwrap();
                                }
                                crate::container::TreeActionInner::Move { parent, index } => {
                                    let parent = TreeID {
                                        peer: parent.0,
                                        counter: parent.1,
                                    };
                                    if let Err(LoroError::TreeError(e)) =
                                        tree.mov_to(target, Some(parent), *index)
                                    {
                                        // cycle move
                                        tracing::warn!("move error {}", e);
                                    }
                                }
                                crate::container::TreeActionInner::MoveBefore {
                                    target,
                                    before,
                                } => {
                                    let target = TreeID {
                                        peer: target.0,
                                        counter: target.1,
                                    };
                                    let before = TreeID {
                                        peer: before.0,
                                        counter: before.1,
                                    };
                                    tree.mov_before(target, before).unwrap();
                                }
                                crate::container::TreeActionInner::MoveAfter { target, after } => {
                                    let target = TreeID {
                                        peer: target.0,
                                        counter: target.1,
                                    };
                                    let after = TreeID {
                                        peer: after.0,
                                        counter: after.1,
                                    };
                                    tree.mov_after(target, after).unwrap();
                                }
                                crate::container::TreeActionInner::Meta { meta: (k, v) } => {
                                    let meta = tree.get_meta(target).unwrap();
                                    meta.insert(k, v.to_string()).unwrap();
                                }
                                crate::container::TreeActionInner::MetaDelete { key } => {
                                    let meta = tree.get_meta(target).unwrap();
                                    meta.delete(key).unwrap();
                                }
                                crate::container::TreeActionInner::MetaClear => {
                                    let meta = tree.get_meta(target).unwrap();
                                    meta.clear().unwrap();
                                }
                                crate::container::TreeActionInner::CreateWithoutIndex { parent } => {
                                    let parent = if parent.0 == 0 && parent.1 == 0 {
                                        None
                                    } else {
                                        Some(TreeID::new(parent.0, parent.1))
                                    };
                                    tree.create(parent).unwrap();
                                }
                                crate::container::TreeActionInner::Mov { parent } => {
                                    let parent = if parent.0 == 0 && parent.1 == 0 {
                                        None
                                    } else {
                                        Some(TreeID::new(parent.0, parent.1))
                                    };
                                    if let Err(LoroError::TreeError(e)) = tree.mov(target, parent) {
                                        tracing::warn!("mov error {}", e);
                                    }
                                }
                            }
                        }
                    },
                    _ => unreachable!(),
                }
            }
            Action::Checkout { .. } => {}
            Action::Sync { from, to } => {
                let mut f = self.branches[*from as usize].frontiers.clone();
                f.merge_with_greater(&self.branches[*to as usize].frontiers);
                self.branches[*to as usize].frontiers = self.doc.minimize_frontiers(&f).unwrap();
            }
            Action::SyncAll => {
                let f = self.doc.oplog_frontiers();
                for b in self.branches.iter_mut() {
                    b.frontiers = f.clone();
                }
            }
            Action::Undo { site, op_len } => {
                let undo = &mut self.undo_managers[*site as usize];
                let undo_len = *op_len % 16;
                if undo_len != 0 && undo.can_undo() {
                    self.doc.checkout(&self.branches[*site as usize].frontiers).unwrap();
                    for _ in 0..undo_len {
                        undo.undo().unwrap();
                    }
                    self.branches[*site as usize].frontiers = self.doc.oplog_frontiers();
                    undo.clear();
                }
            }
            Action::SyncAllUndo { site, op_len } => {
                let f = self.doc.oplog_frontiers();
                for b in self.branches.iter_mut() {
                    b.frontiers = f.clone();
                }
                let undo = &mut self.undo_managers[*site as usize];
                let undo_len = *op_len % 8;
                if undo_len != 0 && undo.can_undo() {
                    self.doc.checkout(&self.branches[*site as usize].frontiers).unwrap();
                    for _ in 0..undo_len {
                        undo.undo().unwrap();
                    }
                    self.branches[*site as usize].frontiers = self.doc.oplog_frontiers();
                    undo.clear();
                }
            }
            Action::ForkAt { site, to } => {
                let frontiers = self.branches[*site as usize].frontiers.clone();
                let _forked = self.doc.fork_at(&frontiers);
            }
            Action::DiffApply { from, to } => {
                let from_frontiers = self.branches[*from as usize].frontiers.clone();
                let to_frontiers = self.branches[*to as usize].frontiers.clone();
                if let Ok(diff) = self.doc.diff(&from_frontiers, &to_frontiers) {
                    let _ = self.doc.apply_diff(diff);
                }
            }
            Action::Query { site, target, query_type } => {
                let branch = &self.branches[*site as usize];
                self.doc.checkout(&branch.frontiers).unwrap();
                let valid_targets = [
                    ContainerType::Text,
                    ContainerType::List,
                    ContainerType::Map,
                    ContainerType::MovableList,
                    ContainerType::Tree,
                    ContainerType::Counter,
                ];
                let ty = valid_targets[*target as usize % valid_targets.len()];
                match ty {
                    ContainerType::Text => {
                        let text = self.doc.get_text("text");
                        match *query_type % 8 {
                            0 => { let _ = text.to_delta(); }
                            1 => { let _ = text.len_unicode(); }
                            2 => { let _ = text.len_utf8(); }
                            3 => {
                                let len = text.len_unicode();
                                if len > 0 {
                                    let _ = text.get_cursor(len / 2, loro::cursor::Side::Left);
                                }
                            }
                            4 => {
                                let len = text.len_unicode();
                                if len > 0 {
                                    let _ = text.slice(0, len / 2);
                                }
                            }
                            5 => {
                                let len = text.len_utf8();
                                if len > 0 {
                                    let _ = text.slice(0, len / 2);
                                }
                            }
                            _ => { let _ = text.to_string(); }
                        }
                    }
                    ContainerType::List => {
                        let list = self.doc.get_list("list");
                        match *query_type % 4 {
                            0 => { let _ = list.len(); }
                            1 => { let _ = list.to_vec(); }
                            2 => {
                                let len = list.len();
                                if len > 0 {
                                    let _ = list.get(len / 2);
                                }
                            }
                            _ => { let _ = list.is_empty(); }
                        }
                    }
                    ContainerType::Map => {
                        let map = self.doc.get_map("map");
                        match *query_type % 4 {
                            0 => { let _ = map.keys(); }
                            1 => { let _ = map.values(); }
                            2 => { let _ = map.len(); }
                            _ => { let _ = map.is_empty(); }
                        }
                    }
                    ContainerType::Tree => {
                        let tree = self.doc.get_tree("tree");
                        match *query_type % 8 {
                            0 => { let _ = tree.nodes(); }
                            1 => { let _ = tree.children(None); }
                            2 => { let _ = tree.children_num(None); }
                            3 => { let _ = tree.contains(TreeID::new(0, 0)); }
                            4 => { let _ = tree.is_node_deleted(&TreeID::new(0, 0)); }
                            5 => { let _ = tree.parent(TreeID::new(0, 0)); }
                            _ => { let _ = tree.is_empty(); }
                        }
                    }
                    ContainerType::MovableList => {
                        let list = self.doc.get_movable_list("movable_list");
                        match *query_type % 4 {
                            0 => { let _ = list.len(); }
                            1 => { let _ = list.to_vec(); }
                            2 => {
                                let len = list.len();
                                if len > 0 {
                                    let _ = list.get(len / 2);
                                }
                            }
                            _ => { let _ = list.is_empty(); }
                        }
                    }
                    ContainerType::Counter => {
                        let counter = self.doc.get_counter("counter");
                        let _ = counter.get();
                    }
                    ContainerType::Unknown(_) => {}
                }
            }
            Action::ExportShallow { site } => {
                let branch = &self.branches[*site as usize];
                self.doc.checkout(&branch.frontiers).unwrap();
                let f = self.doc.oplog_frontiers();
                if !f.is_empty() {
                    let _ = self.doc.export(loro::ExportMode::shallow_snapshot(&f));
                }
            }
            Action::ImportShallow { site, from } => {
                let from_frontiers = self.branches[*from as usize].frontiers.clone();
                self.doc.checkout(&from_frontiers).unwrap();
                let f = self.doc.oplog_frontiers();
                if !f.is_empty() {
                    if let Ok(bytes) = self.doc.export(loro::ExportMode::shallow_snapshot(&f)) {
                        let site_frontiers = self.branches[*site as usize].frontiers.clone();
                        self.doc.checkout(&site_frontiers).unwrap();
                        let _ = self.doc.import(&bytes);
                    }
                }
            }
            Action::StateOnlyRoundTrip { site } => {
                let branch = &self.branches[*site as usize];
                self.doc.checkout(&branch.frontiers).unwrap();
                let f = self.doc.state_frontiers();
                if !f.is_empty() {
                    if let Ok(bytes) = self.doc.export(loro::ExportMode::state_only(Some(&f))) {
                        let new_doc = LoroDoc::new();
                        if new_doc.import(&bytes).is_ok() {
                            assert_eq!(new_doc.get_deep_value(), self.doc.get_deep_value());
                        }
                    }
                }
            }
            Action::Commit { site: _ } => {
                self.doc.commit();
            }
            Action::SetCommitOptions { site, origin, msg } => {
                let branch = &self.branches[*site as usize];
                self.doc.checkout(&branch.frontiers).unwrap();
                let origins = ["fuzz", "test", "a", "b", "c", "d", "e", "f"];
                self.doc
                    .set_next_commit_origin(origins[*origin as usize % origins.len()]);
                let msgs = ["msg1", "msg2", "hello", "world"];
                self.doc
                    .set_next_commit_message(msgs[*msg as usize % msgs.len()]);
            }
        }
    }

    fn check_sync(&self) {
        self.doc.checkout_to_latest();
        self.doc.check_state_correctness_slow();
        for b in self.branches.iter() {
            self.doc.checkout(&b.frontiers).unwrap();
            self.doc.check_state_correctness_slow();
        }
    }
}

pub fn test_multi_sites_on_one_doc(site_num: u8, actions: &mut [Action]) {
    let mut fuzzer = OneDocFuzzer::new(site_num as usize);
    let mut applied = Vec::new();
    for action in actions.iter_mut() {
        fuzzer.pre_process(action);
        info_span!("ApplyAction", ?action).in_scope(|| {
            applied.push(action.clone());
            info!("OptionsTable \n{}", (&applied).table());
            // info!("Apply Action {:?}", applied);
            fuzzer.apply_action(action);
        });
    }

    // println!("OpTable \n{}", (&applied).table());
    info_span!("check synced").in_scope(|| {
        fuzzer.check_sync();
    });
}
