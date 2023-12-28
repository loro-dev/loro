use arbitrary::Arbitrary;

#[derive(Debug, Arbitrary, PartialEq, Eq, Clone)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Arbitrary, PartialEq, Eq, Clone)]
pub enum DrawAction {
    CreatePath {
        points: Vec<Point>,
    },
    Text {
        text: String,
        pos: Point,
        size: Point,
    },
    CreateRect {
        pos: Point,
        size: Point,
    },
    Move {
        id: u32,
        relative_to: Point,
    },
}
