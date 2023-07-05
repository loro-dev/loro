use fxhash::FxHashMap;

use crate::container::ContainerID;

/// Calculate the diff between two versions. given [OpLog][super::oplog::OpLog]
/// and [AppState][super::state::AppState].
#[derive(Default)]
pub struct DiffCalculator {
    calc: FxHashMap<ContainerID, ContainerDiffCalculator>,
}
impl DiffCalculator {
    pub(crate) fn calc(
        &self,
        oplog: &super::oplog::OpLog,
        before: &crate::VersionVector,
        after: &crate::VersionVector,
    ) -> Vec<crate::event::Diff> {
        todo!()
    }
}

enum ContainerDiffCalculator {
    Text(TextDiffCalculator),
    Map(MapDiffCalculator),
    List(MapDiffCalculator),
}

struct TextDiffCalculator {}
struct MapDiffCalculator {}
struct ListDiffCalculator {}
