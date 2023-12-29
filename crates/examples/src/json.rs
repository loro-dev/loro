use bench_utils::{json::JsonAction, Action};
use loro::LoroDoc;

use crate::{run_actions_fuzz_in_async_mode, ActorTrait};

pub struct JsonActor {
    doc: LoroDoc,
    list: loro::LoroList,
    map: loro::LoroMap,
    text: loro::LoroText,
}

impl ActorTrait for JsonActor {
    type ActionKind = JsonAction;

    fn create(peer_id: u64) -> Self {
        let doc = LoroDoc::new();
        doc.set_peer_id(peer_id).unwrap();
        let list = doc.get_list("list");
        let map = doc.get_map("map");
        let text = doc.get_text("text");
        Self {
            doc,
            list,
            map,
            text,
        }
    }

    fn apply_action(&mut self, action: &mut Self::ActionKind) {
        match action {
            JsonAction::InsertMap { key, value } => {
                self.map.insert(key, value.clone()).unwrap();
            }
            JsonAction::InsertList { index, value } => {
                *index %= self.list.len() + 1;
                self.list.insert(*index, value.clone()).unwrap();
            }
            JsonAction::DeleteList { index } => {
                if self.list.is_empty() {
                    return;
                }

                *index %= self.list.len();
                self.list.delete(*index, 1).unwrap();
            }
            JsonAction::InsertText { index, s } => {
                *index %= self.text.len_unicode() + 1;
                self.text.insert(*index, s).unwrap();
            }
            JsonAction::DeleteText { index, len } => {
                if self.text.is_empty() {
                    return;
                }

                *index %= self.text.len_unicode();
                *len %= self.text.len_unicode() - *index;
                self.text.delete(*index, *len).unwrap();
            }
        }
    }

    fn doc(&self) -> &loro::LoroDoc {
        &self.doc
    }
}

pub fn fuzz(peer_num: usize, inputs: &[Action<JsonAction>]) {
    run_actions_fuzz_in_async_mode::<JsonActor>(peer_num, 20, inputs);
}

#[cfg(test)]
mod test {
    #[test]
    fn test() {
        assert_eq!(f64::NAN, f64::NAN)
    }
}
