use crate::{span::IdSpan, VersionVector};

use super::text_content::TextContent;

struct Tracker {}

impl Tracker {
    fn turn_on(&mut self, id: IdSpan) {}
    fn turn_off(&mut self, id: IdSpan) {}
    fn checkout(&mut self, vv: VersionVector) {}
    fn apply(&mut self, content: TextContent) {}
}
