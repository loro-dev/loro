use criterion::{criterion_group, criterion_main, Criterion};

#[cfg(feature = "test_utils")]
mod text_checkout {
    use std::{hint::black_box, sync::Arc, time::Duration};

    use criterion::{measurement::WallTime, BenchmarkGroup, BenchmarkId, Criterion};
    use loro_internal::{
        cursor::PosType,
        id::PeerID,
        loro::{CheckoutProfile, ExportMode, TextStateProfile},
        version::Frontiers,
        LoroDoc, Subscription,
    };
    use rand::{rngs::StdRng, Rng, SeedableRng};

    const TEXT: &str = "fn checkout_profile() { let value = document.version(); }\n";

    #[derive(Debug, Clone, Copy)]
    struct FixtureStats {
        scenario: &'static str,
        peer_count: usize,
        change_count: usize,
        base_len: usize,
        text_container_count: usize,
        large_text_container_count: usize,
        large_text_len: usize,
        version_count: usize,
        subscribed: bool,
    }

    struct CheckoutFixture {
        doc: LoroDoc,
        frontiers: Vec<Frontiers>,
        stats: FixtureStats,
        _subscription: Option<Subscription>,
    }

    #[derive(Debug, Default)]
    struct ProfileTotals {
        samples: u64,
        total: Duration,
        frontier_prepare: Duration,
        frontiers_to_vv: Duration,
        diff_calc: Duration,
        state_apply: Duration,
        emit_events: Duration,
        richtext_tracker_checkout: Duration,
        richtext_tracker_diff: Duration,
        richtext_delta_build: Duration,
        richtext_insert_future_scan: Duration,
        causal_vv_materialize: Duration,
        max_frontiers_width: usize,
        max_vv_width: usize,
        max_causal_vv_width: usize,
        max_diff_container_count: usize,
        richtext_tracker_checkout_count: u64,
        richtext_tracker_diff_count: u64,
        richtext_delta_build_count: u64,
        richtext_insert_future_scan_count: u64,
        richtext_insert_future_scan_visited: u64,
        richtext_insert_future_scan_max_visited: usize,
        causal_vv_materialize_count: u64,
        recording_event_samples: u64,
        forward_diff_calculator_samples: u64,
    }

    impl ProfileTotals {
        fn add(&mut self, profile: CheckoutProfile) {
            self.samples += 1;
            self.total += profile.total;
            self.frontier_prepare += profile.frontier_prepare;
            self.frontiers_to_vv += profile.frontiers_to_vv;
            self.diff_calc += profile.diff_calc;
            self.state_apply += profile.state_apply;
            self.emit_events += profile.emit_events;
            self.richtext_tracker_checkout += profile.richtext_tracker_checkout;
            self.richtext_tracker_diff += profile.richtext_tracker_diff;
            self.richtext_delta_build += profile.richtext_delta_build;
            self.richtext_insert_future_scan += profile.richtext_insert_future_scan;
            self.causal_vv_materialize += profile.causal_vv_materialize;
            self.max_frontiers_width = self
                .max_frontiers_width
                .max(profile.from_frontiers_len)
                .max(profile.to_frontiers_len);
            self.max_vv_width = self
                .max_vv_width
                .max(profile.from_vv_len)
                .max(profile.to_vv_len);
            self.max_causal_vv_width = self.max_causal_vv_width.max(profile.max_causal_vv_width);
            self.max_diff_container_count = self
                .max_diff_container_count
                .max(profile.diff_container_count);
            self.richtext_tracker_checkout_count += profile.richtext_tracker_checkout_count;
            self.richtext_tracker_diff_count += profile.richtext_tracker_diff_count;
            self.richtext_delta_build_count += profile.richtext_delta_build_count;
            self.richtext_insert_future_scan_count += profile.richtext_insert_future_scan_count;
            self.richtext_insert_future_scan_visited += profile.richtext_insert_future_scan_visited;
            self.richtext_insert_future_scan_max_visited = self
                .richtext_insert_future_scan_max_visited
                .max(profile.richtext_insert_future_scan_max_visited);
            self.causal_vv_materialize_count += profile.causal_vv_materialize_count;
            if profile.recording_events {
                self.recording_event_samples += 1;
            }
            if profile.forward_diff_calculator {
                self.forward_diff_calculator_samples += 1;
            }
        }
    }

