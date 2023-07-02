use fxhash::FxHashMap;

use crate::container::ContainerIdx;

/// Calculate the diff between two versions. given [OpLog][super::oplog::OpLog]
/// and [AppState][super::state::AppState].
pub(super) struct DiffCalculator {
    calc: FxHashMap<ContainerIdx, ContainerDiffCalculator>,
}

enum ContainerDiffCalculator {
    Text(TextDiffCalculator),
    Map(MapDiffCalculator),
    List(MapDiffCalculator),
}

struct TextDiffCalculator {}
struct MapDiffCalculator {}
struct ListDiffCalculator {}
