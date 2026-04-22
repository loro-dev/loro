use std::cmp::Ordering;

use loro::{
    CounterSpan, ExportMode, Frontiers, ImVersionVector, LoroDoc, VersionRange, VersionVector,
    VersionVectorDiff, ID,
};
use loro_common::{FxHashMap, IdSpanVector};

fn vv(ids: &[ID]) -> VersionVector {
    ids.iter().copied().collect()
}

fn vv_pairs(pairs: &[(u64, i32)]) -> VersionVector {
    pairs.iter().copied().collect()
}

fn span(peer: u64, start: i32, end: i32) -> loro::IdSpan {
    loro::IdSpan::new(peer, start, end)
}

fn span_map(pairs: &[(u64, (i32, i32))]) -> IdSpanVector {
    let mut map = FxHashMap::default();
    for (peer, (start, end)) in pairs {
        map.insert(*peer, CounterSpan::new(*start, *end));
    }
    map
}

fn sorted_ids(frontiers: &Frontiers) -> Vec<(u64, i32)> {
    let mut ids: Vec<_> = frontiers.iter().map(|id| (id.peer, id.counter)).collect();
    ids.sort();
    ids
}

fn sorted_spans<I>(spans: I) -> Vec<(u64, i32, i32)>
where
    I: IntoIterator<Item = loro::IdSpan>,
{
    let mut spans: Vec<_> = spans
        .into_iter()
        .map(|span| (span.peer, span.counter.start, span.counter.end))
        .collect();
    spans.sort();
    spans
}

fn range_map(pairs: &[(u64, (i32, i32))]) -> FxHashMap<u64, (i32, i32)> {
    let mut map = FxHashMap::default();
    for (peer, (start, end)) in pairs {
        map.insert(*peer, (*start, *end));
    }
    map
}

