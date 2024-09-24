use loro::{ContainerType, Frontiers, LoroDoc};
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
}

impl OneDocFuzzer {
    pub fn new(site_num: usize) -> Self {
        let doc = LoroDoc::new();
        doc.set_detached_editing(true);
        Self {
            doc,
            branches: (0..site_num).map(|_| Branch::default()).collect(),
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
            Action::Checkout { site, to } => {}
            Action::Handle {
                site,
                target,
                container,
                action,
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
                ];
                *target %= valid_targets.len() as u8;
                action.convert_to_inner(&valid_targets[*target as usize]);
                self.doc.checkout(&branch.frontiers).unwrap();
                if let Some(action) = action.as_action_mut() {
                    match action {
                        crate::actions::ActionInner::Map(map_action) => {}
                        crate::actions::ActionInner::List(list_action) => match list_action {
                            crate::container::list::ListAction::Insert { pos, value } => {
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
                        },
                        crate::actions::ActionInner::MovableList(movable_list_action) => {
                            match movable_list_action {
                                crate::actions::MovableListAction::Insert { pos, value } => {
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
                            }
                        }
                        _ => {}
                    }
                }
            }
            Action::Undo { site, op_len } => {}
            Action::SyncAllUndo { site, op_len } => {}
        }
    }

    fn apply_action(&mut self, action: &mut Action) {
        match action {
            Action::Handle {
                site,
                target,
                container,
                action,
            } => {
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
                                    text.delete(text_action.pos as usize, text_action.len)
                                        .unwrap();
                                }
                                crate::container::TextActionInner::Mark(_) => {}
                            }
                        }
                        _ => unimplemented!(),
                    },
                    _ => unreachable!(),
                }
            }
            Action::Sync { from, to } => {
                let a = self.branches[*from as usize].frontiers.clone();
                self.branches[*to as usize].frontiers.extend_from_slice(&a);
            }
            Action::SyncAll => {
                let f = self.doc.oplog_frontiers();
                for b in self.branches.iter_mut() {
                    b.frontiers = f.clone();
                }
            }
            _ => {}
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
