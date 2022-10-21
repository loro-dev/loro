use arbitrary::Arbitrary;
use enum_as_inner::EnumAsInner;
use tabled::{TableIteratorExt, Tabled};

use crate::{
    array_mut_ref,
    container::{text::text_container::TextContainer, Container},
    debug_log, LoroCore,
};

#[derive(Arbitrary, EnumAsInner, Clone, PartialEq, Eq, Debug)]
pub enum Action {
    Ins {
        content: String,
        pos: usize,
        site: u8,
    },
    Del {
        pos: usize,
        len: usize,
        site: u8,
    },
    Sync {
        from: u8,
        to: u8,
    },
    SyncAll,
}

impl Tabled for Action {
    const LENGTH: usize = 5;

    fn fields(&self) -> Vec<std::borrow::Cow<'_, str>> {
        match self {
            Action::Ins { content, pos, site } => vec![
                "ins".into(),
                pos.to_string().into(),
                content.len().to_string().into(),
                content.into(),
                site.to_string().into(),
            ],
            Action::Del { pos, len, site } => vec![
                "del".into(),
                pos.to_string().into(),
                len.to_string().into(),
                "".into(),
                site.to_string().into(),
            ],
            Action::Sync { from, to } => vec![
                "sync".into(),
                "".into(),
                "".into(),
                "".into(),
                format!("{} to {}", from, to).into(),
            ],
            Action::SyncAll => vec![
                "sync all".into(),
                "".into(),
                "".into(),
                "".into(),
                "".into(),
            ],
        }
    }

    fn headers() -> Vec<std::borrow::Cow<'static, str>> {
        vec![
            "type".into(),
            "pos".into(),
            "len".into(),
            "content".into(),
            "site".into(),
        ]
    }
}

trait Actionable {
    fn apply_action(&mut self, action: &Action);
    fn preprocess(&mut self, action: &mut Action);
}

impl Action {
    pub fn preprocess(&mut self, max_len: usize, max_users: u8) {
        match self {
            Action::Ins { pos, site, .. } => {
                *pos %= max_len + 1;
                *site %= max_users;
            }
            Action::Del { pos, len, site } => {
                if max_len == 0 {
                    *pos = 0;
                    *len = 0;
                } else {
                    *pos %= max_len;
                    *len = (*len).min(max_len - (*pos));
                }
                *site %= max_users;
            }
            Action::Sync { from, to } => {
                *from %= max_users;
                *to %= max_users;
            }
            Action::SyncAll => {}
        }
    }
}

impl Actionable for String {
    fn apply_action(&mut self, action: &Action) {
        match action {
            Action::Ins { content, pos, .. } => {
                self.insert_str(*pos, content);
            }
            &Action::Del { pos, len, .. } => {
                if self.is_empty() {
                    return;
                }

                self.drain(pos..pos + len);
            }
            _ => {}
        }
    }

    fn preprocess(&mut self, action: &mut Action) {
        action.preprocess(self.len(), 1);
        match action {
            Action::Ins { pos, .. } => {
                while !self.is_char_boundary(*pos) {
                    *pos = (*pos + 1) % (self.len() + 1)
                }
            }
            Action::Del { pos, len, .. } => {
                if self.is_empty() {
                    *len = 0;
                    *pos = 0;
                    return;
                }

                while !self.is_char_boundary(*pos) {
                    *pos = (*pos + 1) % self.len();
                }

                *len = (*len).min(self.len() - (*pos));
                while !self.is_char_boundary(*pos + *len) {
                    *len += 1;
                }
            }
            _ => {}
        }
    }
}

impl Actionable for TextContainer {
    fn apply_action(&mut self, action: &Action) {
        match action {
            Action::Ins { content, pos, .. } => {
                self.insert(*pos, content);
            }
            &Action::Del { pos, len, .. } => {
                if self.text_len() == 0 {
                    return;
                }

                self.delete(pos, len);
            }
            _ => {}
        }
    }

    fn preprocess(&mut self, _action: &mut Action) {
        unreachable!();
    }
}

impl Actionable for Vec<LoroCore> {
    fn apply_action(&mut self, action: &Action) {
        match action {
            Action::Ins { content, pos, site } => {
                self[*site as usize]
                    .get_or_create_text_container_mut("text".into())
                    .insert(*pos, content);
            }
            Action::Del { pos, len, site } => {
                self[*site as usize]
                    .get_or_create_text_container_mut("text".into())
                    .delete(*pos, *len);
            }
            Action::Sync { from, to } => {
                let to_vv = self[*to as usize].vv();
                let from_exported = self[*from as usize].export(to_vv);
                self[*to as usize].import(from_exported);
            }
            Action::SyncAll => {}
        }
    }