#[test]
fn version_vector_contracts_follow_semantics() -> anyhow::Result<()> {
    let equal = vv_pairs(&[(1, 2), (2, 2)]);
    let greater = vv_pairs(&[(1, 3), (2, 2)]);
    let less = vv_pairs(&[(1, 1), (2, 2)]);
    let incomparable = vv_pairs(&[(1, 4), (2, 1)]);
    let zero_entry = vv_pairs(&[(9, 0)]);

    assert_eq!(equal.partial_cmp(&equal), Some(Ordering::Equal));
    assert_eq!(greater.partial_cmp(&equal), Some(Ordering::Greater));
    assert_eq!(less.partial_cmp(&equal), Some(Ordering::Less));
    assert_eq!(incomparable.partial_cmp(&equal), None);
    assert_eq!(
        zero_entry.partial_cmp(&VersionVector::new()),
        Some(Ordering::Equal)
    );

    let diff_left = vv_pairs(&[(1, 3), (2, 1)]);
    let diff_right = vv_pairs(&[(1, 1), (2, 2)]);
    let diff = diff_left.diff(&diff_right);
    assert_eq!(diff.retreat, span_map(&[(1, (1, 3))]));
    assert_eq!(diff.forward, span_map(&[(2, (1, 2))]));
    assert_eq!(sorted_spans(diff.get_id_spans_left()), vec![(1, 1, 3)]);
    assert_eq!(sorted_spans(diff.get_id_spans_right()), vec![(2, 1, 2)]);

    let mut manual = VersionVectorDiff::default();
    manual.merge_left(span(1, 4, 1));
    manual.merge_left(span(1, 0, 2));
    manual.merge_left(span(3, 2, 4));
    manual.merge_right(span(2, 1, 3));
    manual.merge_right(span(2, 5, 4));
    manual.subtract_start_left(span(1, 0, 3));
    manual.subtract_start_left(span(9, 0, 2));
    manual.subtract_start_right(span(2, 0, 2));
    assert_eq!(manual.retreat, span_map(&[(1, (3, 5)), (3, (2, 4))]));
    assert_eq!(manual.forward, span_map(&[(2, (2, 6))]));

    assert_eq!(
        diff_left.sub_iter(&diff_right).collect::<Vec<_>>(),
        vec![span(1, 1, 3)]
    );
    assert_eq!(
        diff_right.sub_iter(&diff_left).collect::<Vec<_>>(),
        vec![span(2, 1, 2)]
    );
    assert_eq!(
        diff_left
            .sub_iter_im(&diff_right.to_im_vv())
            .collect::<Vec<_>>(),
        vec![span(1, 1, 3)]
    );
    assert_eq!(
        diff_left.iter_between(&diff_right).collect::<Vec<_>>(),
        vec![span(1, 1, 3), span(2, 1, 2)]
    );
    assert_eq!(diff_left.sub_vec(&diff_right), span_map(&[(1, (1, 3))]));
    assert_eq!(diff_left.distance_between(&diff_right), 3);
    assert_eq!(zero_entry.distance_between(&diff_left), 4);
    assert_eq!(diff_left.to_spans(), span_map(&[(1, (0, 3)), (2, (0, 1))]));
    assert_eq!(
        diff_left.get_frontiers(),
        Frontiers::from([ID::new(1, 2), ID::new(2, 0)])
    );
    assert!(greater.includes_id(ID::new(1, 2)));
    assert!(!greater.includes_id(ID::new(1, 3)));
    assert!(greater.includes_vv(&greater));
    assert!(greater.includes_vv(&less));
    assert!(!greater.includes_vv(&incomparable));
    assert_eq!(greater.intersection(&less), vv_pairs(&[(1, 1), (2, 2)]));
    assert_eq!(zero_entry.intersection(&greater), VersionVector::new());
    assert_eq!(diff_left.get_missing_span(&diff_right), vec![span(2, 1, 2)]);
    assert_eq!(
        sorted_spans(VersionVector::new().get_missing_span(&diff_left)),
        vec![(1, 0, 3), (2, 0, 1)]
    );

    let mut adjust = VersionVector::new();
    adjust.set_last(ID::new(4, 0));
    assert_eq!(adjust.get_last(4), Some(0));
    assert_eq!(adjust.get(&4), Some(&1));
    assert!(adjust.try_update_last(ID::new(4, 1)));
    assert_eq!(adjust.get(&4), Some(&2));
    assert!(!adjust.try_update_last(ID::new(4, 0)));
    adjust.set_end(ID::new(4, 0));
    assert!(!adjust.contains_key(&4));
    adjust.set_end(ID::new(5, 3));
    assert_eq!(adjust.get(&5), Some(&3));
    adjust.set_end(ID::new(5, 0));
    assert!(!adjust.contains_key(&5));

    let mut span_ops = vv_pairs(&[(10, 2)]);
    span_ops.extend_to_include_last_id(ID::new(10, 3));
    assert_eq!(span_ops.get(&10), Some(&4));
    span_ops.extend_to_include_end_id(ID::new(10, 6));
    assert_eq!(span_ops.get(&10), Some(&6));
    span_ops.extend_to_include(span(10, 1, 8));
    assert_eq!(span_ops.get(&10), Some(&8));
    span_ops.shrink_to_exclude(span(10, 0, 2));
    assert!(!span_ops.contains_key(&10));

    let mut span_ops = VersionVector::new();
    span_ops.forward(&{
        let mut spans = IdSpanVector::default();
        spans.insert(7, CounterSpan::new(1, 3));
        spans.insert(8, CounterSpan::new(0, 2));
        spans
    });
    assert_eq!(span_ops, vv_pairs(&[(7, 3), (8, 2)]));
    span_ops.retreat(&{
        let mut spans = IdSpanVector::default();
        spans.insert(7, CounterSpan::new(1, 2));
        spans.insert(9, CounterSpan::new(0, 1));
        spans
    });
    assert_eq!(span_ops, vv_pairs(&[(7, 1), (8, 2)]));

    let encoded = greater.encode();
    assert_eq!(VersionVector::decode(&encoded)?, greater);
    let mut truncated = encoded.clone();
    truncated.pop();
    assert!(VersionVector::decode(&truncated).is_err());
    assert_eq!(ImVersionVector::decode(&encoded)?, greater.to_im_vv());
    assert!(ImVersionVector::decode(&truncated).is_err());

    let im = greater.to_im_vv();
    assert_eq!(VersionVector::from_im_vv(&im), greater);

    let mut im2 = ImVersionVector::new();
    assert!(im2.is_empty());
    im2.insert(1, 2);
    im2.insert(2, 4);
    assert_eq!(im2.len(), 2);
    assert_eq!(im2.get(&1), Some(&2));
    assert!(im2.contains_key(&2));
    *im2.get_mut(&2).unwrap() = 5;
    assert_eq!(im2.remove(&1), Some(2));
    im2.extend_to_include_vv(greater.iter());
    im2.merge(&im);
    im2.merge_vv(&less);
    im2.extend_to_include_last_id(ID::new(3, 3));
    assert!(im2.contains_key(&3));
    im2.set_last(ID::new(4, 1));
    assert_eq!(im2.to_vv().get(&4), Some(&2));
    let im_encoded = im2.encode();
    assert_eq!(ImVersionVector::decode(&im_encoded)?, im2);
    im2.clear();
    assert!(im2.is_empty());

    Ok(())
}

