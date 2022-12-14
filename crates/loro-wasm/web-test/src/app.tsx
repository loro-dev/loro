import { useCallback, useState } from "preact/hooks";
import { Loro, setPanicHook, enableDebug } from "loro-wasm/bundler/loro_wasm";
import "./app.css";

// enableDebug();
async function testListInsert() {
  const loro = new Loro();
  const loroB = new Loro();
  const a = loro.getList("list");
  loro.subscribe((local: boolean) => {
    if (local) {
      loroB.importUpdates(loro.exportUpdates(loroB.version()));
    }
  })

  for (let i = 0; i < 1000; i++) {
    a.insert(loro, i, i);
  }
}

function testManyActors() {
  const actors = new Array(200).fill(0).map(() => new Loro());
  console.log(actors);
  for (let i = 0; i < actors.length; i++) {
    const list = actors[i].getList("list");
    list.insert(actors[i], 0, i);
  }

  for (let i = 1; i < actors.length; i++) {
    actors[0].importUpdates(actors[i].exportUpdates(undefined));
  }

  for (let i = 1; i < actors.length; i++) {
    actors[i].importUpdates(actors[0].exportUpdates(undefined));
  }

  console.log(actors[0].getList("list").value);
  for (let i = 0; i < actors.length - 1; i++) {
    const listA = actors[i].getList("list").value;
    const listB = actors[i + 1].getList("list").value;

    if (listA.length != listB.length) {
      console.log(listA.value);
      console.log(listB.value);
      throw new Error("not eq");
    }

    for (let j = 0; j < listA.length; j++) {
      if (listA[j] != listB[j]) {
        console.log(listA.value);
        console.log(listB.value);
        throw new Error("not eq");
      }
    }
  }
}

export function App() {
  return (
    <>
      <Bench fn={testListInsert} name="test list insert"/>
      <Bench fn={testManyActors} name="test many actors"/>
    </>
  );
}

function Bench({ fn, name }: { fn: () => void, name: string }) {
  const [duration, setDuration] = useState(0)
  const onClick = useCallback(() => {
    setDuration(0);
    const start = performance.now()
    fn()
    const end = performance.now()
    setDuration(end - start)
  }, [fn])
  return (
    <p>
      <button onClick={onClick}>{name}</button>
      {
        duration != 0 ? (
          <span style={{marginLeft: 8}}>{duration.toFixed(0)} ms</span>
        ) : undefined
      }
    </p>
  )
}
