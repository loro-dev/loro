# Performance And Research

## Performance Interpretation

- Treat benchmarks as tradeoff indicators, not universal rankings.
- Compare:
  - encode/decode cost
  - parse time
  - document/update size
  - memory footprint
  - conflict-heavy vs low-conflict workloads

## Main Performance Topics

- Benchmark framing and methodology.
- Encoded document size comparisons and shallow snapshot savings.
- Native Rust benchmark context.

## Main Concept Topic

- Event-graph replay explains why Loro can keep local operations simple while still merging remote history efficiently.

## Stored Knowledge Threads

- Stable encoding and import/export speedups.
- Movable tree algorithm, unsafe moves, and fractional index tradeoffs.
- Rich text design, style anchors, overlap, and expansion behavior.
- Peritext/Fugue background for rich text intent preservation.
- Mirror motivation, complexity model, and state-to-CRDT mapping.
- Local-first product vision and project history.

## Practical Rule

- Reach for these topics when the user asks “why does Loro work this way?” or “what tradeoff is Loro making?”.
- Do not dump benchmark tables into product recommendations without first matching them to the workload.