#[test]
fn version_range_contracts_follow_semantics() -> anyhow::Result<()> {
    let vv_from_ids = vv(&[ID::new(1, 3), ID::new(2, 2)]);
    let from_vv = VersionRange::from_vv(&vv_from_ids);
    assert_eq!(from_vv.get(&1), Some(&(0, 4)));
    assert_eq!(from_vv.get(&2), Some(&(0, 3)));
    assert!(from_vv.contains_id(ID::new(1, 0)));
    assert!(from_vv.contains_id(ID::new(1, 2)));
    assert!(!from_vv.contains_id(ID::new(1, 4)));
    assert!(from_vv.contains_id_span(span(1, 0, 3)));
    assert!(!from_vv.contains_id_span(span(1, 1, 5)));
    assert!(from_vv.has_overlap_with(span(1, 2, 5)));
    assert!(!from_vv.has_overlap_with(span(3, 0, 1)));

    let a = vv(&[ID::new(1, 2), ID::new(2, 1)]);
    let b = vv(&[ID::new(1, 1), ID::new(2, 2)]);
    let full = VersionRange::from_map(range_map(&[(1, (1, 3)), (2, (1, 3))]));
    assert!(full.contains_ops_between(&a, &b));
    let tight = VersionRange::from_map(range_map(&[(1, (2, 3)), (2, (1, 3))]));
    assert!(!tight.contains_ops_between(&a, &b));

    let mut extend = VersionRange::new();
    assert!(extend.is_empty());
    extend.extends_to_include_id_span(span(3, 4, 1));
    assert_eq!(extend.get(&3), Some(&(2, 5)));
    extend.extends_to_include_id_span(span(3, 0, 6));
    assert_eq!(extend.get(&3), Some(&(0, 6)));
    extend.insert(4, 2, 5);
    assert_eq!(extend.get(&4), Some(&(2, 5)));

    let mut items: Vec<_> = extend
        .iter()
        .map(|(peer, span)| (*peer, span.0, span.1))
        .collect();
    items.sort();
    assert_eq!(items, vec![(3, 0, 6), (4, 2, 5)]);

    extend.iter_mut().for_each(|(_, span)| {
        span.1 += 1;
    });
    assert_eq!(extend.get(&3), Some(&(0, 7)));
    assert_eq!(extend.get(&4), Some(&(2, 6)));
    extend.clear();
    assert!(extend.is_empty());

    Ok(())
}