    pub fn text_checkout(c: &mut Criterion) {
        let peer_count = env_usize("LORO_TEXT_CHECKOUT_PEERS", 1000).max(1);
        let base_len = env_usize("LORO_TEXT_CHECKOUT_BASE_LEN", 8192).max(1);
        let sequential_changes = env_usize("LORO_TEXT_CHECKOUT_CHANGES", peer_count.max(1000));
        let text_container_count = env_usize("LORO_TEXT_CHECKOUT_TEXT_CONTAINERS", 10_000).max(1);
        let large_text_container_count =
            env_usize("LORO_TEXT_CHECKOUT_LARGE_TEXT_CONTAINERS", 8).min(text_container_count);
        let small_text_len = env_usize("LORO_TEXT_CHECKOUT_SMALL_TEXT_LEN", 8);
        let large_text_len = env_usize("LORO_TEXT_CHECKOUT_LARGE_TEXT_LEN", 65_536);
        let container_edit_count =
            env_usize("LORO_TEXT_CHECKOUT_CONTAINER_EDITS", text_container_count).max(1);

        let mut group = c.benchmark_group("text checkout");
        group.sample_size(10);

        bench_fixture(
            &mut group,
            "plain/random-peer-checkout",
            build_concurrent_plain(peer_count, base_len, false, false),
        );
        bench_fixture(
            &mut group,
            "plain/same-position-peer-checkout",
            build_concurrent_plain(peer_count, base_len, true, false),
        );
        bench_fixture(
            &mut group,
            "plain/random-peer-checkout/subscribed",
            build_concurrent_plain(peer_count, base_len, false, true),
        );
        bench_fixture(
            &mut group,
            "plain/wide-causal-peer-checkout",
            build_wide_causal_plain(peer_count, base_len, false),
        );
        bench_fixture(
            &mut group,
            "rich/overlap-mark-peer-checkout",
            build_concurrent_rich_marks(peer_count, base_len, false),
        );
        bench_fixture(
            &mut group,
            "rich/overlap-mark-peer-checkout/subscribed",
            build_concurrent_rich_marks(peer_count, base_len, true),
        );
        bench_fixture(
            &mut group,
            "rich/unmark-style-peer-checkout",
            build_concurrent_rich_unmarks(peer_count, base_len, false),
        );
        bench_fixture(
            &mut group,
            "code/sequential-one-op-txn",
            build_code_like_history(sequential_changes, base_len, 1, false),
        );
        bench_fixture(
            &mut group,
            "code/sequential-eight-op-txn",
            build_code_like_history((sequential_changes / 8).max(1), base_len, 8, false),
        );
        bench_checkout_to_latest_fixture(
            &mut group,
            "code/checkout-to-latest-linear",
            build_code_like_history(sequential_changes, base_len, 1, false),
        );
        bench_checkout_latest_to_base_fixture(
            &mut group,
            "multi-container/latest-to-base",
            build_many_text_container_history(
                peer_count,
                text_container_count,
                large_text_container_count,
                small_text_len,
                large_text_len,
                container_edit_count,
                false,
            ),
        );

        group.finish();
    }

    fn bench_fixture(
        group: &mut BenchmarkGroup<'_, WallTime>,
        name: &str,
        fixture: CheckoutFixture,
    ) {
        let CheckoutFixture {
            doc,
            frontiers,
            stats,
            _subscription,
        } = fixture;
        let mut totals = ProfileTotals::default();
        let mut rng = StdRng::seed_from_u64(0x74ea_7c0d);
        let mut last_frontier_idx = usize::MAX;

        group.bench_with_input(
            BenchmarkId::new(name, stats.version_count),
            &frontiers,
            |b, frontiers| {
                b.iter_custom(|iters| {
                    let start = std::time::Instant::now();
                    for _ in 0..iters {
                        let mut frontier_idx = rng.gen_range(0..frontiers.len());
                        if frontiers.len() > 1 && frontier_idx == last_frontier_idx {
                            frontier_idx = (frontier_idx + 1) % frontiers.len();
                        }
                        last_frontier_idx = frontier_idx;
                        let frontier = &frontiers[frontier_idx];
                        let profile = doc.checkout_with_profile(frontier).unwrap();
                        totals.add(profile);
                        black_box(profile);
                    }

                    start.elapsed()
                });
            },
        );

        let state_profile = doc.text_state_profile("text");
        maybe_report_profile(name, stats, &totals, state_profile);
    }

