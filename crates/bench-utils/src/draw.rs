use arbitrary::Arbitrary;

use crate::ActionTrait;

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

impl DrawAction {
    pub const MAX_X: i32 = 1_000_000;
    pub const MAX_Y: i32 = 1_000_000;
    pub const MAX_MOVE: i32 = 200;
}

impl ActionTrait for DrawAction {
    fn normalize(&mut self) {
        match self {
            DrawAction::CreatePath { points } => {
                for point in points {
                    point.x %= Self::MAX_X;
                    point.y %= Self::MAX_Y;
                }
            }
            DrawAction::Text { pos, size, .. } => {
                pos.x %= Self::MAX_X;
                pos.y %= Self::MAX_Y;
                size.x %= Self::MAX_X;
                size.y %= Self::MAX_Y;
            }
            DrawAction::CreateRect { pos, size } => {
                pos.x %= Self::MAX_X;
                pos.y %= Self::MAX_Y;
                size.x %= Self::MAX_X;
                size.y %= Self::MAX_Y;
            }
            DrawAction::Move { relative_to, .. } => {
                relative_to.x %= Self::MAX_MOVE;
                relative_to.y %= Self::MAX_MOVE;
            }
        }
    }
}
