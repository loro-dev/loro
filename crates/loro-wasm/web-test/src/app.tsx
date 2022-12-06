import { useState, useEffect } from "preact/hooks";
import preactLogo from "./assets/preact.svg";
import init, { Loro } from "loro-wasm";
import "./app.css";

init();
export function App() {
  const [count, setCount] = useState(0);
  useEffect(() => {
    (async () => {
      await init();
      const loro = new Loro();
      const a = loro.getText("ha");
      a.insert(loro, 0, "hello world");
      a.delete(loro, 6, 5);
      a.insert(loro, 6, "everyone");
      console.log(a.value);
      const b = loro.getMap("ha");
      b.set(loro, "ab", 123);
      console.log(b.value);
      console.log(a.value);
    })();
  }, []);

  return (
    <>
      <div>
        <a href="https://vitejs.dev" target="_blank">
          <img src="/vite.svg" class="logo" alt="Vite logo" />
        </a>
        <a href="https://preactjs.com" target="_blank">
          <img src={preactLogo} class="logo preact" alt="Preact logo" />
        </a>
      </div>
      <h1>Vite + Preact</h1>
      <div class="card">
        <button onClick={() => setCount((count) => count + 1)}>
          count is {count}
        </button>
        <p>
          Edit <code>src/app.tsx</code> and save to test HMR
        </p>
      </div>
      <p class="read-the-docs">
        Click on the Vite and Preact logos to learn more
      </p>
    </>
  );
}