    fn bench_checkout_latest_to_base_fixture(
        group: &mut BenchmarkGroup<'_, WallTime>,
        name: &str,
        fixture: CheckoutFixture,
    ) {
        let CheckoutFixture {
            doc,
            frontiers,
            stats,
            _subscription,
        } = fixture;
        let base_frontier = frontiers.first().unwrap().clone();
        let latest_frontier = frontiers.last().unwrap().clone();
        let mut totals = ProfileTotals::default();

        group.bench_with_input(
            BenchmarkId::new(name, stats.version_count),
            &base_frontier,
            |b, base_frontier| {
                b.iter_custom(|iters| {
                    let mut measured = Duration::ZERO;
                    for _ in 0..iters {
                        doc.checkout(&latest_frontier).unwrap();
                        let start = std::time::Instant::now();
                        let profile = doc.checkout_with_profile(base_frontier).unwrap();
                        measured += start.elapsed();
                        totals.add(profile);
                        black_box(profile);
                    }

                    measured
                });
            },
        );

        let state_profile = doc.text_state_profile("text");
        maybe_report_profile(name, stats, &totals, state_profile);
    }

    fn bench_checkout_to_latest_fixture(
        group: &mut BenchmarkGroup<'_, WallTime>,
        name: &str,
        fixture: CheckoutFixture,
    ) {
        let CheckoutFixture {
            doc,
            frontiers,
            stats,
            _subscription,
        } = fixture;
        let old_frontier_idx = if frontiers.len() > 2 {
            frontiers.len() / 2
        } else {
            0
        };
        let old_frontier = frontiers[old_frontier_idx].clone();
        let latest_frontier = frontiers.last().unwrap().clone();
        let mut totals = ProfileTotals::default();

        group.bench_with_input(
            BenchmarkId::new(name, stats.version_count),
            &latest_frontier,
            |b, latest_frontier| {
                b.iter_custom(|iters| {
                    let mut measured = Duration::ZERO;
                    for _ in 0..iters {
                        doc.checkout(&old_frontier).unwrap();
                        let start = std::time::Instant::now();
                        let profile = doc.checkout_with_profile(latest_frontier).unwrap();
                        measured += start.elapsed();
                        totals.add(profile);
                        black_box(profile);
                    }

                    measured
                });
            },
        );

        let state_profile = doc.text_state_profile("text");
        maybe_report_profile(name, stats, &totals, state_profile);
    }

    fn build_concurrent_plain(
        peer_count: usize,
        base_len: usize,
        same_position: bool,
        subscribed: bool,
    ) -> CheckoutFixture {
        let (snapshot, base_vv) = build_base_snapshot(base_len);
        let doc = LoroDoc::new_auto_commit();
        doc.import(&snapshot).unwrap();
        let mut frontiers = Vec::with_capacity(peer_count + 1);
        frontiers.push(doc.oplog_frontiers());
        let mut rng = StdRng::seed_from_u64(if same_position { 1 } else { 2 });

        for peer in 0..peer_count {
            let peer_doc = doc_from_snapshot(&snapshot, peer as PeerID + 2);
            let text = peer_doc.get_text("text");
            let pos = if same_position {
                0
            } else {
                rng.gen_range(0..=base_len)
            };
            text.insert(pos, "x", PosType::Unicode).unwrap();
            peer_doc.commit_then_renew();
            let update = peer_doc.export(ExportMode::updates(&base_vv)).unwrap();
            doc.import(&update).unwrap();
            frontiers.push(doc.oplog_frontiers());
        }

        attach_subscription(
            doc,
            frontiers,
            FixtureStats {
                scenario: if same_position {
                    "plain same-position concurrent inserts"
                } else {
                    "plain random concurrent inserts"
                },
                peer_count,
                change_count: peer_count,
                base_len,
                text_container_count: 1,
                large_text_container_count: 0,
                large_text_len: 0,
                version_count: peer_count + 1,
                subscribed,
            },
            subscribed,
        )
    }

