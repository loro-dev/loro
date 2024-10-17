use std::collections::HashMap;

use bench_utils::{draw::DrawAction, Action};
use loro::{ContainerID, LoroList, LoroMap, LoroText};

use crate::{run_actions_fuzz_in_async_mode, ActorTrait};

pub struct DrawActor {
    pub doc: loro::LoroDoc,
    paths: loro::LoroList,
    texts: loro::LoroList,
    rects: loro::LoroList,
    id_to_obj: HashMap<usize, ContainerID>,
}

impl DrawActor {
    pub fn new(id: u64) -> Self {
        let doc = loro::LoroDoc::new();
        doc.set_peer_id(id).unwrap();
        let paths = doc.get_list("all_paths");
        let texts = doc.get_list("all_texts");
        let rects = doc.get_list("all_rects");
        let id_to_obj = HashMap::new();
        Self {
            doc,
            paths,
            texts,
            rects,
            id_to_obj,
        }
    }
}

impl ActorTrait for DrawActor {
    type ActionKind = DrawAction;

    fn create(peer_id: u64) -> Self {
        Self::new(peer_id)
    }

    fn apply_action(&mut self, action: &mut Self::ActionKind) {
        match action {
            DrawAction::CreatePath { points } => {
                let path_map = self.paths.insert_container(0, LoroMap::new()).unwrap();
                let pos_map = path_map.insert_container("pos", LoroMap::new()).unwrap();
                pos_map.insert("x", 0).unwrap();
                pos_map.insert("y", 0).unwrap();
                let path = path_map.insert_container("path", LoroList::new()).unwrap();
                for p in points {
                    let map = path.push_container(LoroMap::new()).unwrap();
                    map.insert("x", p.x).unwrap();
                    map.insert("y", p.y).unwrap();
                }
                let len = self.id_to_obj.len();
                self.id_to_obj.insert(len, path_map.id());
            }
            DrawAction::Text { text, pos, size } => {
                let text_container = self.texts.insert_container(0, LoroMap::new()).unwrap();
                let text_inner = text_container
                    .insert_container("text", LoroText::new())
                    .unwrap();
                text_inner.insert(0, text).unwrap();
                let map = text_container
                    .insert_container("pos", LoroMap::new())
                    .unwrap();
                map.insert("x", pos.x).unwrap();
                map.insert("y", pos.y).unwrap();
                let map = text_container
                    .insert_container("size", LoroMap::new())
                    .unwrap();
                map.insert("x", size.x).unwrap();
                map.insert("y", size.y).unwrap();

                let len = self.id_to_obj.len();
                self.id_to_obj.insert(len, text_container.id());
            }
            DrawAction::CreateRect { pos, .. } => {
                let rect_map = self.rects.insert_container(0, LoroMap::new()).unwrap();
                let pos_map = rect_map.insert_container("pos", LoroMap::new()).unwrap();
                pos_map.insert("x", pos.x).unwrap();
                pos_map.insert("y", pos.y).unwrap();

                let size_map = rect_map.insert_container("size", LoroMap::new()).unwrap();
                size_map.insert("width", pos.x).unwrap();
                size_map.insert("height", pos.y).unwrap();

                let len = self.id_to_obj.len();
                self.id_to_obj.insert(len, rect_map.id());
            }
            DrawAction::Move { id, relative_to } => {
                let Some(id) = self.id_to_obj.get(&(*id as usize)) else {
                    return;
                };

                let map = self.doc.get_map(id);
                let pos_map = map
                    .get("pos")
                    .unwrap()
                    .into_container()
                    .unwrap()
                    .into_map()
                    .unwrap();
                let x = pos_map
                    .get("x")
                    .unwrap()
                    .into_value()
                    .unwrap()
                    .into_i64()
                    .unwrap();
                let y = pos_map
                    .get("y")
                    .unwrap()
                    .into_value()
                    .unwrap()
                    .into_i64()
                    .unwrap();
                pos_map
                    .insert("x", x.overflowing_add(relative_to.x as i64).0)
                    .unwrap();
                pos_map
                    .insert("y", y.overflowing_add(relative_to.y as i64).0)
                    .unwrap();
            }
        }
    }

    fn doc(&self) -> &loro::LoroDoc {
        &self.doc
    }
}

pub fn fuzz(peer_num: usize, sync_all_interval: usize, actions: &[Action<DrawAction>]) {
    run_actions_fuzz_in_async_mode::<DrawActor>(peer_num, sync_all_interval, actions);
}
