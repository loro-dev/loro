<script setup lang="ts">
  import { onMounted, onUnmounted, reactive, ref, watch } from "vue";
  import Quill from "quill";
  import "quill/dist/quill.core.css";
  import "quill/dist/quill.bubble.css";
  import "quill/dist/quill.snow.css";
  import { QuillBinding } from "./binding";
  import { Loro, setPanicHook, convertVersionToReadableMap } from "loro-crdt";

  setPanicHook();

  const editor1 = ref<null | HTMLDivElement>(null);
  const editor2 = ref<null | HTMLDivElement>(null);
  const editor3 = ref<null | HTMLDivElement>(null);
  const editor4 = ref<null | HTMLDivElement>(null);
  const binds: QuillBinding[] = [];
  const texts: Loro[] = [];
  const editors = [editor1, editor2, editor3, editor4];
  const editorVersions = reactive(["", "", "", ""]);
  const online = reactive([true, true, true, true]);
  onMounted(() => {
    let index = 0;
    for (const editor of editors) {
      const text = new Loro();
      text.setPeerId(BigInt(index));
      texts.push(text);
      const quill = new Quill(editor.value!, {
        modules: {
          toolbar: [
            [
              {
                header: [1, 2, 3, 4, false],
              },
            ],
            ["bold", "italic", "underline", "link"],
          ],
        },
        //theme: 'bubble',
        theme: "snow",
        formats: ["bold", "underline", "header", "italic", "link"],
        placeholder: "Type something in here!",
      });
      binds.push(new QuillBinding(text, quill));
      const this_index = index;

      const sync = () => {
        if (!online[this_index]) {
          return;
        }

        for (let i = 0; i < texts.length; i++) {
          if (i === this_index || !online[i]) {
            continue;
          }

          texts[i].import(text.exportFrom(texts[i].version()));
          text.import(texts[i].exportFrom(text.version()));
        }
      };

      text.subscribe((e) => {
        if (e.local) {
          Promise.resolve().then(sync);
        }
        Promise.resolve().then(() => {
          const version = text.version();
          const map = convertVersionToReadableMap(version);
          const versionStr = JSON.stringify(map, null, 2);
          editorVersions[this_index] = versionStr;
        });
      });

      watch(
        () => online[this_index],
        (isOnline) => {
          if (isOnline) {
            sync();
          }
        }
      );

      index += 1;
    }
  });

  onUnmounted(() => {
    binds.forEach((x) => x.destroy());
  });
</script>

<template>
  <h2>
    <a href="https://github.com/loro-dev/crdt-richtext">
      <img src="./assets/Loro.svg" alt="Loro Logo" class="logo" />
      Loro crdt-richtext
    </a>
  </h2>

  <div class="parent">
    <div class="editor">
      <button
        @click="
          () => {
            online[0] = !online[0];
          }
        "
      >
        Editor 0 online: {{ online[0] }}
      </button>
      <div class="version">version: {{ editorVersions[0] }}</div>
      <div ref="editor1" />
    </div>
    <div class="editor">
      <button
        @click="
          () => {
            online[1] = !online[1];
          }
        "
      >
        Editor 1 online: {{ online[1] }}
      </button>
      <div class="version">version: {{ editorVersions[1] }}</div>
      <div ref="editor2" />
    </div>
    <div class="editor">
      <button
        @click="
          () => {
            online[2] = !online[2];
          }
        "
      >
        Editor 2 online: {{ online[2] }}
      </button>
      <div class="version">version: {{ editorVersions[2] }}</div>
      <div ref="editor3" />
    </div>
    <div class="editor">
      <button
        @click="
          () => {
            online[3] = !online[3];
          }
        "
      >
        Editor 3 online: {{ online[3] }}
      </button>
      <div class="version">version: {{ editorVersions[3] }}</div>
      <div ref="editor4" />
    </div>
  </div>
</template>

<style scoped>
  a {
    color: black;
    font-weight: 900;
  }

  h2 {
    font-weight: 900;
  }

  .logo {
    width: 2em;
    margin-right: 0.5em;
    vertical-align: -0.5em;
  }

  .editor {
    width: 400px;
    display: flex;
    flex-direction: column;
    min-height: 200px;
  }

  button {
    color: #565656;
    padding: 0.3em 0.6em;
    margin-bottom: 0.4em;
    background-color: #eee;
  }

  /**matrix 2x2 */
  .parent {
    display: grid;
    grid-template-columns: 1fr 1fr;
    grid-template-rows: 1fr 1fr;
    gap: 2em 1em;
  }

  .version {
    color: grey;
    font-size: 0.8em;
  }
</style>