    fn build_wide_causal_plain(
        peer_count: usize,
        base_len: usize,
        subscribed: bool,
    ) -> CheckoutFixture {
        let doc = LoroDoc::new_auto_commit();
        doc.set_peer_id(1).unwrap();
        let text = doc.get_text("text");
        let base = repeated_text(base_len);
        text.insert(0, &base, PosType::Unicode).unwrap();
        doc.commit_then_renew();
        let mut frontiers = Vec::with_capacity(peer_count + 1);
        frontiers.push(doc.oplog_frontiers());
        let mut rng = StdRng::seed_from_u64(6);

        for (peer, len) in (0..peer_count).zip(base_len..) {
            let snapshot = doc.export(ExportMode::snapshot()).unwrap();
            let base_vv = doc.oplog_vv();
            let peer_doc = doc_from_snapshot(&snapshot, peer as PeerID + 2);
            let text = peer_doc.get_text("text");
            let pos = rng.gen_range(0..=len);
            text.insert(pos, "x", PosType::Unicode).unwrap();
            peer_doc.commit_then_renew();
            let update = peer_doc.export(ExportMode::updates(&base_vv)).unwrap();
            doc.import(&update).unwrap();
            frontiers.push(doc.oplog_frontiers());
        }

        attach_subscription(
            doc,
            frontiers,
            FixtureStats {
                scenario: "plain sequential multi-peer edits with wide causal VV",
                peer_count,
                change_count: peer_count,
                base_len,
                text_container_count: 1,
                large_text_container_count: 0,
                large_text_len: 0,
                version_count: peer_count + 1,
                subscribed,
            },
            subscribed,
        )
    }

    fn build_concurrent_rich_marks(
        peer_count: usize,
        base_len: usize,
        subscribed: bool,
    ) -> CheckoutFixture {
        let (snapshot, base_vv) = build_base_snapshot(base_len);
        let doc = LoroDoc::new_auto_commit();
        doc.import(&snapshot).unwrap();
        let mut frontiers = Vec::with_capacity(peer_count + 1);
        frontiers.push(doc.oplog_frontiers());
        let mut rng = StdRng::seed_from_u64(3);
        let keys = ["bold", "italic", "comment"];

        for peer in 0..peer_count {
            let peer_doc = doc_from_snapshot(&snapshot, peer as PeerID + 2);
            let text = peer_doc.get_text("text");
            let start = rng.gen_range(0..base_len);
            let end = (start + rng.gen_range(1..=32)).min(base_len);
            text.mark(
                start,
                end,
                keys[peer % keys.len()],
                true.into(),
                PosType::Unicode,
            )
            .unwrap();
            peer_doc.commit_then_renew();
            let update = peer_doc.export(ExportMode::updates(&base_vv)).unwrap();
            doc.import(&update).unwrap();
            frontiers.push(doc.oplog_frontiers());
        }

        attach_subscription(
            doc,
            frontiers,
            FixtureStats {
                scenario: "rich text overlapping concurrent marks",
                peer_count,
                change_count: peer_count,
                base_len,
                text_container_count: 1,
                large_text_container_count: 0,
                large_text_len: 0,
                version_count: peer_count + 1,
                subscribed,
            },
            subscribed,
        )
    }

