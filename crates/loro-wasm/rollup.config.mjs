import fs from 'node:fs';
import path from 'node:path';
import typescript from '@rollup/plugin-typescript';
import { nodeResolve } from '@rollup/plugin-node-resolve';

const packageRoot = path.resolve();

// Rewrite source paths in the emitted sourcemap so they stay inside the
// published package. @rollup/plugin-typescript hands rollup a map whose
// `sources` are relative to TS's intended output location, but rollup
// resolves them against the .ts module's directory — yielding paths like
// "../../index.ts" relative to the published sourcemap that escape the
// package and trigger Vite/Vitest's "source file outside its package"
// warning when downstream users install loro-crdt. See #947.
const sourcemapPathTransform = (relativePath, sourcemapPath) => {
  const sourcemapDir = path.dirname(sourcemapPath);
  const absoluteSource = path.resolve(sourcemapDir, relativePath);
  if (absoluteSource.startsWith(packageRoot + path.sep) && fs.existsSync(absoluteSource)) {
    return relativePath.split(path.sep).join('/');
  }
  // The reported path resolves outside the package. Locate the real file
  // inside the package by basename and produce a path relative to the
  // sourcemap that stays inside the published tree.
  const fileName = path.basename(absoluteSource);
  const candidate = path.resolve(packageRoot, fileName);
  if (fs.existsSync(candidate)) {
    return path.relative(sourcemapDir, candidate).split(path.sep).join('/');
  }
  return relativePath;
};

const createConfig = (format, tsTarget, outputDir) => ({
  input: {
    'index': 'index.ts',
  },
  output: {
    dir: outputDir,
    format: format,
    sourcemap: true,
    sourcemapPathTransform,
    entryFileNames: '[name].js',
  },
  plugins: [
    typescript({
      tsconfig: 'tsconfig.json',
      compilerOptions: {
        target: tsTarget,
        declaration: true,
        outDir: outputDir,
        inlineSources: true,
      },
      exclude: ['tests/**/*', 'vite.config.*']
    }),
    nodeResolve()
  ],
  external: [/loro_wasm/]
});

// Create different bundle configurations
export default [
  // CommonJS for Node.js
  createConfig('cjs', 'ES2020', 'nodejs'),

  // ESM for Web
  createConfig('es', 'ES2020', 'web'),

  // ESM for browser bundlers that do not support top-level await.
  createConfig('es', 'ES2020', 'browser'),

  // ESM for bundler
  createConfig('es', 'ES2020', 'bundler'),
];
