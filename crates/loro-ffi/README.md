# loro-ffi

- `cargo build --release`
- move `libloro.a` to directory `examples/cpp`
- run

```bash
g++ loro.cpp -Bstatic -framework Security -L. -lloro -o loro
```
