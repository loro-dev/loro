import typescript from '@rollup/plugin-typescript';
import { nodeResolve } from '@rollup/plugin-node-resolve';
import { wasm } from '@rollup/plugin-wasm';

const base64Config = {
    input: {
        'index': 'bundler/index.js',
    },
    output: {
        dir: 'base64',
        format: 'es',
        sourcemap: false,
        entryFileNames: '[name].js',
    },
    plugins: [
        typescript({
            tsconfig: 'tsconfig.json',
            compilerOptions: {
                target: 'ES2020',
                declaration: true,
                outDir: 'base64',
            },
            exclude: ['tests/**/*', 'vite.config.*']
        }),
        nodeResolve(),
        wasm({
            maxFileSize: 1024 * 1024 * 10,
            sync: ["*", "loro_wasm_bg.wasm", "bundler/loro_wasm_bg.wasm"]
        }),
    ]
};

export default [base64Config];
