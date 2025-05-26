#![allow(unexpected_cfgs)]
use loro::LoroDoc;

#[ctor::ctor]
fn init() {
    dev_utils::setup_test_log();
}

#[test]
fn issue_0() {
    let bytes = include_bytes!("./issue_0.bin");
    let doc = LoroDoc::new();
    doc.import_batch(&[bytes.into()]).unwrap();
    #[allow(deprecated)]
    doc.export_snapshot();
    doc.export(loro::ExportMode::Snapshot).unwrap();
}

#[test]
fn test_browser_throttling_simulation_heavy_operations() {
    // This test simulates browser throttling by performing many operations
    // with frequent state checks that might trigger lock conflicts
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();

    // Simulate heavy document operations that might trigger auto-commits
    for i in 0..100 {
        // Insert operations that will trigger auto-commit
        doc.get_text("text")
            .insert(0, &format!("Operation {}", i))
            .unwrap();

        // Frequent state checks (simulating what browser might do during throttling)
        if i % 10 == 0 {
            let _frontiers = doc.state_frontiers();
            let _oplog_vv = doc.oplog_vv();
            let _state_vv = doc.state_vv();
            let _deep_value = doc.get_deep_value();
        }

        // Mixed container operations
        if i % 15 == 0 {
            doc.get_map("map")
                .insert(&format!("key_{}", i), i as i64)
                .unwrap();
            doc.get_list("list").push(i as i64).unwrap();
        }

        // Occasionally force commits (simulating user actions)
        if i % 25 == 0 {
            doc.commit();
        }
    }

    // Final operations that might trigger the problematic state
    doc.commit();
    let _final_state = doc.get_deep_value();
}

#[test]
fn test_rapid_auto_commit_with_state_inspection() {
    // Test rapid operations that trigger auto-commits with concurrent state inspection
    let doc = LoroDoc::new();
    doc.set_peer_id(2).unwrap();

    let text = doc.get_text("rapid_text");
    let map = doc.get_map("rapid_map");
    let list = doc.get_list("rapid_list");

    // Rapid operations that should trigger auto-commits
    for i in 0..50 {
        // Multiple operations in quick succession
        text.insert(0, "a").unwrap();
        map.insert(&format!("k{}", i), i as i64).unwrap();
        list.push(i as i64).unwrap();

        // Immediately check state (simulating browser state inspection during throttling)
        let _vv = doc.oplog_vv();
        let _frontiers = doc.state_frontiers();

        // More operations
        text.insert(1, "b").unwrap();
        map.insert(&format!("j{}", i), (i * 2) as i64).unwrap();

        // Check if we're detached (might trigger internal operations)
        let _is_detached = doc.is_detached();

        if i % 7 == 0 {
            // Force commit which might cause lock contention
            doc.commit();
            let _value = doc.get_value();
        }
    }
}

#[test]
fn test_commit_with_immediate_operations() {
    // Test scenarios where commits happen immediately followed by operations
    // This might reproduce the commit_with immediate_renew issue
    let doc = LoroDoc::new();
    doc.set_peer_id(3).unwrap();

    for i in 0..30 {
        // Setup some state
        doc.get_text("text")
            .insert(0, &format!("Setup {}", i))
            .unwrap();

        // Commit with options (might trigger immediate renewal)
        doc.commit_with(loro::CommitOptions::default());

        // Immediately check state and perform more operations
        let _frontiers = doc.state_frontiers();
        doc.get_map("post_commit")
            .insert(&format!("after_commit_{}", i), i as i64)
            .unwrap();

        // Check version vectors which might trigger internal lock operations
        let _oplog_vv = doc.oplog_vv();
        let _state_vv = doc.state_vv();

        // More operations that might conflict with auto-commit
        doc.get_list("operations")
            .push(format!("op_{}", i))
            .unwrap();

        if i % 5 == 0 {
            // Export operations that force commits
            let _export = doc.export(loro::ExportMode::Snapshot).unwrap();
            // Immediately perform operations
            doc.get_text("after_export").insert(0, "immediate").unwrap();
        }
    }
}

