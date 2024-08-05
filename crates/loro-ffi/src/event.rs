use loro::{
    event::{ListDiffItem, MapDelta},
    TextDelta, TreeDiff,
};

pub enum Diff<'a> {
    /// A list diff.
    List(Vec<ListDiffItem>),
    /// A text diff.
    Text(Vec<TextDelta>),
    /// A map diff.
    Map(MapDelta<'a>),
    /// A tree diff.
    Tree(&'a TreeDiff),
    #[cfg(feature = "counter")]
    /// A counter diff.
    Counter(f64),
    /// An unknown diff.
    Unknown,
}