    fn preprocess(&mut self, action: &mut Action) {
        match action {
            Action::Ins { content, pos, site } => {
                *site %= self.len() as u8;
                let mut text = self[*site as usize].get_or_create_text_container_mut("text".into());
                let value = text.get_value().as_string().unwrap();
                *pos %= value.len() + 1;
                while !value.is_char_boundary(*pos) {
                    *pos = (*pos + 1) % (value.len() + 1)
                }
            }
            Action::Del { pos, len, site } => {
                *site %= self.len() as u8;
                let mut text = self[*site as usize].get_or_create_text_container_mut("text".into());
                if text.text_len() == 0 {
                    *len = 0;
                    *pos = 0;
                    return;
                }

                let str = text.get_value().as_string().unwrap();
                *pos %= str.len() + 1;
                while !str.is_char_boundary(*pos) {
                    *pos = (*pos + 1) % str.len();
                }

                *len = (*len).min(str.len() - (*pos));
                while !str.is_char_boundary(*pos + *len) {
                    *len += 1;
                }
            }
            Action::Sync { from, to } => {
                *from %= self.len() as u8;
                *to %= self.len() as u8;
            }
            Action::SyncAll => {}
        }
    }
}

fn check_eq(site_a: &mut LoroCore, site_b: &mut LoroCore) {
    let mut a = site_a.get_or_create_text_container_mut("text".into());
    let mut b = site_b.get_or_create_text_container_mut("text".into());
    let value_a = a.get_value();
    let value_b = b.get_value();
    assert_eq!(value_a.as_string().unwrap(), value_b.as_string().unwrap());
}

fn check_synced(sites: &mut [LoroCore]) {
    for i in 0..sites.len() - 1 {
        for j in i + 1..sites.len() {
            debug_log!("-------------------------------");
            debug_log!("checking {} with {}", i, j);
            debug_log!("-------------------------------");

            let (a, b) = array_mut_ref!(sites, [i, j]);
            a.import(b.export(a.vv()));
            b.import(a.export(b.vv()));
            check_eq(a, b)
        }
    }
}

pub fn test_single_client(mut actions: Vec<Action>) {
    let mut store = LoroCore::new(Default::default(), Some(1));
    let mut text_container = store.get_or_create_text_container_mut("haha".into());
    let mut ground_truth = String::new();
    let mut applied = Vec::new();
    for action in actions
        .iter_mut()
        .filter(|x| x.as_del().is_some() || x.as_ins().is_some())
    {
        ground_truth.preprocess(action);
        applied.push(action.clone());
        // println!("{}", (&applied).table());
        ground_truth.apply_action(action);
        text_container.apply_action(action);
        assert_eq!(
            ground_truth.as_str(),
            text_container.get_value().as_string().unwrap().as_str(),
            "{}",
            applied.table()
        );
    }
}

pub fn test_multi_sites(site_num: u8, mut actions: Vec<Action>) {
    let mut sites = Vec::new();
    for i in 0..site_num {
        sites.push(LoroCore::new(Default::default(), Some(i as u64)));
    }

    let mut applied = Vec::new();
    for action in actions.iter_mut() {
        sites.preprocess(action);
        applied.push(action.clone());
        debug_log!("\n{}", (&applied).table());
        sites.apply_action(action);
    }

    debug_log!("=================================");
    // println!("{}", actions.table());
    check_synced(&mut sites);
}

#[cfg(test)]
mod test {
    use ctor::ctor;

    use super::Action::*;
    use super::*;
    #[test]
    fn test_two_unknown() {
        test_multi_sites(
            2,
            vec![
                Ins {
                    content: "xy".into(),
                    pos: 16212948762929070335,
                    site: 224,
                },
                Ins {
                    content: "ab".into(),
                    pos: 18444492273993252863,
                    site: 5,
                },
                Sync { from: 254, to: 255 },
                Ins {
                    content: "1234".into(),
                    pos: 128512,
                    site: 0,
                },
            ],
        )
    }

    #[test]
    fn test_two_change_deps_issue() {
        test_multi_sites(
            2,
            vec![
                Ins {
                    content: "12345".into(),
                    pos: 281479272970938,
                    site: 21,
                },
                Ins {
                    content: "67890".into(),
                    pos: 17870294359908942010,
                    site: 248,
                },
                Sync { from: 1, to: 0 },
                Ins {
                    content: "abc".into(),
                    pos: 186,
                    site: 0,
                },
            ],
        )
    }

    #[test]
    fn test_two() {
        test_multi_sites(
            2,
            vec![
                Ins {
                    content: "12345".into(),
                    pos: 6447834,
                    site: 0,
                },
                Ins {
                    content: "x".into(),
                    pos: 17753860855744831232,
                    site: 115,
                },
                Del {
                    pos: 18335269204214833762,
                    len: 52354349510255359,
                    site: 0,
                },
            ],
        )
    }

    #[ctor]
    fn init_color_backtrace() {
        color_backtrace::install();
    }
}
