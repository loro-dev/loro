use arbitrary::Arbitrary;
use enum_as_inner::EnumAsInner;
use tabled::{TableIteratorExt, Tabled};

use crate::{
    container::{text::text_container::TextContainer, Container},
    LoroCore,
};

#[derive(Arbitrary, EnumAsInner, Clone, PartialEq, Eq, Debug)]
pub enum Action {
    Ins { content: String, pos: usize },
    Del { pos: usize, len: usize },
}

impl Tabled for Action {
    const LENGTH: usize = 4;

    fn fields(&self) -> Vec<std::borrow::Cow<'_, str>> {
        match self {
            Action::Ins { content, pos } => vec![
                "ins".into(),
                pos.to_string().into(),
                content.len().to_string().into(),
                content.into(),
            ],
            Action::Del { pos, len } => vec![
                "del".into(),
                pos.to_string().into(),
                len.to_string().into(),
                "".into(),
            ],
        }
    }

    fn headers() -> Vec<std::borrow::Cow<'static, str>> {
        vec!["type".into(), "pos".into(), "len".into(), "content".into()]
    }
}

trait Actionable {
    fn apply_action(&mut self, action: &Action);
    fn preprocess(&self, action: &mut Action);
}

impl Action {
    pub fn preprocess(&mut self, max_len: usize) {
        match self {
            Action::Ins { pos, .. } => {
                *pos %= max_len + 1;
            }
            Action::Del { pos, len, .. } => {
                if max_len == 0 {
                    *pos = 0;
                    *len = 0;
                } else {
                    *pos %= max_len;
                    *len = (*len).min(max_len - (*pos));
                }
            }
        }
    }
}

impl Actionable for String {
    fn apply_action(&mut self, action: &Action) {
        match action {
            Action::Ins { content, pos } => {
                self.insert_str(*pos, content);
            }
            &Action::Del { pos, len } => {
                if self.len() == 0 {
                    return;
                }

                self.drain(pos..pos + len);
            }
        }
    }

    fn preprocess(&self, action: &mut Action) {
        action.preprocess(self.len());
        match action {
            Action::Ins { pos, .. } => {
                while !self.is_char_boundary(*pos) {
                    *pos = (*pos + 1) % (self.len() + 1)
                }
            }
            Action::Del { pos, len } => {
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
        }
    }
}

impl Actionable for TextContainer {
    fn apply_action(&mut self, action: &Action) {
        match action {
            Action::Ins { content, pos } => {
                self.insert(*pos, content);
            }
            &Action::Del { pos, len } => {
                if self.text_len() == 0 {
                    return;
                }

                self.delete(pos, len);
            }
        }
    }

