console.log('dafuq');

import { LoroMap } from 'loro-crdt';
window.LoroMap = LoroMap;

const val = new LoroMap().get('k');
console.log('bug test:', val);
document.getElementById('app').textContent = String(val);
