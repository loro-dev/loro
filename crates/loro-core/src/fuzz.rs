use enum_as_inner::EnumAsInner;
use tabled::{TableIteratorExt, Tabled};

use crate::{
    array_mut_ref,
    container::{text::text_container::TextContainer, Container},
    debug_log,
    id::ClientID,
    LoroCore,
};

#[derive(arbitrary::Arbitrary, EnumAsInner, Clone, PartialEq, Eq, Debug)]
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
                site.to_string().into(),
                pos.to_string().into(),
                content.len().to_string().into(),
                content.into(),
            ],
            Action::Del { pos, len, site } => vec![
                "del".into(),
                site.to_string().into(),
                pos.to_string().into(),
                len.to_string().into(),
                "".into(),
            ],
            Action::Sync { from, to } => vec![
                "sync".into(),
                format!("{} to {}", from, to).into(),
                "".into(),
                "".into(),
                "".into(),
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
            "site".into(),
            "pos".into(),
            "len".into(),
            "content".into(),
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
                    .get_or_create_root_text("text")
                    .lock()
                    .unwrap()
                    .as_text_mut()
                    .unwrap()
                    .insert(*pos, content);
            }
            Action::Del { pos, len, site } => {
                self[*site as usize]
                    .get_or_create_root_text("text")
                    .lock()
                    .unwrap()
                    .as_text_mut()
                    .unwrap()
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
            Action::Ins { pos, site, .. } => {
                *site %= self.len() as u8;
                let get_or_create_root_text = self[*site as usize].get_or_create_root_text("text");
                let mut container_instance = get_or_create_root_text.lock().unwrap();
                let text = container_instance.as_text_mut().unwrap();
                let value = text.get_value();
                let value = value.as_string().unwrap();
                *pos %= value.len() + 1;
                while !value.is_char_boundary(*pos) {
                    *pos = (*pos + 1) % (value.len() + 1)
                }
            }
            Action::Del { pos, len, site } => {
                *site %= self.len() as u8;
                let get_or_create_root_text = self[*site as usize].get_or_create_root_text("text");
                let mut container_instance = get_or_create_root_text.lock().unwrap();
                let text = container_instance.as_text_mut().unwrap();
                if text.text_len() == 0 {
                    *len = 0;
                    *pos = 0;
                    return;
                }

                let text = text.get_value();
                let str = text.as_string().unwrap();
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
    let get_or_create_root_text = site_a.get_or_create_root_text("text");
    let a = get_or_create_root_text.lock().unwrap();
    let get_or_create_root_text = site_b.get_or_create_root_text("text");
    let b = get_or_create_root_text.lock().unwrap();
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
    let get_or_create_root_text = store.get_or_create_root_text("haha");
    let mut container_instance = get_or_create_root_text.lock().unwrap();
    let text_container = container_instance.as_text_mut().unwrap();
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
            &**text_container.get_value().as_string().unwrap(),
            "{}",
            applied.table()
        );
    }
}