#[test]
fn frontiers_contracts_follow_semantics() -> anyhow::Result<()> {
    let mut frontiers = Frontiers::new();
    assert!(frontiers.is_empty());
    assert_eq!(frontiers.as_single(), None);
    assert_eq!(frontiers.as_map(), None);

    frontiers.push(ID::new(1, 1));
    assert_eq!(frontiers, Frontiers::from_id(ID::new(1, 1)));
    frontiers.push(ID::new(1, 0));
    assert_eq!(frontiers, Frontiers::from_id(ID::new(1, 1)));
    frontiers.push(ID::new(1, 3));
    assert_eq!(frontiers, Frontiers::from_id(ID::new(1, 3)));
    frontiers.push(ID::new(2, 2));
    assert_eq!(sorted_ids(&frontiers), vec![(1, 3), (2, 2)]);
    assert!(frontiers.as_map().is_some());
    assert!(frontiers.contains(&ID::new(1, 3)));
    assert!(!frontiers.contains(&ID::new(1, 1)));
    assert_eq!(
        sorted_ids(&frontiers),
        sorted_ids(&Frontiers::from(vec![ID::new(1, 3), ID::new(2, 2)]))
    );

    let mut retained = frontiers.clone();
    retained.retain(|id| id.peer == 2);
    assert_eq!(retained, Frontiers::from_id(ID::new(2, 2)));
    retained.retain(|_| false);
    assert!(retained.is_empty());

    let mut removed = frontiers.clone();
    removed.remove(&ID::new(2, 2));
    assert_eq!(removed, Frontiers::from_id(ID::new(1, 3)));
    removed.remove(&ID::new(1, 3));
    assert_eq!(removed, Frontiers::None);

    let mut merged = Frontiers::new();
    merged.merge_with_greater(&frontiers);
    assert_eq!(merged, frontiers);

    let mut same_peer = Frontiers::from_id(ID::new(3, 1));
    same_peer.merge_with_greater(&Frontiers::from_id(ID::new(3, 4)));
    assert_eq!(same_peer, Frontiers::from_id(ID::new(3, 4)));

    let mut diff_peer = Frontiers::from_id(ID::new(3, 4));
    diff_peer.merge_with_greater(&Frontiers::from_id(ID::new(4, 2)));
    assert_eq!(sorted_ids(&diff_peer), vec![(3, 4), (4, 2)]);

    let mut map_merge = Frontiers::from([ID::new(1, 2), ID::new(3, 1)]);
    map_merge.merge_with_greater(&Frontiers::from([ID::new(1, 5), ID::new(2, 4)]));
    assert_eq!(sorted_ids(&map_merge), vec![(1, 5), (2, 4), (3, 1)]);

    let mut keep_one = Frontiers::from([ID::new(1, 1), ID::new(2, 2), ID::new(3, 3)]);
    keep_one.keep_one();
    assert_eq!(keep_one.len(), 1);
    assert!(keep_one.as_single().is_some());

    let deps = Frontiers::from([ID::new(10, 1), ID::new(11, 1)]);
    let mut next = deps.clone();
    next.update_frontiers_on_new_change(ID::new(12, 1), &deps);
    assert_eq!(next, Frontiers::from_id(ID::new(12, 1)));

    let deps = Frontiers::from([ID::new(1, 4), ID::new(4, 4)]);
    let mut next = Frontiers::from([ID::new(1, 4), ID::new(2, 2), ID::new(4, 4)]);
    next.update_frontiers_on_new_change(ID::new(5, 5), &deps);
    assert_eq!(sorted_ids(&next), vec![(2, 2), (5, 5)]);

    let from_option = Frontiers::from(Some(ID::new(7, 1)));
    assert_eq!(from_option, Frontiers::from_id(ID::new(7, 1)));
    assert_eq!(Frontiers::from(None::<ID>), Frontiers::None);
    assert_eq!(
        Frontiers::from([ID::new(8, 2)]),
        Frontiers::from_id(ID::new(8, 2))
    );
    assert_eq!(
        Frontiers::from(vec![ID::new(9, 3)]),
        Frontiers::from_id(ID::new(9, 3))
    );

    let encoded = next.encode();
    assert_eq!(Frontiers::decode(&encoded)?, next);
    let mut truncated = encoded.clone();
    truncated.pop();
    assert!(Frontiers::decode(&truncated).is_err());

    let doc = LoroDoc::new();
    doc.get_map("root").insert("a", 1)?;
    doc.commit();
    let fork = doc.fork();
    fork.get_map("root").insert("b", 2)?;
    fork.commit();
    doc.import(&fork.export(ExportMode::all_updates())?)?;

    let doc_frontiers = doc.oplog_frontiers();
    assert!(!doc_frontiers.is_empty());
    let vv = doc.frontiers_to_vv(&doc_frontiers).unwrap();
    assert_eq!(doc.vv_to_frontiers(&vv), doc_frontiers);
    assert_eq!(
        doc.frontiers_to_vv(&Frontiers::None),
        Some(VersionVector::new())
    );

    let foreign = LoroDoc::new();
    foreign.set_peer_id(77)?;
    foreign.get_text("foreign").insert(0, "x")?;
    foreign.commit();
    assert!(doc.frontiers_to_vv(&foreign.state_frontiers()).is_none());
    assert_eq!(
        doc.minimize_frontiers(&foreign.state_frontiers())
            .expect("foreign frontiers should remain unchanged"),
        foreign.state_frontiers()
    );

    let minimized = doc
        .minimize_frontiers(&doc_frontiers)
        .expect("frontiers from the same doc should be minimizable");
    assert_eq!(doc.frontiers_to_vv(&minimized), Some(vv));
    assert!(minimized.len() <= doc_frontiers.len());

    Ok(())
}
