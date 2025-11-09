import { useMemo, useState } from "react";
import { LoroDoc } from "loro-crdt";

const containerId = "welcome";

function useLoroMessage() {
  const doc = useMemo(() => {
    const instance = new LoroDoc();
    const text = instance.getText(containerId);
    if (text.length === 0) {
      text.insert(0, "Hello from LoroDoc running inside Rsbuild!");
    }
    return instance;
  }, []);

  const [message, setMessage] = useState(() => doc.getText(containerId).toString());

  const appendGreeting = () => {
    const text = doc.getText(containerId);
    text.insert(text.length, " 👋");
    setMessage(text.toString());
  };

  return { message, appendGreeting };
}

export default function App() {
  const { message, appendGreeting } = useLoroMessage();

  return (
    <main>
      <h1>Rsbuild + Loro</h1>
      <p>
        <strong>Shared doc:</strong> {message}
      </p>
      <button type="button" onClick={appendGreeting}>
        Append greeting
      </button>
    </main>
  );
}
