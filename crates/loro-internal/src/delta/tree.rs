use std::{ops::Deref, sync::Arc};

use fxhash::{FxHashMap, FxHashSet};
use loro_common::{ContainerType, LoroValue, TreeID};
use serde::Serialize;

/// Representation of differences in movable tree. It's an ordered list of [`TreeDiff`].
#[derive(Debug, Clone, Default, Serialize)]
pub struct TreeDelta {
    pub(crate) diff: Vec<TreeDiff>,
}

/// The semantic action in movable tree.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct TreeDiff {
    pub target: TreeID,
    pub action: TreeDiffItem,
}

/// The action of [`TreeDiff`]. It's the same as  [`crate::container::tree::tree_op::TreeOp`], but semantic.
#[derive(Debug, Clone, Copy, Serialize)]
pub enum TreeDiffItem {
    Create,
    Move(TreeID),
    CreateMove(TreeID),
    CreateOrAsRoot,
    Delete,
    UnCreate,
}

impl TreeDiff {
    pub(crate) fn new(target: TreeID, parent: Option<TreeID>, old_parent: Option<TreeID>) -> Self {
        let action = match (parent, old_parent) {
            (Some(p), _) => {
                if TreeID::is_deleted_root(parent) {
                    TreeDiffItem::Delete
                } else if TreeID::is_unexist_root(parent) {
                    TreeDiffItem::UnCreate
                } else if TreeID::is_deleted_root(old_parent) || TreeID::is_unexist_root(old_parent)
                {
                    TreeDiffItem::CreateMove(p)
                } else {
                    TreeDiffItem::Move(p)
                }
            }
            (None, Some(_)) => {
                if TreeID::is_deleted_root(old_parent) || TreeID::is_unexist_root(old_parent) {
                    TreeDiffItem::Create
                } else {
                    TreeDiffItem::CreateOrAsRoot
                }
            }
            (None, None) => {
                unreachable!()
            }
        };
        TreeDiff { target, action }
    }
}

impl Deref for TreeDelta {
    type Target = Vec<TreeDiff>;
    fn deref(&self) -> &Self::Target {
        &self.diff
    }
}

impl TreeDelta {
    // TODO: cannot handle this for now
    pub(crate) fn compose(&self, _x: TreeDelta) -> TreeDelta {
        unimplemented!("tree compose")
    }

    pub(crate) fn push(mut self, diff: TreeDiff) -> Self {
        self.diff.push(diff);
        self
    }
}

pub(crate) struct TreeValue<'a>(pub(crate) &'a mut Vec<LoroValue>);

impl<'a> TreeValue<'a> {
    pub(crate) fn apply_diff(&mut self, diff: &TreeDelta) {
        for d in diff.iter() {
            let target = d.target;
            debug_log::debug_log!("before {:?}", self.0);
            match d.action {
                TreeDiffItem::Create => {
                    debug_log::debug_log!("create {:?}", target);
                    let mut t = FxHashMap::default();
                    t.insert("id".to_string(), target.id().to_string().into());
                    t.insert("parent".to_string(), LoroValue::Null);
                    t.insert("meta".to_string(), ContainerType::Map.default_value());
                    self.0.push(t.into());
                }
                TreeDiffItem::CreateMove(p) => {
                    debug_log::debug_log!("create {:?} move {:?}", target, p);
                    let mut t = FxHashMap::default();
                    t.insert("id".to_string(), target.id().to_string().into());
                    t.insert("parent".to_string(), p.to_string().into());
                    t.insert("meta".to_string(), ContainerType::Map.default_value());
                    self.0.push(t.into());
                }
                TreeDiffItem::CreateOrAsRoot => {
                    debug_log::debug_log!("move {:?} None", target);
                    if let Some(map) = self.0.iter_mut().find(|x| {
                        let id = x.as_map().unwrap().get("id").unwrap().as_string().unwrap();
                        id.as_ref() == &target.to_string()
                    }) {
                        let map = map.as_map_mut().unwrap();
                        let map_mut = Arc::make_mut(map);
                        map_mut.insert("parent".to_string(), LoroValue::Null);
                    } else {
                        let mut t = FxHashMap::default();
                        t.insert("id".to_string(), target.id().to_string().into());
                        t.insert("parent".to_string(), LoroValue::Null);
                        t.insert("meta".to_string(), ContainerType::Map.default_value());
                        self.0.push(t.into());
                    }
                }
                TreeDiffItem::Move(p) => {
                    debug_log::debug_log!("{:?} move {:?}", target, p);
                    let map = self
                        .0
                        .iter_mut()
                        .find(|x| {
                            let id = x.as_map().unwrap().get("id").unwrap().as_string().unwrap();
                            id.as_ref() == &target.to_string()
                        })
                        .unwrap()
                        .as_map_mut()
                        .unwrap();
                    let map_mut = Arc::make_mut(map);
                    map_mut.insert("parent".to_string(), p.to_string().into());
                }
                TreeDiffItem::Delete | TreeDiffItem::UnCreate => {
                    debug_log::debug_log!("delete {:?} ", target);
                    let mut deleted = FxHashSet::default();
                    let mut s = vec![target.to_string()];
                    while let Some(delete) = s.pop() {
                        deleted.insert(delete.clone());
                        self.0.retain_mut(|x| {
                            let id = x.as_map().unwrap().get("id").unwrap().as_string().unwrap();
                            !deleted.contains(id.as_ref())
                        });
                        for node in self.0.iter() {
                            let node = node.as_map().unwrap().as_ref();
                            if let Some(LoroValue::String(parent)) = node.get("parent") {
                                if parent.as_ref() == &delete {
                                    s.push(
                                        (*node.get("id").unwrap().as_string().unwrap().clone())
                                            .clone(),
                                    );
                                }
                            }
                        }
                    }
                }
            }
            debug_log::debug_log!("after {:?}\n", self.0);
        }
    }
}
