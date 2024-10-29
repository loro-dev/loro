import typescript from '@rollup/plugin-typescript';
import { nodeResolve } from '@rollup/plugin-node-resolve';
const createConfig = (format, tsTarget, outputDir) => ({
  input: {
    'index': 'index.ts',
  },
  output: {
    dir: outputDir,
    format: format,
    sourcemap: true,
    entryFileNames: '[name].js',
  },
  plugins: [
    typescript({
      tsconfig: 'tsconfig.json',
      compilerOptions: {
        target: tsTarget,
        declaration: true,
        outDir: outputDir,
      },
      exclude: ['tests/**/*']
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

  // ESM for bundler
  createConfig('es', 'ES2020', 'bundler'),
];
