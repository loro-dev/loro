pub mod draw;
use arbitrary::{Arbitrary, Unstructured};
use enum_as_inner::EnumAsInner;
use rand::{RngCore, SeedableRng};
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

#[derive(Debug, EnumAsInner, Arbitrary, PartialEq, Eq)]
pub enum Action<T> {
    Action { peer: usize, action: T },
    Sync { from: usize, to: usize },
    SyncAll,
}

pub fn gen_realtime_actions<'a, T: Arbitrary<'a>>(
    action_num: usize,
    peer_num: usize,
    seed: &'a [u8],
    mut preprocess: impl FnMut(&mut Action<T>),
) -> Result<Vec<Action<T>>, Box<str>> {
    let mut arb = Unstructured::new(seed);
    let mut ans = Vec::new();
    let mut last_sync_all = 0;
    for i in 0..action_num {
        if ans.len() >= action_num {
            break;
        }

        let mut action: Action<T> = arb
            .arbitrary()
            .map_err(|e| e.to_string().into_boxed_str())?;
        match &mut action {
            Action::Action { peer, .. } => {
                *peer %= peer_num;
            }
            Action::SyncAll => {
                last_sync_all = i;
            }
            Action::Sync { from, to } => {
                *from %= peer_num;
                *to %= peer_num;
            }
        }

        preprocess(&mut action);
        ans.push(action);
        if i - last_sync_all > 10 {
            ans.push(Action::SyncAll);
            last_sync_all = i;
        }
    }

    Ok(ans)
}

pub fn gen_async_actions<'a, T: Arbitrary<'a>>(
    action_num: usize,
    peer_num: usize,
    seed: &'a [u8],
    actions_before_sync: usize,
    mut preprocess: impl FnMut(&mut Action<T>),
) -> Result<Vec<Action<T>>, Box<str>> {
    let mut arb = Unstructured::new(seed);
    let mut ans = Vec::new();
    let mut last_sync_all = 0;
    while ans.len() < action_num {
        if ans.len() >= action_num {
            break;
        }

        if arb.is_empty() {
            return Err("not enough actions".into());
        }

        let mut action: Action<T> = arb
            .arbitrary()
            .map_err(|e| e.to_string().into_boxed_str())?;
        match &mut action {
            Action::Action { peer, .. } => {
                *peer %= peer_num;
            }
            Action::SyncAll => {
                if ans.len() - last_sync_all < actions_before_sync {
                    continue;
                }

                last_sync_all = ans.len();
            }
            Action::Sync { from, to } => {
                *from %= peer_num;
                *to %= peer_num;
            }
        }

        preprocess(&mut action);
        ans.push(action);
    }

    Ok(ans)
}

pub fn create_seed(seed: u64, size: usize) -> Vec<u8> {
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let mut ans = vec![0; size];
    rng.fill_bytes(&mut ans);
    ans
}
