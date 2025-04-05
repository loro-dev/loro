# Contributing Guide

### Development Environment Setup

1. **Rust**: Install from the official Rust website.
2. **Deno**: Download and install from Deno's website.
3. **Node**: Install from the Node.js website.
4. **pnpm**: Run `npm i -g pnpm` for global installation.
5. **Rust Target**: Add with `rustup target add wasm32-unknown-unknown`.
6. **wasm-bindgen-cli**: Install version 0.2.100 via `cargo install wasm-bindgen-cli --version 0.2.100`.
6. **wasm-opt**: Install using `cargo install wasm-opt --locked`.
7. **wasm-snip**: Install using `cargo install wasm-snip`.
8. **cargo-nextest**: Install using `cargo install cargo-nextest --locked`.
9. **cargo-fuzz**: Run `cargo install cargo-fuzz`.
10. **cargo-llvm-cov**(to generate coverage report): Run `cargo install cargo-llvm-cov` 

### Test

```bash
deno task test

# Build and test WASM
deno task test-wasm
```
