const assert = require('node:assert/strict')
const fs = require('node:fs')
const [mode, packagePath, file] = process.argv.slice(2)
const { LoroDoc } = require(packagePath)
const peers = 30_000

if (mode === 'prepare') {
  const updates = []
  for (let i = 0; i < peers; i++) {
    const peer = new LoroDoc()
    peer.setPeerId(i + 1)
    const map = peer.getMap('map')
    map.set('v', i)
    updates.push(peer.export({ mode: 'update' }))
    map.free()
    peer.free()
  }
  const doc = new LoroDoc()
  doc.importBatch(updates)
  assert.equal(doc.frontiers().length, peers)
  fs.writeFileSync(file, doc.export({ mode: 'snapshot' }))
  doc.free()
} else {
  const snapshot = fs.readFileSync(file)
  const doc = new LoroDoc()
  global.gc()
  const start = performance.now()
  const { success, pending } = doc.import(snapshot)
  const ms = performance.now() - start
  assert.equal(success.size, peers)
  assert.equal(pending, null)
  assert.equal(doc.frontiers().length, peers)
  assert.equal(doc.getMap('map').get('v'), peers - 1)
  console.log(JSON.stringify({ ms, snapshotBytes: snapshot.length }))
  doc.free()
}
