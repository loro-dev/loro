use arbitrary::Arbitrary;
use loro_delta::{
    text_delta::{TextChunk, TextDelta},
    DeltaItem,
};
use tracing::{debug_span, instrument, trace};

#[derive(Debug, Arbitrary)]
pub enum Op {
    Insert { site: u8, pos: u16, text: u16 },
    Delete { site: u8, pos: u16, len: u16 },
    Sync { site: u8 },
}

pub struct Actor {
    rope: TextDelta,
    last_sync_version: usize,
    pending: TextDelta,
}

pub struct Manager {
    server: TextDelta,
    versions: Vec<TextDelta>,
    actors: Vec<Actor>,
}

#[instrument(skip(m))]
fn sync(m: &mut Manager, site: usize) {
    let actor = &mut m.actors[site];
    let mut server_ops = TextDelta::new();
    for t in &m.versions[actor.last_sync_version..] {
        server_ops.compose(t);
    }

    let client_to_apply = server_ops.transform(&actor.pending, true);

    let client_ops = std::mem::take(&mut actor.pending);

    let server_to_apply = client_ops.transform(&server_ops, false);

    actor.rope.compose(&client_to_apply);
    m.server.compose(&server_to_apply);
    m.versions.push(server_to_apply);
    actor.last_sync_version = m.versions.len();
}

pub fn run(mut ops: Vec<Op>, site_num: usize) {
    let mut m = Manager {
        server: TextDelta::new(),
        versions: vec![],
        actors: vec![],
    };
    for _ in 0..site_num {
        m.actors.push(Actor {
            rope: TextDelta::new(),
            last_sync_version: 0,
            pending: TextDelta::new(),
        })
    }

    for op in &mut ops {
        match op {
            Op::Insert { site, pos, text } => {
                *site = ((*site as usize) % site_num) as u8;
                let actor = &mut m.actors[*site as usize];
                let len = actor.rope.len();
                *pos = ((*pos as usize) % (len + 1)) as u16;
                let pos = *pos as usize;

                actor.rope.insert_str(pos, text.to_string().as_str());
                if actor.pending.len() < pos {
                    actor.pending.push_retain(pos, ());
                }
                actor.pending.insert_values(
                    pos,
                    TextChunk::from_long_str(text.to_string().as_str()).map(|chunk| {
                        DeltaItem::Replace {
                            value: chunk,
                            attr: Default::default(),
                            delete: 0,
                        }
                    }),
                );
            }
            Op::Delete {
                site,
                pos,
                len: del_len,
            } => {
                *site = ((*site as usize) % site_num) as u8;
                let actor = &mut m.actors[*site as usize];
                let len = actor.rope.len();
                if len == 0 {
                    continue;
                }
                *pos = ((*pos as usize) % len) as u16;
                let pos = *pos as usize;
                *del_len = ((*del_len as usize) % len) as u16;
                let del_len = *del_len as usize;
                let mut del = TextDelta::new();
                del.push_retain(pos, ()).push_delete(del_len);
                actor.rope.compose(&del);
                actor.pending.compose(&del);
            }
            Op::Sync { site } => {
                *site = ((*site as usize) % site_num) as u8;
                let site = *site as usize;
                sync(&mut m, site);
            }
        }
    }

    debug_span!("Round 1").in_scope(|| {
        for i in 0..site_num {
            sync(&mut m, i);
        }
    });
    debug_span!("Round 2").in_scope(|| {
        for i in 0..site_num {
            sync(&mut m, i);
        }
    });

    let server_str = m.server.try_to_string().unwrap();
    for i in 0..site_num {
        let actor = &m.actors[i];
        let rope_str = actor.rope.try_to_string().unwrap();
        assert_eq!(rope_str, server_str, "site {} ops={:#?}", i, &ops);
    }
}

#[cfg(test)]
mod tests {
    use super::Op::*;
    use super::*;

    #[ctor::ctor]
    fn init() {
        dev_utils::setup_test_log();
    }

    #[test]
    fn test_run() {
        let ops = vec![
            Insert {
                site: 1,
                pos: 0,
                text: 65535,
            },
            Sync { site: 1 },
            Insert {
                site: 1,
                pos: 5,
                text: 0,
            },
        ];
        run(ops, 2);
    }
}
