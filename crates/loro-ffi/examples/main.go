package main;



/*
#cgo LDFLAGS: -L./lib -framework Security -lloro
#include "./lib/loro_ffi.h"
*/
import "C"
import "fmt"

func main() {
	loro := C.loro_new();
	text := C.loro_get_text(loro, C.CString("text"));
	// pos := C.uint(0);
	C.text_insert(text, loro, 0, C.CString("abc"));
	value := C.text_value(text);
	fmt.Println(C.GoString(value));
	C.text_free(text);
	C.loro_free(loro);
}