    fn build_concurrent_rich_unmarks(
        peer_count: usize,
        base_len: usize,
        subscribed: bool,
    ) -> CheckoutFixture {
        let (snapshot, base_vv) = build_styled_base_snapshot(base_len);
        let doc = LoroDoc::new_auto_commit();
        doc.import(&snapshot).unwrap();
        let mut frontiers = Vec::with_capacity(peer_count + 1);
        frontiers.push(doc.oplog_frontiers());
        let mut rng = StdRng::seed_from_u64(5);

        for peer in 0..peer_count {
            let peer_doc = doc_from_snapshot(&snapshot, peer as PeerID + 2);
            let text = peer_doc.get_text("text");
            let start = rng.gen_range(0..base_len);
            let end = (start + rng.gen_range(1..=32)).min(base_len).max(start + 1);
            text.unmark(start, end, "bold", PosType::Unicode).unwrap();
            peer_doc.commit_then_renew();
            let update = peer_doc.export(ExportMode::updates(&base_vv)).unwrap();
            doc.import(&update).unwrap();
            frontiers.push(doc.oplog_frontiers());
        }

        attach_subscription(
            doc,
            frontiers,
            FixtureStats {
                scenario: "rich text concurrent style deletion",
                peer_count,
                change_count: peer_count,
                base_len,
                text_container_count: 1,
                large_text_container_count: 0,
                large_text_len: 0,
                version_count: peer_count + 1,
                subscribed,
            },
            subscribed,
        )
    }

    fn build_code_like_history(
        change_count: usize,
        base_len: usize,
        ops_per_commit: usize,
        subscribed: bool,
    ) -> CheckoutFixture {
        let doc = LoroDoc::new_auto_commit();
        doc.set_peer_id(1).unwrap();
        let text = doc.get_text("text");
        let base = repeated_text(base_len);
        text.insert(0, &base, PosType::Unicode).unwrap();
        doc.commit_then_renew();
        let mut frontiers = Vec::with_capacity(change_count + 1);
        frontiers.push(doc.oplog_frontiers());
        let mut rng = StdRng::seed_from_u64(4 + ops_per_commit as u64);
        let mut len = base_len;

        for change in 0..change_count {
            for op in 0..ops_per_commit {
                if len > 0 && (change + op) % 5 == 0 {
                    let pos = rng.gen_range(0..len);
                    text.delete(pos, 1, PosType::Unicode).unwrap();
                    len -= 1;
                } else {
                    let token = if op % 2 == 0 { "\nlet x = 1;" } else { ";" };
                    let pos = rng.gen_range(0..=len);
                    text.insert(pos, token, PosType::Unicode).unwrap();
                    len += token.chars().count();
                }
            }
            doc.commit_then_renew();
            frontiers.push(doc.oplog_frontiers());
        }

        attach_subscription(
            doc,
            frontiers,
            FixtureStats {
                scenario: if ops_per_commit == 1 {
                    "code-like sequential one-op transactions"
                } else {
                    "code-like sequential multi-op transactions"
                },
                peer_count: 1,
                change_count,
                base_len,
                text_container_count: 1,
                large_text_container_count: 0,
                large_text_len: 0,
                version_count: change_count + 1,
                subscribed,
            },
            subscribed,
        )
    }

    fn build_many_text_container_history(
        peer_count: usize,
        text_container_count: usize,
        large_text_container_count: usize,
        small_text_len: usize,
        large_text_len: usize,
        edit_count: usize,
        subscribed: bool,
    ) -> CheckoutFixture {
        let doc = LoroDoc::new_auto_commit();
        doc.set_peer_id(1).unwrap();
        let small_text = repeated_text(small_text_len);
        let large_text = repeated_text(large_text_len);
        let mut texts = Vec::with_capacity(text_container_count);
        let mut lens = Vec::with_capacity(text_container_count);

        for idx in 0..text_container_count {
            let name = text_container_name(idx);
            let text = doc.get_text(name.as_str());
            let initial = if idx < large_text_container_count {
                &large_text
            } else {
                &small_text
            };
            if !initial.is_empty() {
                text.insert(0, initial, PosType::Unicode).unwrap();
            }
            texts.push(text);
            lens.push(initial.chars().count());
        }

        doc.commit_then_renew();
        let mut frontiers = Vec::with_capacity(edit_count + 1);
        frontiers.push(doc.oplog_frontiers());
        let mut rng = StdRng::seed_from_u64(0x7e57_c001);

        for edit in 0..edit_count {
            let peer = edit % peer_count;
            doc.set_peer_id(peer as PeerID + 2).unwrap();
            let text_idx = edit % text_container_count;
            let pos = rng.gen_range(0..=lens[text_idx]);
            texts[text_idx].insert(pos, "x", PosType::Unicode).unwrap();
            lens[text_idx] += 1;
            doc.commit_then_renew();
            frontiers.push(doc.oplog_frontiers());
        }

        attach_subscription(
            doc,
            frontiers,
            FixtureStats {
                scenario: "many text containers with wide multi-peer checkout",
                peer_count,
                change_count: edit_count,
                base_len: small_text_len,
                text_container_count,
                large_text_container_count,
                large_text_len,
                version_count: edit_count + 1,
                subscribed,
            },
            subscribed,
        )
    }

