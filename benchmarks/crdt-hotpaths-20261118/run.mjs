import { writeFileSync } from 'node:fs'
import { runBenchmarks } from './js-lib/index.js'
import { benchmarkResults } from './js-lib/utils.js'

const [library, suite, output] = process.argv.slice(2)
const factory = library === 'loro'
  ? new (await import('./benchmarks/loro/factory.js')).LoroFactory()
  : new (await import('./benchmarks/yjs/factory.js')).YjsFactory()

const filters = {
  full: /^\[([BC][1-4](\.[0-9]+)?)\]/,
  b4: /^\[B4\]/,
  b4x100: /^\[B4x100\]/
}
let peakHeapUsedBytes = process.memoryUsage().heapUsed
let samples = 1
if (suite === 'b4x100') {
  const create = factory.create.bind(factory)
  factory.create = handler => {
    const doc = create(handler)
    let completed = 0
    for (const method of ['insertText', 'deleteText']) {
      const edit = doc[method].bind(doc)
      doc[method] = (pos, value) => {
        edit(pos, value)
        if (++completed % 8192 === 0) {
          peakHeapUsedBytes = Math.max(peakHeapUsedBytes, process.memoryUsage().heapUsed)
          samples++
        }
      }
    }
    return doc
  }
}
const start = performance.now()
await runBenchmarks(factory, name => filters[suite].test(name))
writeFileSync(output, JSON.stringify({
  node: process.version,
  totalWallMs: performance.now() - start,
  maxRssKiB: process.resourceUsage().maxRSS,
  peakHeapUsedBytes: Math.max(peakHeapUsedBytes, process.memoryUsage().heapUsed),
  heapSamples: samples + 1,
  results: benchmarkResults
}, null, 2))
