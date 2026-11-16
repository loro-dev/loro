# Loro 1.18.0 release acceptance

Baseline: 98465472165d086ea7649df4facc17637a73fb72. Only release fixes are added.

| Check | Acceptance | Result |
| --- | --- | --- |
| B-tree | 0.10.8 source matches in-tree fork; tests; published archive | Published; 26 unit and 5 integration tests passed; generic-btree PR #7 |
| Rust package | registry-only consumer; text chunk and malformed mark regressions | 8 malformed-input and 44 Delta tests passed; registry-only package check running |
| Rust CI | final release commit existing platform CI | Pending |
| npm | 1.18.0 core/map, loro.js 0.4.1, fractional-index 0.2.1; generated entrypoints and consumers | Pending |
| FFI | 1.18.0 core dependency, Rust and generated binding tests, registry archive | Pending |
| PyPI | 1.18.0, existing full wheel matrix and sdist; install smoke | Pending |
| Swift | 1.18.0; all existing XCFramework slices; remote SwiftPM checksum and tests | Pending |
| Recovery | retry only failed surfaces; never overwrite versions or move release tags | Pending |

Publish order: generic-btree, Rust core, npm, FFI/Python, Swift.
Track immutable versions, source commits, workflow runs and online artifact checks in the final report. Native build evidence is separate from device execution.