#[test]
fn test_auto_commit_renewal_scenarios() {
    // Test scenarios that might trigger renew_txn_if_auto_commit issues
    let doc = LoroDoc::new();
    doc.set_peer_id(4).unwrap();

    // Create complex nested container structure
    let root_map = doc.get_map("root");
    let nested_text = root_map
        .insert_container("nested_text", loro::LoroText::new())
        .unwrap();
    let nested_list = root_map
        .insert_container("nested_list", loro::LoroList::new())
        .unwrap();

    for i in 0..25 {
        // Operations on nested containers (might trigger auto-commit renewal)
        nested_text.insert(0, &format!("nested_{}", i)).unwrap();
        nested_list.push(i as i64).unwrap();

        // State inspection that might conflict with transaction renewal
        let _deep_value = doc.get_deep_value();
        let _oplog_frontiers = doc.oplog_frontiers();

        // More complex operations
        if i % 3 == 0 {
            let sub_map = nested_list
                .insert_container(0, loro::LoroMap::new())
                .unwrap();
            sub_map
                .insert("sub_key", format!("sub_value_{}", i))
                .unwrap();
        }

        // Operations that might trigger transaction boundaries
        if i % 6 == 0 {
            doc.commit();
            // Immediately check state (might trigger renewal conflicts)
            let _state_frontiers = doc.state_frontiers();
            let _is_detached = doc.is_detached();
        }

        // Check container states
        let _text_len = nested_text.len_unicode();
        let _list_len = nested_list.len();
    }
}

#[test]
fn test_concurrent_state_operations() {
    // Test operations that might cause lock ordering violations through state access
    let doc = LoroDoc::new();
    doc.set_peer_id(5).unwrap();

    for round in 0..20 {
        // Multiple operations that accumulate in auto-commit transaction
        for i in 0..10 {
            doc.get_text("concurrent")
                .insert(i, &format!("{}", round))
                .unwrap();
            doc.get_map("data")
                .insert(&format!("{}_{}", round, i), i as i64)
                .unwrap();
        }

        // Simultaneous state checks (simulating browser doing multiple things)
        let _vv1 = doc.oplog_vv();
        let _vv2 = doc.state_vv();
        let _frontiers1 = doc.oplog_frontiers();
        let _frontiers2 = doc.state_frontiers();
        let _deep_value = doc.get_deep_value();
        let _shallow_value = doc.get_value();

        // More operations that might conflict with state access
        doc.get_list("more_ops").push(round as i64).unwrap();

        // Operations that might trigger exports/imports (which force commits)
        if round % 4 == 0 {
            let export_data = doc.export(loro::ExportMode::Snapshot).unwrap();
            // Immediately perform more operations
            doc.get_text("after_export").push_str("immediate").unwrap();

            // Create a second doc and import (might trigger state conflicts in original)
            let doc2 = LoroDoc::new();
            doc2.import(&export_data).unwrap();

            // Back to original doc - more operations
            doc.get_map("post_import").insert("key", "value").unwrap();
        }

        // Check various states that might trigger lock acquisition
        let _len_ops = doc.len_ops();
        let _len_changes = doc.len_changes();
        let _is_detached = doc.is_detached();
    }

    // Final commit that processes all accumulated operations
    doc.commit();
}

#[test]
fn test_stress_auto_commit_boundaries() {
    // Stress test auto-commit boundaries with rapid operations and state checks
    let doc = LoroDoc::new();
    doc.set_peer_id(6).unwrap();

    // Setup initial state
    let text = doc.get_text("stress_text");
    let map = doc.get_map("stress_map");
    let list = doc.get_list("stress_list");

    // Rapid operations that stress the auto-commit system
    for batch in 0..15 {
        // Burst of operations
        for i in 0..20 {
            text.insert(0, "x").unwrap();
            map.insert(&format!("batch_{}_{}", batch, i), i as i64)
                .unwrap();
            list.push(format!("item_{}_{}", batch, i)).unwrap();

            // Frequent state access during operations
            if i % 3 == 0 {
                let _frontiers = doc.state_frontiers();
                let _vv = doc.oplog_vv();
            }
        }

        // State inspection that might conflict with auto-commit
        let _deep_state = doc.get_deep_value();
        let _text_content = text.to_string();
        let _map_len = map.len();
        let _list_len = list.len();

        // Force commit and immediately perform operations
        doc.commit();
        text.insert(0, "post_commit").unwrap();

        // Rapid state checks after commit
        let _state_after_commit = doc.state_frontiers();
        let _oplog_after_commit = doc.oplog_frontiers();

        // More operations that start new transaction
        map.insert("immediate_after_commit", batch as i64).unwrap();
        list.push("immediate").unwrap();
    }
}