    fn build_base_snapshot(base_len: usize) -> (Vec<u8>, loro_internal::VersionVector) {
        let doc = LoroDoc::new_auto_commit();
        doc.set_peer_id(1).unwrap();
        let text = doc.get_text("text");
        let base = repeated_text(base_len);
        text.insert(0, &base, PosType::Unicode).unwrap();
        doc.commit_then_renew();
        (doc.export(ExportMode::snapshot()).unwrap(), doc.oplog_vv())
    }

    fn build_styled_base_snapshot(base_len: usize) -> (Vec<u8>, loro_internal::VersionVector) {
        let doc = LoroDoc::new_auto_commit();
        doc.set_peer_id(1).unwrap();
        let text = doc.get_text("text");
        let base = repeated_text(base_len);
        text.insert(0, &base, PosType::Unicode).unwrap();
        text.mark(0, base_len, "bold", true.into(), PosType::Unicode)
            .unwrap();
        doc.commit_then_renew();
        (doc.export(ExportMode::snapshot()).unwrap(), doc.oplog_vv())
    }

    fn doc_from_snapshot(snapshot: &[u8], peer: PeerID) -> LoroDoc {
        let doc = LoroDoc::new_auto_commit();
        doc.import(snapshot).unwrap();
        doc.set_peer_id(peer).unwrap();
        doc
    }

    fn attach_subscription(
        doc: LoroDoc,
        frontiers: Vec<Frontiers>,
        stats: FixtureStats,
        subscribed: bool,
    ) -> CheckoutFixture {
        let subscription = subscribed.then(|| {
            doc.subscribe_root(Arc::new(|event| {
                black_box(event);
            }))
        });

        CheckoutFixture {
            doc,
            frontiers,
            stats,
            _subscription: subscription,
        }
    }

    fn repeated_text(len: usize) -> String {
        let mut out = String::with_capacity(len);
        while out.len() < len {
            out.push_str(TEXT);
        }
        out.truncate(len);
        out
    }

    fn text_container_name(index: usize) -> String {
        if index == 0 {
            "text".to_string()
        } else {
            format!("text_{index}")
        }
    }

    fn env_usize(name: &str, default: usize) -> usize {
        std::env::var(name)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(default)
    }

