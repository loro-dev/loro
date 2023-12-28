use std::{collections::HashMap, time::Instant};

use bench_utils::{
    create_seed, draw::DrawAction, gen_async_actions, gen_realtime_actions, make_actions_async,
    Action,
};
use loro::{ContainerID, ContainerType};

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

    pub fn apply_action(&mut self, action: &mut DrawAction) {
        match action {
            DrawAction::CreatePath { points } => {
                let path = self.paths.insert_container(0, ContainerType::Map).unwrap();
                let path_map = path.into_map().unwrap();
                let pos_map = path_map
                    .insert_container("pos", ContainerType::Map)
                    .unwrap()
                    .into_map()
                    .unwrap();
                pos_map.insert("x", 0).unwrap();
                pos_map.insert("y", 0).unwrap();
                let path = path_map
                    .insert_container("path", ContainerType::List)
                    .unwrap()
                    .into_list()
                    .unwrap();
                for p in points {
                    let map = path
                        .push_container(ContainerType::Map)
                        .unwrap()
                        .into_map()
                        .unwrap();
                    map.insert("x", p.x).unwrap();
                    map.insert("y", p.y).unwrap();
                }
                let len = self.id_to_obj.len();
                self.id_to_obj.insert(len, path_map.id());
            }
            DrawAction::Text { text, pos, size } => {
                let text_container = self
                    .texts
                    .insert_container(0, ContainerType::Map)
                    .unwrap()
                    .into_map()
                    .unwrap();
                let text_inner = text_container
                    .insert_container("text", ContainerType::Text)
                    .unwrap()
                    .into_text()
                    .unwrap();
                text_inner.insert(0, text).unwrap();
                let map = text_container
                    .insert_container("pos", ContainerType::Map)
                    .unwrap()
                    .into_map()
                    .unwrap();
                map.insert("x", pos.x).unwrap();
                map.insert("y", pos.y).unwrap();
                let map = text_container
                    .insert_container("size", ContainerType::Map)
                    .unwrap()
                    .into_map()
                    .unwrap();
                map.insert("x", size.x).unwrap();
                map.insert("y", size.y).unwrap();

                let len = self.id_to_obj.len();
                self.id_to_obj.insert(len, text_container.id());
            }
            DrawAction::CreateRect { pos, .. } => {
                let rect = self.rects.insert_container(0, ContainerType::Map).unwrap();
                let rect_map = rect.into_map().unwrap();
                let pos_map = rect_map
                    .insert_container("pos", ContainerType::Map)
                    .unwrap()
                    .into_map()
                    .unwrap();
                pos_map.insert("x", pos.x).unwrap();
                pos_map.insert("y", pos.y).unwrap();

                let size_map = rect_map
                    .insert_container("size", ContainerType::Map)
                    .unwrap()
                    .into_map()
                    .unwrap();
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
                let pos_map = map.get("pos").unwrap().unwrap_right().into_map().unwrap();
                let x = pos_map.get("x").unwrap().unwrap_left().into_i32().unwrap();
                let y = pos_map.get("y").unwrap().unwrap_left().into_i32().unwrap();
                pos_map
                    .insert("x", x.overflowing_add(relative_to.x).0)
                    .unwrap();
                pos_map
                    .insert("y", y.overflowing_add(relative_to.y).0)
                    .unwrap();
            }
        }
    }
}

pub struct DrawActors {
    pub docs: Vec<DrawActor>,
}

impl DrawActors {
    pub fn new(size: usize) -> Self {
        let docs = (0..size).map(|i| DrawActor::new(i as u64)).collect();
        Self { docs }
    }

    pub fn apply_action(&mut self, action: &mut Action<DrawAction>) {
        match action {
            Action::Action { peer, action } => {
                self.docs[*peer].apply_action(action);
            }
            Action::Sync { from, to } => {
                let vv = self.docs[*from].doc.oplog_vv();
                let data = self.docs[*from].doc.export_from(&vv);
                self.docs[*to].doc.import(&data).unwrap();
            }
            Action::SyncAll => self.sync_all(),
        }
    }

    pub fn sync_all(&mut self) {
        let (first, rest) = self.docs.split_at_mut(1);
        for doc in rest.iter_mut() {
            let vv = first[0].doc.oplog_vv();
            first[0].doc.import(&doc.doc.export_from(&vv)).unwrap();
        }
        for doc in rest.iter_mut() {
            let vv = doc.doc.oplog_vv();
            doc.doc.import(&first[0].doc.export_from(&vv)).unwrap();
        }
    }

    pub fn check_sync(&self) {
        let first = &self.docs[0];
        let content = first.doc.get_deep_value();
        for doc in self.docs.iter().skip(1) {
            assert_eq!(content, doc.doc.get_deep_value());
        }
    }
}

pub fn run_async_draw_workflow(
    peer_num: usize,
    action_num: usize,
    actions_before_sync: usize,
    seed: u64,
) -> (DrawActors, Instant) {
    let seed = create_seed(seed, action_num * 32);
    let mut actions =
        gen_async_actions::<DrawAction>(action_num, peer_num, &seed, actions_before_sync, |_| {})
            .unwrap();
    let mut actors = DrawActors::new(peer_num);
    let start = Instant::now();
    for action in actions.iter_mut() {
        actors.apply_action(action);
    }

    (actors, start)
}

pub fn run_realtime_collab_draw_workflow(
    peer_num: usize,
    action_num: usize,
    seed: u64,
) -> (DrawActors, Instant) {
    let seed = create_seed(seed, action_num * 32);
    let mut actions =
        gen_realtime_actions::<DrawAction>(action_num, peer_num, &seed, |_| {}).unwrap();
    let mut actors = DrawActors::new(peer_num);
    let start = Instant::now();
    for action in actions.iter_mut() {
        actors.apply_action(action);
    }

    (actors, start)
}

pub fn run_actions_fuzz_in_async_mode(
    peer_num: usize,
    sync_all_interval: usize,
    actions: &[Action<DrawAction>],
) {
    let mut actions = make_actions_async(peer_num, actions, sync_all_interval);
    let mut actors = DrawActors::new(peer_num);
    for action in actions.iter_mut() {
        actors.apply_action(action);
    }
    actors.sync_all();
    actors.check_sync();
}
