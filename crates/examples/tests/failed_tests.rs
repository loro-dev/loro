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
                peer: 44971974514245632,
                action: InsertText {
                    index: 228,
                    s: "0\0\0".into(),
                },
            },
            SyncAll,
            Action {
                peer: 23939170762752,
                action: InsertText {
                    index: 404,
                    s: "C\u{b}0\0\u{15555}".into(),
                },
            },
            Sync {
                from: 10778685752873424277,
                to: 52870070483605,
                kind: Pending,
            },
            Action {
                peer: 6128427715264512,
                action: InsertMap {
                    key: "".into(),
                    value: "".into(),
                },
            },
            Action {
                peer: 10778685752873447424,
                action: DeleteList { index: 368 },
            },
            Sync {
                from: 10778685752873440661,
                to: 10778685752873424277,
                kind: Pending,
            },
            Sync {
                from: 10778685752873424277,
                to: 18395315059780064661,
                kind: Pending,
            },
            SyncAll,
            SyncAll,
            Sync {
                from: 445944668984725,
                to: 256,
                kind: Snapshot,
            },
            Action {
                peer: 562699868423424,
                action: InsertText {
                    index: 228,
                    s: "\0\0".into(),
                },
            },
            SyncAll,
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
