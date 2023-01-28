# loro-ffi

- `cargo build --release`
- move `libloro.a` and `loro_ffi.h` to directory `examples/lib`
- run

## C++

Read more: [cbindgen](https://github.com/eqrion/cbindgen)

```bash
g++ loro.cpp -Bstatic -framework Security -L. -lloro -o loro
```

## Go

Read more: [cgo](https://pkg.go.dev/cmd/cgo)

```bash
go run main.go
```

## [Python](../loro-python/)

## Java

Candidates:

- [JNR](https://github.com/jnr/jnr-ffi)
- [Panama](https://jdk.java.net/panama/) [blog](https://jornvernee.github.io/java/panama/rust/panama-ffi/2021/09/03/rust-panama-helloworld.html)
- [JNI](https://github.com/jni-rs/jni-rs)

### Panama

install panama-jdk and jextract

```bash
jextract -I /Library/Developer/CommandLineTools/usr/include/c++/v1 -I /Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/usr/include -d loro_java -t org.loro -l loro -- lib/loro_ffi.h
```

### JNR

move `libloro.dylib` into `jnr/app`

```bash
gradle run
```
