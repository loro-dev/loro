use fractional_index::FractionalIndex;
use loro_delta::{array_vec::ArrayVec, delta_trait::DeltaAttr};
use loro_internal::{
    delta::{
        DeltaType, Meta, ResolvedMapDelta, ResolvedMapValue, TreeDiff, TreeDiffItem,
        TreeExternalDiff,
    },
    event::{
        path_to_str, str_to_path, Diff, Index, ListDeltaMeta, ListDiff, ListDiffInsertItem,
        TextDiff, TextMeta,
    },
    handler::ValueOrHandler,
    FxHashMap, IdLp, LoroValue, StringSlice, TreeID, TreeParentId,
};
use pretty_assertions::assert_eq;

fn list_values(values: impl IntoIterator<Item = LoroValue>) -> ListDiffInsertItem {
    let mut array = ArrayVec::new();
    for value in values {
        array.push(ValueOrHandler::Value(value)).unwrap();
    }
    array
}

fn list_insert(values: impl IntoIterator<Item = LoroValue>, from_move: bool) -> ListDiff {
    let mut diff = ListDiff::new();
    diff.push_insert(list_values(values), ListDeltaMeta { from_move });
    diff
}

fn text_insert(value: &str) -> TextDiff {
    let mut diff = TextDiff::new();
    diff.push_insert(StringSlice::from(value), TextMeta::default());
    diff
}

fn map_diff(key: &str, value: Option<LoroValue>, lamport: u32) -> ResolvedMapDelta {
    ResolvedMapDelta::new().with_entry(
        key.into(),
        ResolvedMapValue {
            value: value.map(ValueOrHandler::Value),
            idlp: IdLp::new(1, lamport),
        },
    )
}

fn tree_create(target: TreeID, index: usize) -> TreeDiff {
    TreeDiff {
        diff: vec![TreeDiffItem {
            target,
            action: TreeExternalDiff::Create {
                parent: TreeParentId::Root,
                index,
                position: FractionalIndex::default(),
            },
        }],
    }
}

#[test]
fn event_diff_helpers_compose_transform_and_report_empty_by_kind() {
    assert_eq!(format!("{:?}", Index::Seq(7)), "Index::Seq(7)");
    assert_eq!(
        format!("{:?}", Index::Node(TreeID::new(3, 9))),
        "Index::Node(9@3)"
    );
    assert_eq!(Index::from(4).to_string(), "4");
    assert_eq!(
        path_to_str(&[Index::Key("root".into()), Index::Seq(2)]),
        "root/2"
    );
    assert_eq!(
        str_to_path("root/2/9@3"),
        Some(vec![
            Index::Key("root".into()),
            Index::Seq(2),
            Index::Node(TreeID::new(3, 9)),
        ])
    );

    let mut attrs = FxHashMap::default();
    attrs.insert("bold".to_string(), LoroValue::Bool(true));
    let text_meta = TextMeta(attrs.clone());
    let roundtrip_attrs: FxHashMap<String, LoroValue> = text_meta.clone().into();
    assert_eq!(roundtrip_attrs, attrs);
    assert_eq!(TextMeta::from(attrs), text_meta);

    let mut list_meta = ListDeltaMeta::default();
    assert!(<ListDeltaMeta as Meta>::is_empty(&list_meta));
    <ListDeltaMeta as Meta>::compose(
        &mut list_meta,
        &ListDeltaMeta { from_move: true },
        (DeltaType::Retain, DeltaType::Retain),
    );
    assert!(list_meta.from_move);
    assert!(<ListDeltaMeta as Meta>::is_mergeable(
        &list_meta,
        &ListDeltaMeta { from_move: true }
    ));
    <ListDeltaMeta as Meta>::merge(&mut list_meta, &ListDeltaMeta { from_move: true });

    let mut delta_attr = ListDeltaMeta::default();
    assert!(DeltaAttr::attr_is_empty(&delta_attr));
    DeltaAttr::compose(&mut delta_attr, &ListDeltaMeta { from_move: true });
    assert!(delta_attr.from_move);
    assert!(!DeltaAttr::attr_is_empty(&delta_attr));

    let mut list = Diff::List(list_insert([LoroValue::I64(1)], false));
    list.compose_ref(&Diff::List(list_insert([LoroValue::I64(2)], true)));
    assert!(!list.is_empty());
    let list_composed = Diff::List(list_insert([LoroValue::I64(3)], false))
        .compose(Diff::List(list_insert([LoroValue::I64(4)], false)))
        .expect("same diff kinds should compose");
    assert!(!list_composed.is_empty());

    let mut text = Diff::Text(text_insert("a"));
    text.compose_ref(&Diff::Text(text_insert("b")));
    assert!(!text.is_empty());
    let text_composed = Diff::Text(text_insert("c"))
        .compose(Diff::Text(text_insert("d")))
        .expect("text diffs should compose");
    assert!(!text_composed.is_empty());

    let map_a = Diff::Map(map_diff("title", Some("old".into()), 1));
    let map_b = Diff::Map(map_diff("title", Some("new".into()), 2));
    let mut map_composed = map_a
        .clone()
        .compose(map_b.clone())
        .expect("map diffs should compose");
    assert!(!map_composed.is_empty());
    map_composed.transform(&map_b, false);
    assert!(map_composed.is_empty());

    let target = TreeID::new(1, 2);
    let tree_a = Diff::Tree(tree_create(target, 0));
    let tree_b = Diff::Tree(tree_create(target, 1));
    let mut tree_composed = tree_a
        .clone()
        .compose(tree_b.clone())
        .expect("tree diffs should compose");
    assert!(!tree_composed.is_empty());
    tree_composed.transform(&tree_b, false);
    assert!(tree_composed.is_empty());

    assert!(Diff::Unknown.is_empty());
    let wrong_kind = Diff::Text(text_insert("x")).compose(Diff::List(list_insert([], false)));
    assert!(matches!(wrong_kind, Err(Diff::Text(_))));

    #[cfg(feature = "counter")]
    {
        let counter = Diff::Counter(1.0)
            .compose(Diff::Counter(2.0))
            .expect("counter diffs should compose");
        assert!(matches!(counter, Diff::Counter(3.0)));
        let mut transformed = Diff::Counter(1.0);
        transformed.transform(&Diff::Counter(2.0), false);
        assert!(matches!(transformed, Diff::Counter(1.0)));
        assert!(Diff::Counter(0.0).is_empty());
    }
}
