const { performance } = require("node:perf_hooks");
const { TextDecoder } = require("node:util");
const { LoroDoc } = require("../nodejs/index.js");

const DISTINCT_COUNT = 20_000;
const REPEATED_READS = 1_000_000;
const WARMUP_ROUNDS = 3;
const ROUNDS = 7;
let blackhole = 0;

function makeContainers(count = DISTINCT_COUNT) {
  const doc = new LoroDoc();
  const getters = [
    (name) => doc.getMap(name),
    (name) => doc.getList(name),
    (name) => doc.getText(name),
    (name) => doc.getTree(name),
    (name) => doc.getMovableList(name),
    (name) => doc.getCounter(name),
  ];
  const containers = Array.from({ length: count }, (_, index) =>
    getters[index % getters.length](`container-${index}`),
  );
  return { doc, containers };
}

function median(values) {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.floor(sorted.length / 2)];
}

function timed(name, setup, run) {
  const samples = [];
  for (let round = 0; round < WARMUP_ROUNDS + ROUNDS; round++) {
    global.gc?.();
    const state = setup();
    const start = performance.now();
    run(state);
    const elapsed = performance.now() - start;
    if (round >= WARMUP_ROUNDS) samples.push(elapsed);
    state.doc?.free();
  }
  return { name, medianMs: median(samples), samplesMs: samples };
}

function countDecodes(setup, run) {
  const state = setup();
  const original = TextDecoder.prototype.decode;
  let decodes = 0;
  TextDecoder.prototype.decode = function (...args) {
    decodes++;
    return original.apply(this, args);
  };
  try {
    run(state);
  } finally {
    TextDecoder.prototype.decode = original;
    state.doc?.free();
  }
  return decodes;
}

const repeatedSetup = () => {
  const doc = new LoroDoc();
  return { doc, container: doc.getText("repeated-container-id") };
};
const repeatedRun = ({ container }) => {
  for (let i = 0; i < REPEATED_READS; i++) blackhole += container.id.length;
};

const firstRun = ({ containers }) => {
  for (const container of containers) blackhole += container.id.length;
};

const mirrorReuseRun = ({ containers }) => {
  for (const container of containers) {
    blackhole += container.id.length;
    blackhole += container.id.length;
    blackhole += container.id.length;
    blackhole += container.id.length;
    const json = container.toJSON();
    if (json != null) blackhole++;
  }
};

const churnSetup = () => {
  const state = makeContainers();
  state.ids = state.containers.map((container) => container.id);
  return state;
};
const churnRun = ({ doc, ids }) => {
  for (const id of ids) {
    const container = doc.getContainerById(id);
    blackhole += container.id.length;
    const json = container.toJSON();
    if (json != null) blackhole++;
  }
};

const cases = [
  ["same-wrapper-repeated-id", repeatedSetup, repeatedRun],
  ["distinct-wrappers-first-id", makeContainers, firstRun],
  ["mirror-reuse-wrapper", makeContainers, mirrorReuseRun],
  ["mirror-new-wrapper-per-id", churnSetup, churnRun],
];

const result = cases.map(([name, setup, run]) => ({
  ...timed(name, setup, run),
  decoderCalls: countDecodes(setup, run),
}));

console.log(
  JSON.stringify(
    {
      distinctCount: DISTINCT_COUNT,
      repeatedReads: REPEATED_READS,
      warmupRounds: WARMUP_ROUNDS,
      rounds: ROUNDS,
      blackhole,
      result,
    },
    null,
    2,
  ),
);
