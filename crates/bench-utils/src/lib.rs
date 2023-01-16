use arbitrary::Arbitrary;
use enum_as_inner::EnumAsInner;
use rand::{rngs::StdRng, RngCore, SeedableRng};
use std::io::Read;

use flate2::read::GzDecoder;
use serde_json::Value;

#[derive(Arbitrary)]
pub struct TextAction {
    pub pos: usize,
    pub ins: String,
    pub del: usize,
}

pub fn get_automerge_actions() -> Vec<TextAction> {
    const RAW_DATA: &[u8; 901823] =
        include_bytes!("../../loro-internal/benches/automerge-paper.json.gz");
    let mut actions = Vec::new();
    let mut d = GzDecoder::new(&RAW_DATA[..]);
    let mut s = String::new();
    d.read_to_string(&mut s).unwrap();
    let json: Value = serde_json::from_str(&s).unwrap();
    let txns = json.as_object().unwrap().get("txns");
    for txn in txns.unwrap().as_array().unwrap() {
        let patches = txn
            .as_object()
            .unwrap()
            .get("patches")
            .unwrap()
            .as_array()
            .unwrap();
        for patch in patches {
            let pos = patch[0].as_u64().unwrap() as usize;
            let del_here = patch[1].as_u64().unwrap() as usize;
            let ins_content = patch[2].as_str().unwrap();
            actions.push(TextAction {
                pos,
                ins: ins_content.to_string(),
                del: del_here,
            });
        }
    }
    actions
}

#[derive(EnumAsInner, Arbitrary)]
pub enum Action {
    Text { client: usize, action: TextAction },
    SyncAll,
}

pub fn gen_realtime_actions(action_num: usize, client_num: usize, seed: u64) -> Vec<Action> {
    let mut gen = StdRng::seed_from_u64(seed);
    let size = Action::size_hint(1);
    let size = size.1.unwrap_or(size.0);
    let mut dest = vec![0; action_num * size];
    gen.fill_bytes(&mut dest);
    let mut arb = arbitrary::Unstructured::new(&dest);
    let mut ans = Vec::new();
    let mut last_sync_all = 0;
    for i in 0..action_num {
        if ans.len() >= action_num {
            break;
        }

        let mut action = arb.arbitrary().unwrap();
        match &mut action {
            Action::Text { client, action } => {
                *client %= client_num;
                if !action.ins.is_empty() {
                    action.ins = (action.ins.as_bytes()[0] as u8).to_string();
                }
            }
            Action::SyncAll => {
                last_sync_all = i;
            }
        }

        ans.push(action);
        if i - last_sync_all > 100 {
            ans.push(Action::SyncAll);
            last_sync_all = i;
        }
    }

    ans
}
