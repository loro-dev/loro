use loro::{ExportMode, Frontiers, LoroDoc, LoroError, ID};

#[test]
fn shallow_tail_spans_are_the_exact_imported_suffix() -> anyhow::Result<()> {
    let seed = LoroDoc::new();
    seed.set_peer_id(1)?;
    seed.get_text("pool").insert(0, "ab")?;
    seed.commit();
    let root = seed.oplog_frontiers();
    let shallow = LoroDoc::new();
    shallow.import(&seed.export(ExportMode::shallow_snapshot(&root))?)?;
    shallow.set_peer_id(1)?;
    shallow.get_text("pool").insert(2, "c")?;
    shallow.commit();
    let before = shallow.oplog_frontiers();
    let writer = shallow.fork();
    writer.set_peer_id(2)?;
    writer.get_text("pool").insert(0, "x")?;
    writer.commit();
    let update = writer.export(ExportMode::updates(&shallow.oplog_vv()))?;
    let status = shallow.import(&update)?;
    let after = shallow.oplog_frontiers();
    assert!(status.pending.is_none());
    let spans = shallow.find_id_spans_between(&before, &after);
    let forward: Vec<_> = spans
        .forward
        .iter()
        .map(|(peer, span)| (*peer, span.start, span.end))
        .collect();
    assert_eq!(forward, [(2, 0, 1)]);
    assert!(spans.retreat.is_empty());
    assert_eq!(shallow.try_find_id_spans_between(&before, &after)?, spans);
    Ok(())
}

#[test]
fn shallow_span_queries_reject_pruned_versions_and_match_full_history(
) -> Result<(), Box<dyn std::error::Error>> {
    let full = LoroDoc::new();
    full.set_peer_id(1)?;
    full.get_text("pool").insert(0, "aé𝄞bc")?;
    full.commit();
    let old = full.oplog_frontiers();
    full.get_text("pool").insert(1, "root")?;
    full.commit();
    let root = full.oplog_frontiers();
    let mut versions = vec![root.clone()];
    for peer in [2, 3] {
        full.set_peer_id(peer)?;
        full.get_text("pool").insert(0, "tail")?;
        full.commit();
        versions.push(full.oplog_frontiers());
    }
    let left = full.fork();
    left.set_peer_id(4)?;
    left.get_text("pool").insert(1, "left")?;
    left.commit();
    versions.push(left.oplog_frontiers());
    let right = full.fork();
    right.set_peer_id(5)?;
    right.get_text("pool").delete(2, 1)?;
    right.commit();
    versions.push(right.oplog_frontiers());
    full.import(&left.export(ExportMode::updates(&full.oplog_vv()))?)?;
    full.import(&right.export(ExportMode::updates(&full.oplog_vv()))?)?;
    versions.push(full.oplog_frontiers());
    full.set_peer_id(6)?;
    full.get_text("pool").insert(3, "after both")?;
    full.commit();
    versions.push(full.oplog_frontiers());
    let shallow = LoroDoc::new();
    shallow.import(&full.export(ExportMode::shallow_snapshot(&root))?)?;
    assert_eq!(shallow.shallow_since_frontiers(), root);
    let unknown = Frontiers::from(ID::new(99, 0));
    for from in [&Frontiers::default(), &old] {
        for to in [from, &root, &versions[versions.len() - 1]] {
            assert!(matches!(
                shallow.try_find_id_spans_between(from, to),
                Err(LoroError::SwitchToVersionBeforeShallowRoot)
            ));
            assert!(matches!(
                shallow.try_find_id_spans_between(to, from),
                Err(LoroError::SwitchToVersionBeforeShallowRoot)
            ));
        }
    }
    assert!(matches!(
        shallow.try_find_id_spans_between(&unknown, &unknown),
        Err(LoroError::FrontiersNotFound(_))
    ));
    for from in &versions {
        for to in &versions {
            let expected = full
                .frontiers_to_vv(from)
                .ok_or("full from")?
                .diff(&full.frontiers_to_vv(to).ok_or("full to")?);
            assert_eq!(
                shallow.try_find_id_spans_between(from, to)?,
                expected,
                "from={from:?} to={to:?}"
            );
            assert_eq!(shallow.find_id_spans_between(from, to), expected);
        }
    }
    assert_eq!(shallow.get_deep_value(), full.get_deep_value());
    Ok(())
}
