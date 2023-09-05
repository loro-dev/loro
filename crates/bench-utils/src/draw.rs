use arbitrary::{Arbitrary, Unstructured};

#[derive(Arbitrary)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Arbitrary)]
pub enum DrawAction {
    DrawPath {
        points: Vec<Point>,
        color: i32,
    },
    Text {
        id: i32,
        text: String,
        pos: Point,
        width: i32,
        height: i32,
    },
}

pub fn gen_draw_actions(seed: u64, num: usize) -> Vec<DrawAction> {
    let be_bytes = seed.to_be_bytes();
    let mut gen = Unstructured::new(&be_bytes);
    let mut ans = vec![];
    for _ in 0..num {
        ans.push(gen.arbitrary().unwrap());
    }

    ans
}
