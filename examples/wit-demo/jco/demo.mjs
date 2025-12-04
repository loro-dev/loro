/**
 * Loro CRDT WebAssembly Component Demo with jco
 *
 * This demo shows how to use the Loro CRDT library compiled as a
 * WebAssembly Component, transpiled to JavaScript using jco.
 */

// Import the transpiled component
import { loroDoc, loroText, loroMap, loroList } from './gen/loro.js';

console.log('=== Loro CRDT WebAssembly Component Demo ===\n');

// Create a new document
console.log('1. Creating a new Loro document...');
const doc = new loroDoc.Doc();
console.log(`   Document created with peer ID: ${doc.peerId()}`);
console.log(`   Is empty: ${doc.isEmpty()}\n`);

// Working with Text
console.log('2. Working with Text container...');
const textHandle = doc.getText('my-text');
loroText.insert(doc, textHandle, 0, 'Hello, ');
loroText.insert(doc, textHandle, 7, 'World!');
console.log(`   Text content: "${loroText.toString(doc, textHandle)}"`);
console.log(`   Length (UTF-8): ${loroText.lenUtf8(doc, textHandle)}`);
console.log(`   Length (Unicode): ${loroText.lenUnicode(doc, textHandle)}\n`);

// Working with Map
console.log('3. Working with Map container...');
const mapHandle = doc.getMap('my-map');
loroMap.insertString(doc, mapHandle, 'name', 'Loro');
loroMap.insertNumber(doc, mapHandle, 'version', 1.10);
loroMap.insertBool(doc, mapHandle, 'isAwesome', true);
console.log(`   Keys: ${JSON.stringify(loroMap.keys(doc, mapHandle))}`);
console.log(`   Name: ${loroMap.get(doc, mapHandle, 'name')}`);
console.log(`   Version: ${loroMap.get(doc, mapHandle, 'version')}`);
console.log(`   Map length: ${loroMap.len(doc, mapHandle)}\n`);

// Working with List
console.log('4. Working with List container...');
const listHandle = doc.getList('my-list');
loroList.pushString(doc, listHandle, 'first');
loroList.pushNumber(doc, listHandle, 42);
loroList.insertString(doc, listHandle, 0, 'zeroth');
console.log(`   List length: ${loroList.len(doc, listHandle)}`);
console.log(`   Item 0: ${loroList.get(doc, listHandle, 0)}`);
console.log(`   Item 1: ${loroList.get(doc, listHandle, 1)}`);
console.log(`   Item 2: ${loroList.get(doc, listHandle, 2)}\n`);

// Commit changes
console.log('5. Committing changes...');
doc.commit('Initial data setup');
console.log('   Changes committed!\n');

// Export and show document state
console.log('6. Document state:');
const jsonState = doc.toJson();
console.log(`   JSON: ${jsonState}\n`);

// Export updates
console.log('7. Exporting updates...');
const updates = doc.exportUpdates();
console.log(`   Updates size: ${updates.length} bytes\n`);

// Create a new document and import updates
console.log('8. Syncing with another document...');
const doc2 = new loroDoc.Doc();
console.log(`   Doc2 peer ID: ${doc2.peerId()}`);
doc2.importUpdates(updates);
console.log(`   Doc2 after import: ${doc2.toJson()}\n`);

// Fork the document
console.log('9. Forking document...');
const forked = doc.fork();
console.log(`   Forked doc peer ID: ${forked.peerId()}`);
const forkedText = forked.getText('my-text');
loroText.insert(forked, forkedText, 0, '[Forked] ');
forked.commit('Forked edit');
console.log(`   Forked text: "${loroText.toString(forked, forkedText)}"\n`);

console.log('=== Demo Complete ===');
