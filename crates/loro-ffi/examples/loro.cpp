#include <stdio.h>
extern "C" {
    #include "../target/loro_ffi.h"
};

int main(void) {
  LoroDoc* loro = loro_new();
  TextHandler* text = loro_get_text(loro, "text");
  text_insert(text, loro, 0, "abc");
  char* str = text_value(text);
  printf("%s", str);
  text_free(text);
  loro_free(loro);
}
