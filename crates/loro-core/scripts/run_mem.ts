import { run, Tasks } from "./mem.ts";

const output = [];
for (const task of Tasks) {
  const result = await run(task);
  output.push(result);
}

console.log(output);
