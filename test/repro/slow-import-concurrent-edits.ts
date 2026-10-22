import assert from 'assert'
import { parseArgs } from 'util'
import { LoroDoc } from 'loro-crdt'

const { values } = parseArgs({
  options: {
    historical: { type: 'string', default: '5000' },
    concurrent: { type: 'string', default: '4' },
  },
  allowPositionals: false,
  strict: true,
})

const historical = Number(values.historical)
const concurrent = Number(values.concurrent)
if (!Number.isInteger(historical)) throw new Error('--historical must be an integer')
if (!Number.isInteger(concurrent)) throw new Error('--concurrent must be an integer')

const base = new LoroDoc()

for (let i = 1; i < historical; i++) {
  base.setPeerId(BigInt(i))
  base.getMap('x').set('x', i)
  base.commit()
}

const snapshot = base.export({ mode: 'snapshot' })
console.log('snapshot size bytes:', snapshot.byteLength)
console.log('shallow size bytes:', base.export({ mode: 'shallow-snapshot', frontiers: base.frontiers() }).byteLength)
console.log('doc historical peers:', historical)
console.log('testing importing concurrent updates:', concurrent)

const testType = (mutate: (doc: LoroDoc) => void) => {
  const concurrentUpdates = Array.from({ length: concurrent }, (_, i) => {
    let update: Uint8Array | undefined
    const branch = new LoroDoc()
    branch.import(snapshot)
    branch.setPeerId(BigInt(historical + i))
    branch.subscribeLocalUpdates(u => (update = u))
    mutate(branch)
    branch.commit()
    assert.notEqual(update, undefined)
    return update!
  })

  const doc1 = new LoroDoc()
  doc1.import(snapshot)
  return Object.fromEntries(
    concurrentUpdates.map((update, i) => {
      const start = performance.now()
      doc1.import(update)
      return [`import concurrent update ${i + 1}`, (performance.now() - start).toFixed(2)]
    })
  )
}

console.table({
  LoroText: testType(d => d.getText('text').insert(0, d.peerIdStr)),
  LoroMap: testType(d => d.getMap('map').set('key', d.peerIdStr)),
  LoroTree: testType(d => d.getTree('tree').createNode()),
  LoroCounter: testType(d => d.getCounter('counter').increment(1)),
  LoroList: testType(d => d.getList('list').insert(0, d.peerIdStr)),
  LoroMovableList: testType(d => d.getMovableList('movableList').insert(0, d.peerIdStr)),
})
