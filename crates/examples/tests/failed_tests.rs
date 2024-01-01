use std::sync::Arc;

use examples::json::fuzz;
use loro::loro_value;

#[test]
fn fuzz_json() {
    use examples::test_preload::*;
    fuzz(
        5,
        &[
            Action {
                peer: 5280832617179597129,
                action: InsertList {
                    index: 311,
                    value: Bool(true),
                },
            },
            Action {
                peer: 8174158055725953393,
                action: DeleteList { index: 341 },
            },
            Sync {
                from: 18446543177843820913,
                to: 5280832620235194367,
                kind: Fit,
            },
            Action {
                peer: 8174439530700032329,
                action: DeleteList { index: 341 },
            },
            Sync {
                from: 8174439528799404056,
                to: 8174439530702664049,
                kind: Snapshot,
            },
            Action {
                peer: 8174439043468105841,
                action: DeleteList { index: 341 },
            },
            Action {
                peer: 5280832617179597129,
                action: InsertList {
                    index: 311,
                    value: Bool(true),
                },
            },
            Action {
                peer: 5280832617179597129,
                action: InsertList {
                    index: 341,
                    value: Bool(true),
                },
            },
            Action {
                peer: 8174439139858008393,
                action: DeleteList { index: 341 },
            },
            Sync {
                from: 8174439393263710577,
                to: 7586675626393291081,
                kind: Fit,
            },
            Sync {
                from: 8174439530702664049,
                to: 8174439530702664049,
                kind: Fit,
            },
            Action {
                peer: 5280832685899073865,
                action: InsertList {
                    index: 351,
                    value: Bool(true),
                },
            },
            Action {
                peer: 5280832789652009216,
                action: InsertList {
                    index: 311,
                    value: Bool(true),
                },
            },
            Sync {
                from: 8174439358230251889,
                to: 8174439530702664049,
                kind: Snapshot,
            },
            Action {
                peer: 8174439530700032329,
                action: DeleteList { index: 341 },
            },
            Sync {
                from: 5280832617178745161,
                to: 5280832617179597129,
                kind: Snapshot,
            },
            Action {
                peer: 5280832616743389513,
                action: InsertList {
                    index: 311,
                    value: Bool(true),
                },
            },
            Sync {
                from: 5280832617853317489,
                to: 8174439530702664049,
                kind: Snapshot,
            },
            Action {
                peer: 5280832617179593801,
                action: InsertList {
                    index: 311,
                    value: Bool(true),
                },
            },
            Action {
                peer: 5280876597644708169,
                action: DeleteList { index: 341 },
            },
            Sync {
                from: 8174439530702664049,
                to: 5280876770117120369,
                kind: Pending,
            },
            SyncAll,
            Action {
                peer: 8174439358230251849,
                action: DeleteText {
                    index: 960,
                    len: 126,
                },
            },
            Action {
                peer: 18404522827202906441,
                action: InsertList {
                    index: 311,
                    value: Bool(true),
                },
            },
            Action {
                peer: 8174439530700032329,
                action: DeleteList { index: 341 },
            },
            Sync {
                from: 8174439528799404056,
                to: 5292135769185546609,
                kind: Fit,
            },
            Action {
                peer: 5280832617179596873,
                action: InsertList {
                    index: 311,
                    value: Bool(true),
                },
            },
            Action {
                peer: 5292135596713134409,
                action: InsertList {
                    index: 311,
                    value: Bool(true),
                },
            },
            Sync {
                from: 8174439526407696753,
                to: 8174439530702664049,
                kind: Snapshot,
            },
            Action {
                peer: 8174439498734632959,
                action: InsertList {
                    index: 311,
                    value: Bool(true),
                },
            },
            Action {
                peer: 5833687803971913,
                action: InsertList {
                    index: 301,
                    value: Bool(true),
                },
            },
            Sync {
                from: 8174439530702664049,
                to: 8174439530702664049,
                kind: Fit,
            },
            SyncAll,
            SyncAll,
            Action {
                peer: 5280832617179597129,
                action: DeleteList { index: 311 },
            },
            Sync {
                from: 8174439530702664049,
                to: 8174439530702664049,
                kind: Fit,
            },
            Sync {
                from: 8163136378478610801,
                to: 8174439139858008393,
                kind: Snapshot,
            },
            Sync {
                from: 8174439530702664049,
                to: 18395314732082491761,
                kind: Snapshot,
            },
            Action {
                peer: 5280832617179597129,
                action: InsertList {
                    index: 303,
                    value: Bool(true),
                },
            },
            Sync {
                from: 8174439530702664049,
                to: 8174439530702664049,
                kind: Fit,
            },
            Action {
                peer: 8174412969951185225,
                action: InsertList {
                    index: 351,
                    value: Bool(true),
                },
            },
            Sync {
                from: 8174439530702664049,
                to: 8163136378699346289,
                kind: Snapshot,
            },
            Sync {
                from: 5280876770117120369,
                to: 5280832617179597116,
                kind: Fit,
            },
            Action {
                peer: 5280832634359466315,
                action: DeleteList { index: 341 },
            },
            Sync {
                from: 8174439530702664049,
                to: 8174439358230251849,
                kind: Snapshot,
            },
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            Sync {
                from: 16090538600105537827,
                to: 936747706152398848,
                kind: Pending,
            },
            Action {
                peer: 8174439530702653769,
                action: DeleteList { index: 341 },
            },
            Sync {
                from: 8174395377765151089,
                to: 8174439530702664049,
                kind: Snapshot,
            },
            Action {
                peer: 5280832617179597129,
                action: InsertList {
                    index: 311,
                    value: Bool(true),
                },
            },
            Action {
                peer: 8174439530700032329,
                action: DeleteList { index: 341 },
            },
            Sync {
                from: 5277173443156078961,
                to: 5280832617179597129,
                kind: Fit,
            },
            Action {
                peer: 8174439358230513993,
                action: InsertList {
                    index: 351,
                    value: Bool(true),
                },
            },
            Sync {
                from: 8174439530702664049,
                to: 5280832617178745161,
                kind: Fit,
            },
            Action {
                peer: 5280832617179728201,
                action: DeleteList { index: 341 },
            },
            Sync {
                from: 18446744073709515121,
                to: 18446744073709551615,
                kind: Pending,
            },
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            Action {
                peer: 18446744073709551615,
                action: DeleteText {
                    index: 960,
                    len: 126,
                },
            },
            Sync {
                from: 1412722910008930673,
                to: 18380137171932733261,
                kind: Snapshot,
            },
            Action {
                peer: 0,
                action: InsertMap {
                    key: "".into(),
                    value: Null,
                },
            },
        ],
    )
}

