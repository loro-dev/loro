import { createServer } from 'http';
import fs from 'fs';
import path, { dirname, extname, join, resolve } from 'path';
import { fileURLToPath } from 'url';
import { build } from 'esbuild';

const { readFileSync, existsSync, statSync } = fs;
const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const PORT = 8081;

await build({
  entryPoints: [join(__dirname, 'app.js')],
  bundle: true,
  treeShaking: true,
  outfile: join(__dirname, 'dist', 'app.js'),
  format: 'esm',
  sourcemap: 'inline',
  target: 'chrome122',
  logLevel: 'info',
  banner: {
    js: 'var global = window, globalThis = window;',
  },
  publicPath: '/dist/',
  loader: {
    '.wasm': 'binary',
  },
});

const server = createServer((req, res) => {
  let filePath = resolve(__dirname, '.' + req.url);
  if (req.url === '/' || req.url === '') {
    filePath = resolve(__dirname, 'index.html');
  }

  if (existsSync(filePath) && statSync(filePath).isFile()) {
    const extension = extname(filePath).toLowerCase();
    const mimeTypes = {
      '.html': 'text/html',
      '.js': 'application/javascript',
      '.css': 'text/css',
      '.json': 'application/json',
      '.png': 'image/png',
      '.jpg': 'image/jpg',
      '.gif': 'image/gif',
      '.svg': 'image/svg+xml',
      '.wav': 'audio/wav',
      '.mp4': 'video/mp4',
      '.woff': 'application/font-woff',
      '.ttf': 'application/font-ttf',
      '.eot': 'application/vnd.ms-fontobject',
      '.otf': 'application/font-otf',
      '.wasm': 'application/wasm',
    };
    const contentType = mimeTypes[extension] || 'application/octet-stream';
    res.writeHead(200, { 'Content-Type': contentType });
    res.end(readFileSync(filePath));
  } else {
    res.writeHead(404, { 'Content-Type': 'text/plain' });
    res.end('404 Not Found');
  }
});

server.listen(PORT, () => {
  console.log(`Server running at http://localhost:${PORT}/`);
});

process.on('SIGINT', () => {
  server.close();
  process.exit(0);
});