#[test]
fn test_extreme_commit_with_immediate_renewal() {
    // This test tries to trigger the specific pattern mentioned in the analysis:
    // commit_with(immediate_renew=true) followed by immediate operations
    let doc = LoroDoc::new();
    doc.set_peer_id(7).unwrap();

    for round in 0..50 {
        // Build up transaction content
        for i in 0..5 {
            doc.get_text("immediate_renew")
                .insert(0, &format!("op_{}_{}", round, i))
                .unwrap();
            doc.get_map("immediate_data")
                .insert(&format!("key_{}_{}", round, i), i as i64)
                .unwrap();
        }

        // Commit with default options (which might have immediate_renew)
        doc.commit_with(loro::CommitOptions::default());

        // IMMEDIATELY perform operations that might conflict with transaction renewal
        doc.get_text("post_commit_immediate")
            .insert(0, "immediate")
            .unwrap();

        // IMMEDIATELY check state which might trigger OpLog lock while Txn might be locked
        let _state_vv = doc.state_vv();
        let _oplog_vv = doc.oplog_vv();

        // More immediate operations
        doc.get_list("immediate_list")
            .push(format!("round_{}", round))
            .unwrap();

        // Multiple immediate state checks
        let _frontiers = doc.state_frontiers();
        let _deep_value = doc.get_deep_value();
        let _oplog_frontiers = doc.oplog_frontiers();

        // Immediate container state checks
        let text = doc.get_text("immediate_renew");
        let _text_len = text.len_unicode();
        let _text_value = text.to_string();
    }
}

#[test]
fn test_interleaved_commit_and_operations() {
    // Test pattern that might trigger lock conflicts through interleaved operations
    let doc = LoroDoc::new();
    doc.set_peer_id(8).unwrap();

    for i in 0..100 {
        // Operations that will accumulate in auto-commit transaction
        doc.get_text("interleaved")
            .insert(0, &format!("op_{}", i))
            .unwrap();

        // Every few operations, force commit and immediately do more operations
        if i % 8 == 0 {
            doc.commit();

            // Immediate post-commit operations
            doc.get_map("post_commit")
                .insert(&format!("immediate_{}", i), i as i64)
                .unwrap();

            // Immediate state queries
            let _state = doc.state_frontiers();
            let _oplog = doc.oplog_frontiers();

            // More immediate operations
            doc.get_list("immediate_ops").push(i as i64).unwrap();
        }

        // Frequent state checks during operations accumulation
        if i % 3 == 0 {
            let _vv = doc.oplog_vv();
            let _state_vv = doc.state_vv();
        }

        // Additional operations
        if i % 5 == 0 {
            doc.get_map("frequent")
                .insert(&format!("key_{}", i), format!("value_{}", i))
                .unwrap();
        }
    }
}

#[test]
fn test_export_import_immediate_operations() {
    // Test pattern involving export/import operations with immediate follow-ups
    let doc = LoroDoc::new();
    doc.set_peer_id(9).unwrap();

    for i in 0..20 {
        // Build up some state
        doc.get_text("export_test")
            .insert(0, &format!("content_{}", i))
            .unwrap();
        doc.get_map("export_map")
            .insert(&format!("key_{}", i), i as i64)
            .unwrap();

        if i % 4 == 0 {
            // Export operation (forces commit)
            let export_data = doc.export(loro::ExportMode::Snapshot).unwrap();

            // IMMEDIATELY perform operations (might cause lock conflicts)
            doc.get_text("post_export")
                .insert(0, "immediate_after_export")
                .unwrap();

            // IMMEDIATELY check state
            let _state = doc.state_frontiers();
            let _oplog = doc.oplog_frontiers();

            // Create second doc and import
            let doc2 = LoroDoc::new();
            doc2.import(&export_data).unwrap();

            // IMMEDIATELY continue with original doc operations
            doc.get_list("continuing_ops")
                .push(format!("after_export_{}", i))
                .unwrap();

            // More immediate state checks
            let _deep_value = doc.get_deep_value();
            let _vv = doc.oplog_vv();

            // Additional immediate operations that might conflict
            doc.get_map("conflict_test")
                .insert(&format!("post_export_{}", i), "conflict")
                .unwrap();
        }

        // Regular state monitoring (simulating browser activity)
        let _current_state = doc.get_value();
        let _is_detached = doc.is_detached();
    }
}