    fn preprocess(&self, _action: &mut Action) {
        unreachable!();
    }
}

pub fn test_single_client(mut actions: Vec<Action>) {
    let mut store = LoroCore::new(Default::default(), Some(1));
    let mut text_container = store.get_text_container("haha".into());
    let mut ground_truth = String::new();
    let mut applied = Vec::new();
    for action in actions.iter_mut() {
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

#[cfg(test)]
mod test {
    use ctor::ctor;

    use super::Action::*;
    use super::*;

    #[test]
    fn test() {
        test_single_client(vec! [
            Ins {
                content: "\u{16}\u{16}\u{16}\u{16}\u{16}#####BBBBSSSSSSSSS".into(),
                pos: 60797853338129363,
            },
            Ins {
                content: "\u{13}T0\u{18}5\u{13}".into(),
                pos: 1369375761697341439,
            },
            Ins {
                content: "\0\0\0SS".into(),
                pos: 280733345338323,
            },
            Ins {
                content: "**".into(),
                pos: 5444570,
            },
            Ins {
                content: "\u{13}".into(),
                pos: 5692550972993381338,
            },
            Ins {
                content: "OOOOOOOOOOOOOOBBBBBBBBBBBBBBBBB#\0\0####".into(),
                pos: 138028458976,
            },
            Ins {
                content: "".into(),
                pos: 267263250998051,
            },
            Ins {
                content: "".into(),
                pos: 4774378554966147091,
            },
            Ins {
                content: "BBBBB#\0\0######## \0\0\0######".into(),
                pos: 3038287259207217298,
            },
            Ins {
                content: "".into(),
                pos: 16645325113485036074,
            },
            Ins {
                content: "".into(),
                pos: 23362835702677503,
            },
            Ins {
                content: "S".into(),
                pos: 280733345338323,
            },
            Ins {
                content: "*UUU".into(),
                pos: 2761092332,
            },
            Ins {
                content: "\u{5ec}".into(),
                pos: 15332975680940594378,
            },
            Ins {
                content: "".into(),
                pos: 3038287259199214554,
            },
            Ins {
                content: "PPPPPPPPPPPPP\u{13}".into(),
                pos: 6004374254117322995,
            },
            Ins {
                content: "SSSSSS".into(),
                pos: 48379484722131,
            },
            Ins {
                content: ",\0\0\0UUUU".into(),
                pos: 2761092332,
            },
            Ins {
                content: "\u{5ec}".into(),
                pos: 15332975680940594378,
            },
            Ins {
                content: "".into(),
                pos: 3038287259199214554,
            },
            Ins {
                content: "".into(),
                pos: 5787213827046133840,
            },
            Ins {
                content: "PPPPPPPPPPPPPPPP*****".into(),
                pos: 2762368,
            },
            Ins {
                content: "".into(),
                pos: 0,
            },
            Ins {
                content: "".into(),
                pos: 0,
            },
            Ins {
                content: "".into(),
                pos: 3038287259199220266,
            },
            Ins {
                content: "*\0\u{13}EEEEEEEEEEEEEEEEEEEEEEEE".into(),
                pos: 4179340455027348442,
            },
            Ins {
                content: "\0UUUU".into(),
                pos: 2761092332,
            },
            Ins {
                content: "\u{5ec}".into(),
                pos: 15332976539934053578,
            },
            Ins {
                content: "ڨ\0\0\0*******************".into(),
                pos: 3038287259199220352,
            },
            Ins {
                content: "*&*****".into(),
                pos: 6004234345560396434,
            },
            Ins {
                content: "".into(),
                pos: 3038287259199220307,
            },
            Ins {
                content: "******".into(),
                pos: 3038287259889816210,
            },
            Ins {
                content: "*****".into(),
                pos: 11350616413819538,
            },
            Ins {
                content: "".into(),
                pos: 6004234345560363859,
            },
            Ins {
                content: "S".into(),
                pos: 60797853338129363,
            },
            Ins {
                content: "\u{13}T3\u{18}5\u{13}".into(),
                pos: 1369375761697341439,
            },
            Ins {
                content: "\0\0\0SS".into(),
                pos: 280733345338323,
            },
            Ins {
                content: "*UUU".into(),
                pos: 2761092332,
            },
            Ins {
                content: "\u{5ec}".into(),
                pos: 15332975680940594378,
            },
            Ins {
                content: "".into(),
                pos: 3038287259199214554,
            },
            Ins {
                content: "".into(),
                pos: 5787213827046133840,
            },
            Ins {
                content: "".into(),
                pos: 5787213827046133840,
            },
            Ins {
                content: "PPPP*****".into(),
                pos: 2762368,
            },
            Ins {
                content: "\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0******".into(),
                pos: 3038287259199220266,
            },
            Ins {
                content: "EEEEEEEEEEEEEEEEEEEEEEE".into(),
                pos: 4179340455027348442,
            },
            Ins {
                content: "\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0 ,\0\0\0UUUU".into(),
                pos: 2761092332,
            },
            Ins {
                content: "\u{5ec}".into(),
                pos: 14483766535198004426,
            },
            Ins {
                content: "".into(),
                pos: 3038240898625886739,
            },
            Ins {
                content: "*************".into(),
                pos: 3038287259199220352,
            },
            Ins {
                content: "*&*****".into(),
                pos: 6004234345560396434,
            },
            Ins {
                content: "S*********\0*******".into(),
                pos: 3038287259889816210,
            },
            Ins {
                content: "*****".into(),
                pos: 11350616413819538,
            },
            Ins {
                content: "SSSSSSSSSSSSS".into(),
                pos: 60797853338129363,
            },
            Ins {
                content: "\u{13}T4\u{18}5\u{13}".into(),
                pos: 1369375761697341439,
            },
            Ins {
                content: "\0\0\0SS".into(),
                pos: 3834029289772372947,
            },
            Ins {
                content: "55555555555555555555555555555555555555555555555555555555555555555555555555555555555".into(),
                pos: 280603991029045,
            },
            Ins {
                content: "".into(),
                pos: 356815350314,
            },
            Ins {
                content: "\u{13}\0\u{13}".into(),
                pos: 1369095330717705178,
            },
        ])
    }

    #[ctor]
    fn init_color_backtrace() {
        color_backtrace::install();
    }
}