#[test]
fn fuzz_json_1() {
    use examples::test_preload::*;
    let mut map = loro_value!({"": "test"});
    for _ in 0..64 {
        map = loro_value!({"": map});
    }

    let mut list = loro_value!([map]);
    for _ in 0..64 {
        list = loro_value!([list, 9]);
    }

    fuzz(
        5,
        &[Action {
            peer: 35184913762633,
            action: InsertMap {
                key: "\0IIIIIIIIIIIIIIIIIII\0\0".into(),
                value: list,
            },
        }],
    );
}

#[test]
fn fuzz_json_2() {
    use examples::test_preload::*;
    fuzz(
        5,
        &[
            Action {
                peer: 13835902481151819776,
                action: InsertText {
                    index: 1023,
                    s: ";\0".into(),
                },
            },
            Action {
                peer: 6701356245527298097,
                action: InsertList {
                    index: 0,
                    value: Null,
                },
            },
            Sync {
                from: 97661,
                to: 8725724278038272,
                kind: Fit,
            },
            Action {
                peer: 16600305609883097344,
                action: InsertList {
                    index: 305,
                    value: Bool(true),
                },
            },
            Action {
                peer: 4683750489798492481,
                action: InsertList {
                    index: 305,
                    value: Bool(true),
                },
            },
            Action {
                peer: 4702111234474983745,
                action: InsertList {
                    index: 825,
                    value: Bool(true),
                },
            },
            Action {
                peer: 4702111234474983745,
                action: InsertList {
                    index: 305,
                    value: Bool(true),
                },
            },
            Action {
                peer: 4702111234474983745,
                action: DeleteList { index: 305 },
            },
            Action {
                peer: 5278571986778407233,
                action: InsertList {
                    index: 305,
                    value: Bool(true),
                },
            },
            Action {
                peer: 4702111234474983745,
                action: InsertList {
                    index: 305,
                    value: Bool(true),
                },
            },
            Action {
                peer: 4702111105625964865,
                action: InsertList {
                    index: 305,
                    value: Bool(true),
                },
            },
            Action {
                peer: 4702111234474983745,
                action: InsertList {
                    index: 309,
                    value: Bool(true),
                },
            },
            Action {
                peer: 4702111235867492673,
                action: InsertList {
                    index: 305,
                    value: Bool(true),
                },
            },
            Action {
                peer: 4702111234474983878,
                action: InsertList {
                    index: 305,
                    value: Bool(true),
                },
            },
            Action {
                peer: 4702111234474983745,
                action: InsertList {
                    index: 305,
                    value: Bool(true),
                },
            },
            Action {
                peer: 4702111234474983745,
                action: DeleteText {
                    index: 960,
                    len: 126,
                },
            },
            Action {
                peer: 4702141920927629633,
                action: InsertList {
                    index: 333,
                    value: loro_value!("}}}&}}}}}}}"),
                },
            },
            Sync {
                from: 9042521604759584125,
                to: 4702111234478931325,
                kind: Fit,
            },
            Action {
                peer: 4702111234474983745,
                action: InsertList {
                    index: 388,
                    value: Bool(true),
                },
            },
            Action {
                peer: 4702111234474983745,
                action: InsertList {
                    index: 305,
                    value: Bool(true),
                },
            },
            Action {
                peer: 4702111234474983745,
                action: InsertList {
                    index: 305,
                    value: Bool(true),
                },
            },
            Action {
                peer: 4702111268834722113,
                action: InsertList {
                    index: 305,
                    value: Bool(true),
                },
            },
            Action {
                peer: 18446744070509379905,
                action: DeleteText {
                    index: 960,
                    len: 126,
                },
            },
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            Action {
                peer: 4702142020800561473,
                action: InsertList {
                    index: 305,
                    value: Bool(true),
                },
            },
            Action {
                peer: 4611686019352054603,
                action: InsertList {
                    index: 580,
                    value: I32(1094795619),
                },
            },
            Action {
                peer: 4702111195820802369,
                action: InsertList {
                    index: 305,
                    value: Bool(true),
                },
            },
            Action {
                peer: 10682891539623002433,
                action: InsertList {
                    index: 305,
                    value: Bool(true),
                },
            },
            Action {
                peer: 4702111234474983745,
                action: InsertList {
                    index: 305,
                    value: Bool(true),
                },
            },
            Action {
                peer: 4702111234474983745,
                action: InsertList {
                    index: 305,
                    value: Bool(true),
                },
            },
            Action {
                peer: 4702111234474983745,
                action: InsertList {
                    index: 305,
                    value: Bool(true),
                },
            },
            Action {
                peer: 4702111234474983745,
                action: InsertList {
                    index: 305,
                    value: Bool(true),
                },
            },
            Action {
                peer: 4702111234474983745,
                action: InsertList {
                    index: 305,
                    value: Bool(true),
                },
            },
            Action {
                peer: 5278571986778407233,
                action: InsertList {
                    index: 305,
                    value: Bool(true),
                },
            },
            Action {
                peer: 18393054101681291585,
                action: DeleteList { index: 350 },
            },
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            Action {
                peer: 4702111234474983745,
                action: InsertList {
                    index: 305,
                    value: loro_value!("\0*8AAAAAA"),
                },
            },
            Sync {
                from: 9042521604759584125,
                to: 9042521604759978877,
                kind: Snapshot,
            },
            Sync {
                from: 4702111234474983745,
                to: 4702111234474983745,
                kind: Fit,
            },
            Action {
                peer: 4702111234480423233,
                action: InsertList {
                    index: 305,
                    value: Bool(true),
                },
            },
            Action {
                peer: 4702111234474983745,
                action: InsertList {
                    index: 305,
                    value: Bool(true),
                },
            },
            Action {
                peer: 4702111234474983745,
                action: InsertList {
                    index: 305,
                    value: Bool(true),
                },
            },
            Action {
                peer: 4702111234474983745,
                action: InsertList {
                    index: 305,
                    value: Bool(true),
                },
            },
            Action {
                peer: 4702111234474983745,
                action: InsertList {
                    index: 305,
                    value: Bool(true),
                },
            },
            Action {
                peer: 4702111234474983745,
                action: InsertList {
                    index: 305,
                    value: Bool(true),
                },
            },
            Action {
                peer: 4702111234475508033,
                action: InsertList {
                    index: 305,
                    value: Bool(true),
                },
            },
            Action {
                peer: 18446744073709502785,
                action: DeleteText {
                    index: 960,
                    len: 126,
                },
            },
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            Action {
                peer: 4702111234944745793,
                action: InsertList {
                    index: 305,
                    value: Bool(true),
                },
            },
            Action {
                peer: 3242380625474238493,
                action: InsertList {
                    index: 843,
                    value: Bool(true),
                },
            },
            Action {
                peer: 4702111234473083201,
                action: InsertList {
                    index: 305,
                    value: Bool(true),
                },
            },
            Action {
                peer: 4704363034288668993,
                action: InsertList {
                    index: 305,
                    value: Bool(true),
                },
            },
            Action {
                peer: 18446534347256316225,
                action: DeleteText {
                    index: 960,
                    len: 126,
                },
            },
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            Action {
                peer: 4702111234476818753,
                action: InsertList {
                    index: 305,
                    value: Bool(true),
                },
            },
            Action {
                peer: 7146367359073124408,
                action: InsertList {
                    index: 809,
                    value: Bool(true),
                },
            },
            Action {
                peer: 4702104637405217089,
                action: InsertList {
                    index: 388,
                    value: Bool(true),
                },
            },
            Action {
                peer: 4702111234474983745,
                action: InsertList {
                    index: 305,
                    value: Bool(true),
                },
            },
            Action {
                peer: 4702111234474983745,
                action: InsertList {
                    index: 305,
                    value: Bool(true),
                },
            },
            Action {
                peer: 4702111268834722113,
                action: InsertList {
                    index: 305,
                    value: Bool(true),
                },
            },
            SyncAll,
            SyncAll,
            Action {
                peer: 4702111354343939393,
                action: InsertText {
                    index: 350,
                    s: "}}}&}}}}}}}".into(),
                },
            },
            Sync {
                from: 9042521604759584125,
                to: 4702111234478931325,
                kind: Fit,
            },
            Action {
                peer: 4702111234474983745,
                action: InsertMap {
                    key: "AAAAAAAAAAAAAAAAAAA".into(),
                    value: Binary(Arc::new(vec![
                        65, 65, 65, 65, 65, 65, 65, 65, 65, 65, 65, 65, 65, 65, 65, 65, 65, 65, 65,
                        65, 65, 65, 65, 65, 65, 65, 73, 65, 65, 65, 65, 65, 65, 65, 65, 65, 65, 65,
                        65, 65, 93, 65, 65, 65, 65, 65, 65, 65, 75, 29, 65, 65, 65, 85, 85, 85, 85,
                        85, 133, 133, 1, 122, 0, 0, 93, 197, 1,
                    ])),
                },
            },
        ],
    )
}
