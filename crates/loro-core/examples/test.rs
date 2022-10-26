use std::{cell::RefCell, rc::Rc};

#[cfg(not(feature = "fuzzing"))]
fn main() {}

struct Ref {
    out: Option<Rc<RefCell<Ref>>>,
}

#[cfg(feature = "fuzzing")]
fn main() {
    use std::cell::RefCell;
    use std::rc::Rc;

    use loro_core::fuzz::test_multi_sites;
    use loro_core::fuzz::Action::*;

    // let mut a = Rc::new(RefCell::new(Ref { out: None }));
    // let mut b = Rc::new(RefCell::new(Ref {
    //     out: Some(a.clone()),
    // }));
    // a.borrow_mut().out = Some(b.clone());
    for _ in 0..1 {
        test_multi_sites(
            10,
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
            ],
        )
    }

    println!("HAHA");
}