pub fn test_multi_sites(site_num: u8, mut actions: Vec<Action>) {
    let mut sites = Vec::new();
    for i in 0..site_num {
        sites.push(LoroCore::new(Default::default(), Some(i as ClientID)));
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
    fn test_16() {
        // retreat failed
        test_multi_sites(
            8,
            vec![
                Ins {
                    content: "\0\0\0\0".into(),
                    pos: 46183951198975,
                    site: 0,
                },
                Ins {
                    content: "hhhh\0\u{15}".into(),
                    pos: 18446463180827232135,
                    site: 255,
                },
                Ins {
                    content: "\u{1}\0\0\0".into(),
                    pos: 17872543955649300641,
                    site: 247,
                },
                SyncAll,
                Ins {
                    content: "I".into(),
                    pos: 17144359356472099242,
                    site: 237,
                },
                Ins {
                    content: "\u{1}\0\0\u{14}\0".into(),
                    pos: 17149707381026848749,
                    site: 64,
                },
                Ins {
                    content: "\0\0".into(),
                    pos: 576460752320727201,
                    site: 8,
                },
                Ins {
                    content: "\u{8}\u{8}\u{8}".into(),
                    pos: 74451787075125633,
                    site: 7,
                },
                Ins {
                    content: "\0DD*\u{1}\0\n\0".into(),
                    pos: 1100718403053,
                    site: 161,
                },
                SyncAll,
                SyncAll,
                Ins {
                    content: "".into(),
                    pos: 18444210798919163626,
                    site: 255,
                },
                Del {
                    pos: 7523377975159973992,
                    len: 7523377975159973992,
                    site: 104,
                },
                Del {
                    pos: 90728552,
                    len: 7523377975153131520,
                    site: 104,
                },
                Del {
                    pos: 6842472,
                    len: 8259072,
                    site: 48,
                },
                Ins {
                    content: "0002".into(),
                    pos: 60868003017,
                    site: 0,
                },
                Ins {
                    content: "\0\0\0\0hhhh\0\u{15}".into(),
                    pos: 201792722274183,
                    site: 0,
                },
                Ins {
                    content: "\t\0,`".into(),
                    pos: 2882585236494125450,
                    site: 136,
                },
                Ins {
                    content: "\u{4}\u{4}\u{4}".into(),
                    pos: 6929914515027398276,
                    site: 138,
                },
                Sync { from: 137, to: 4 },
                Ins {
                    content: "\u{4}".into(),
                    pos: 2879109919149700,
                    site: 4,
                },
                Del {
                    pos: 8680820740569200760,
                    len: 18446744071435745400,
                    site: 255,
                },
                SyncAll,
                SyncAll,
                Del {
                    pos: 144116287587483658,
                    len: 8646911284728217599,
                    site: 120,
                },
                Ins {
                    content: "\0\0SSSSSS\0".into(),
                    pos: 4755847789890108906,
                    site: 38,
                },
                Ins {
                    content: "\0\u{10}I\u{4}\u{4}\u{4}\u{4}\u{4}\0\0\u{1}\0".into(),
                    pos: 10768900350821001,
                    site: 0,
                },
                Ins {
                    content: "I\u{4}\u{4}".into(),
                    pos: 73387494324306057,
                    site: 0,
                },
                Ins {
                    content: "\u{4}\u{4}\u{4}\u{4}\u{4}".into(),
                    pos: 47890328526783742,
                    site: 0,
                },
                Ins {
                    content: "\0\0\0\0\0\0\0\0\0\0\0\0\0\0".into(),
                    pos: 792926255917040777,
                    site: 0,
                },
                Ins {
                    content: "\u{4}\0\0\0\0\u{4}".into(),
                    pos: 3497652043353424901,
                    site: 48,
                },
            ],
        )
    }

    #[test]
    fn test_15() {
        // retreat failed
        test_multi_sites(
            2,
            vec![
                Ins {
                    content: "012345".into(),
                    pos: 16145685675428772607,
                    site: 50,
                },
                Ins {
                    content: "k".into(),
                    pos: 12514849900981321857,
                    site: 173,
                },
                Ins {
                    content: "jjj".into(),
                    pos: 827253908580597753,
                    site: 235,
                },
                Ins {
                    content: "hh".into(),
                    pos: 10619084171383039,
                    site: 186,
                },
                Sync { from: 186, to: 187 },
                Ins {
                    content: "bb".into(),
                    pos: 9154669102093696963,
                    site: 1,
                },
                Ins {
                    content: "ccc".into(),
                    pos: 10619084154605823,
                    site: 186,
                },
                Sync { from: 186, to: 187 },
                Ins {
                    content: "mm".into(),
                    pos: 10619084154662902,
                    site: 186,
                },
                Sync { from: 103, to: 186 },
                Ins {
                    content: "abcdef".into(),
                    pos: 2718485543582577120,
                    site: 0,
                },
                Del {
                    pos: 77426591251806906,
                    len: 15800371181095160576,
                    site: 1,
                },
                Del {
                    pos: 16348878094905390375,
                    len: 16348879061405328098,
                    site: 226,
                },
                Del {
                    pos: 196469752251612538,
                    len: 13455202075478711110,
                    site: 186,
                },
                Del {
                    pos: 13527329853585490429,
                    len: 8863007108824969696,
                    site: 186,
                },
                Sync { from: 186, to: 65 },
                Ins {
                    content: "kk".into(),
                    pos: 18230571291461418738,
                    site: 255,
                },
                Sync { from: 0, to: 43 },
                Del {
                    pos: 9007624456401910,
                    len: 17728429828748607488,
                    site: 0,
                },
                Ins {
                    content: "666666".into(),
                    pos: 5058070217968582656,
                    site: 219,
                },
                Del {
                    pos: 1437795153694407354,
                    len: 10619084140647187,
                    site: 0,
                },
            ],
        )
    }
    #[test]
    fn test_14() {
        // retreat failed
        test_multi_sites(
            8,
            vec![
                Ins {
                    content: "(`".into(),
                    pos: 289360693583710602,
                    site: 4,
                },
                Sync { from: 4, to: 213 },
                Ins {
                    content: "333FFFF\u{3}\0(".into(),
                    pos: 289360693583710602,
                    site: 4,
                },
                Sync { from: 4, to: 137 },
                Ins {
                    content: "3\u{3}\0@".into(),
                    pos: 289360693583710602,
                    site: 4,
                },
                Sync { from: 4, to: 137 },
                Ins {
                    content: "\u{4}(".into(),
                    pos: 9877801357708624266,
                    site: 5,
                },
                Del {
                    pos: 14806280867407362682,
                    len: 289360691352074874,
                    site: 4,
                },
                Sync { from: 137, to: 0 },
                Sync { from: 129, to: 10 },
                Ins {
                    content: "".into(),
                    pos: 15408455913343287690,
                    site: 14,
                },
                Sync { from: 204, to: 52 },
                Ins {
                    content: "".into(),
                    pos: 1513209475975492403,
                    site: 137,
                },
                Ins {
                    content: "\u{4}\0\u{4}(".into(),
                    pos: 106532207656077961,
                    site: 28,
                },
                Del {
                    pos: 7089336938131513954,
                    len: 7089336938131513954,
                    site: 98,
                },
                Del {
                    pos: 7089336938131513954,
                    len: 7089336938131513899,
                    site: 98,
                },
                Del {
                    pos: 7089336938131513954,
                    len: 7089336938131513954,
                    site: 98,
                },
                Del {
                    pos: 7089336938131513954,
                    len: 7089336938131513954,
                    site: 98,
                },
                Del {
                    pos: 7089336938131513954,
                    len: 7089336938131513954,
                    site: 98,
                },
                Del {
                    pos: 7089336938131513954,
                    len: 9971017955808381026,
                    site: 137,
                },
                Sync { from: 98, to: 98 },
            ],
        )
    }
    #[test]
    fn test_13() {
        // retreat failed
        test_multi_sites(
            8,
            vec![
                Ins {
                    content: "ab".into(),
                    pos: 289360693583710602,
                    site: 4,
                },
                Sync { from: 4, to: 137 },
                Del {
                    pos: 9873061956456284998,
                    len: 9955211391596233748,
                    site: 137,
                },
                Ins {
                    content: "xxx".into(),
                    pos: 289365091630221706,
                    site: 4,
                },
                Sync { from: 137, to: 0 },
                Sync { from: 129, to: 10 },
                Del {
                    pos: 1513209475199240744,
                    len: 1130315200595337,
                    site: 44,
                },
                Del {
                    pos: 0,
                    len: 1,
                    site: 1,
                },
                Sync { from: 4, to: 0 },
                Ins {
                    content: "m".into(),
                    pos: 8825501086237362561,
                    site: 122,
                },
            ],
        )
    }

    #[test]
    fn test_12() {
        // retreat failed
        test_multi_sites(
            3,
            vec![
                Ins {
                    content: "x".into(),
                    pos: 0,
                    site: 0,
                },
                Sync { from: 0, to: 1 },
                Ins {
                    content: "y".into(),
                    pos: 1,
                    site: 1,
                },
                Del {
                    pos: 0,
                    len: 1,
                    site: 0,
                },
            ],
        )
    }

    #[test]
    fn test_11() {
        test_multi_sites(
            2,
            vec![
                Ins {
                    content: "\u{89249}".into(),
                    pos: 13441414791239010697,
                    site: 251,
                },
                Sync { from: 123, to: 118 },
                Ins {
                    content: "3".into(),
                    pos: 1427325526201334008,
                    site: 19,
                },
                Ins {
                    content: "4".into(),
                    pos: 206,
                    site: 0,
                },
            ],
        )
    }

    #[test]
    fn test_10() {
        test_multi_sites(
            10,
            vec![Ins {
                content: "\0\0".into(),
                pos: 0,
                site: 0,
            }],
        )
    }

    #[test]
    fn test_9() {
        test_multi_sites(
            2,
            vec![
                Ins {
                    content: "012".into(),
                    pos: 72066424675961795,
                    site: 195,
                },
                Ins {
                    content: "333".into(),
                    pos: 827253904285695742,
                    site: 11,
                },
                Ins {
                    content: "444".into(),
                    pos: 1941308511220,
                    site: 6,
                },
                Del {
                    pos: 14052919687256211456,
                    len: 8863007108820470271,
                    site: 186,
                },
                Ins {
                    content: "555555555555555555555".into(),
                    pos: 16176931510800348179,
                    site: 49,
                },
                Ins {
                    content: "aaa".into(),
                    pos: 1108097569780,
                    site: 6,
                },
                Sync { from: 255, to: 16 },
                Del {
                    pos: 19,
                    len: 4,
                    site: 31,
                },
                Sync { from: 255, to: 16 },
                Del {
                    pos: 19,
                    len: 4,
                    site: 31,
                },
                Ins {
                    content: "x".into(),
                    pos: 320012288,
                    site: 0,
                },
            ],
        )
    }
    #[test]
    fn test_8() {
        test_multi_sites(
            2,
            vec![
                Ins {
                    content: "abc".into(),
                    pos: 72066424675961795,
                    site: 195,
                },
                Ins {
                    content: "01234".into(),
                    pos: 14320675280616191,
                    site: 70,
                },
                Sync { from: 186, to: 37 },
                Del {
                    pos: 9293188942025195638,
                    len: 1,
                    site: 1,
                },
                Del {
                    pos: 6148914691236517205,
                    len: 17587421942457259349,
                    site: 19,
                },
            ],
        )
    }

    #[test]
    fn test_7() {
        test_multi_sites(
            2,
            vec![
                Ins {
                    content: "0".into(),
                    pos: 14175642987019698115,
                    site: 0,
                },
                Del {
                    pos: 18429584077670300346,
                    len: 125042496512,
                    site: 0,
                },
                Ins {
                    content: "2".into(),
                    pos: 2097865012304223517,
                    site: 29,
                },
                Sync { from: 37, to: 0 },
                Del {
                    pos: 3521846919483432378,
                    len: 4617062741958834688,
                    site: 224,
                },
            ],
        );
    }
    #[test]
    fn test_6() {
        test_multi_sites(
            2,
            vec![
                Ins {
                    content: "a".into(),
                    pos: 2718485539284123587,
                    site: 0,
                },
                Sync { from: 186, to: 187 },
                Ins {
                    content: "b".into(),
                    pos: 2148733715,
                    site: 0,
                },
            ],
        );
    }

    #[test]
    fn test_5() {
        test_multi_sites(
            2,
            vec![
                Ins {
                    content: "1".into(),
                    pos: 72066424675961795,
                    site: 195,
                },
                Ins {
                    content: "0".into(),
                    pos: 2718485543579090699,
                    site: 0,
                },
                Sync { from: 255, to: 122 },
                Ins {
                    content: "abcd".into(),
                    pos: 14051512346867337995,
                    site: 16,
                },
                Ins {
                    content: "xy".into(),
                    pos: 13402753207529835459,
                    site: 255,
                },
            ],
        );
    }

    #[test]
    fn test_k() {
        test_multi_sites(
            2,
            vec![
                Ins {
                    content: "123".into(),
                    pos: 9621242987464197630,
                    site: 133,
                },
                Sync { from: 255, to: 18 },
                Ins {
                    content: "ab".into(),
                    pos: 33,
                    site: 0,
                },
            ],
        );
    }
    fn test_two_unknown() {
        test_multi_sites(
            2,
            vec![
                Ins {
                    content: "0".into(),
                    pos: 3665948784267561747,
                    site: 0,
                },
                Ins {
                    content: "1".into(),
                    pos: 847522254572686275,
                    site: 1,
                },
                Sync { from: 255, to: 122 },
                Ins {
                    content: "2345".into(),
                    pos: 13402768428993809163,
                    site: 37,
                },
                Del {
                    pos: 1374463206306314938,
                    len: 799603422227,
                    site: 0,
                },
            ],
        )
    }

    #[test]
    fn test_two_common_ancestors() {
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