    fn maybe_report_profile(
        name: &str,
        stats: FixtureStats,
        totals: &ProfileTotals,
        state_profile: Option<TextStateProfile>,
    ) {
        if std::env::var_os("LORO_TEXT_CHECKOUT_PROFILE").is_none() || totals.samples == 0 {
            return;
        }

        let samples = totals.samples as u32;
        let state_profile = state_profile.unwrap_or_default();
        let avg_future_scan_visited = totals
            .richtext_insert_future_scan_visited
            .checked_div(totals.richtext_insert_future_scan_count)
            .unwrap_or(0);
        eprintln!(
            concat!(
                "[text-checkout-profile] {name}: scenario={scenario}, peers={peers}, ",
                "changes={changes}, base_len={base_len}, versions={versions}, ",
                "text_containers={text_containers}, large_text_containers={large_text_containers}, ",
                "large_text_len={large_text_len}, ",
                "subscribed={subscribed}, samples={samples}, avg_total={avg_total:?}, ",
                "avg_frontier_prepare={avg_frontier_prepare:?}, ",
                "avg_frontiers_to_vv={avg_frontiers_to_vv:?}, avg_diff_calc={avg_diff_calc:?}, ",
                "avg_causal_vv_materialize={avg_causal_vv_materialize:?}, ",
                "causal_vv_materialize_calls={causal_vv_materialize_calls}, ",
                "max_causal_vv_width={max_causal_vv_width}, ",
                "avg_state_apply={avg_state_apply:?}, avg_emit_events={avg_emit_events:?}, ",
                "avg_richtext_tracker_checkout={avg_richtext_tracker_checkout:?}, ",
                "avg_richtext_tracker_diff={avg_richtext_tracker_diff:?}, ",
                "avg_richtext_delta_build={avg_richtext_delta_build:?}, ",
                "avg_richtext_insert_future_scan={avg_richtext_insert_future_scan:?}, ",
                "richtext_tracker_checkout_calls={richtext_tracker_checkout_calls}, ",
                "richtext_tracker_diff_calls={richtext_tracker_diff_calls}, ",
                "richtext_delta_build_calls={richtext_delta_build_calls}, ",
                "richtext_insert_future_scan_calls={richtext_insert_future_scan_calls}, ",
                "avg_future_scan_visited={avg_future_scan_visited}, ",
                "max_future_scan_visited={max_future_scan_visited}, ",
                "max_frontiers_width={max_frontiers_width}, max_vv_width={max_vv_width}, ",
                "max_diff_containers={max_diff_containers}, recording_event_samples={recording_event_samples}, ",
                "forward_diff_calculator_samples={forward_diff_calculator_samples}, ",
                "richtext_tree_nodes={richtext_tree_nodes}, richtext_chunks={richtext_chunks}, ",
                "text_chunks={text_chunks}, style_anchors={style_anchors}, ",
                "style_range_tree_nodes={style_range_tree_nodes}, style_range_chunks={style_range_chunks}"
            ),
            name = name,
            scenario = stats.scenario,
            peers = stats.peer_count,
            changes = stats.change_count,
            base_len = stats.base_len,
            versions = stats.version_count,
            text_containers = stats.text_container_count,
            large_text_containers = stats.large_text_container_count,
            large_text_len = stats.large_text_len,
            subscribed = stats.subscribed,
            samples = totals.samples,
            avg_total = totals.total / samples,
            avg_frontier_prepare = totals.frontier_prepare / samples,
            avg_frontiers_to_vv = totals.frontiers_to_vv / samples,
            avg_diff_calc = totals.diff_calc / samples,
            avg_causal_vv_materialize = totals.causal_vv_materialize / samples,
            causal_vv_materialize_calls = totals.causal_vv_materialize_count,
            max_causal_vv_width = totals.max_causal_vv_width,
            avg_state_apply = totals.state_apply / samples,
            avg_emit_events = totals.emit_events / samples,
            avg_richtext_tracker_checkout = totals.richtext_tracker_checkout / samples,
            avg_richtext_tracker_diff = totals.richtext_tracker_diff / samples,
            avg_richtext_delta_build = totals.richtext_delta_build / samples,
            avg_richtext_insert_future_scan = totals.richtext_insert_future_scan / samples,
            richtext_tracker_checkout_calls = totals.richtext_tracker_checkout_count,
            richtext_tracker_diff_calls = totals.richtext_tracker_diff_count,
            richtext_delta_build_calls = totals.richtext_delta_build_count,
            richtext_insert_future_scan_calls = totals.richtext_insert_future_scan_count,
            avg_future_scan_visited = avg_future_scan_visited,
            max_future_scan_visited = totals.richtext_insert_future_scan_max_visited,
            max_frontiers_width = totals.max_frontiers_width,
            max_vv_width = totals.max_vv_width,
            max_diff_containers = totals.max_diff_container_count,
            recording_event_samples = totals.recording_event_samples,
            forward_diff_calculator_samples = totals.forward_diff_calculator_samples,
            richtext_tree_nodes = state_profile.richtext_tree_node_count,
            richtext_chunks = state_profile.richtext_chunk_count,
            text_chunks = state_profile.text_chunk_count,
            style_anchors = state_profile.style_anchor_count,
            style_range_tree_nodes = state_profile.style_range_tree_node_count,
            style_range_chunks = state_profile.style_range_chunk_count,
        );
    }
}

pub fn dumb(_c: &mut Criterion) {}

#[cfg(feature = "test_utils")]
criterion_group!(benches, text_checkout::text_checkout);
#[cfg(not(feature = "test_utils"))]
criterion_group!(benches, dumb);
criterion_main!(benches);
