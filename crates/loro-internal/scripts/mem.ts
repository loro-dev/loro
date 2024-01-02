import * as path from "https://deno.land/std@0.105.0/path/mod.ts";
const __dirname = path.dirname(path.fromFileUrl(import.meta.url));
import { resolve } from "https://deno.land/std@0.105.0/path/mod.ts";

export const Tasks = [
  "100_concurrent",
  "200_concurrent",
  "automerge",
  "10_actor_sync_1000_actions",
  "20_actor_sync_1000_actions",
  "10_actor_sync_2000_actions",
];

export interface Result {
  task: string;
  maxBytes: number;
  endBytes: number;
}

// run `cargo run --example mem -r -- ${task}`
export async function run(task: string): Promise<Result> {
  const cmd = `cargo run --example mem -r -- ${task}`;
  const process = Deno.run({
    cmd: cmd.split(" "),
    cwd: resolve(__dirname, ".."),
    stdout: "piped",
    stderr: "piped",
  });

  const output = new TextDecoder().decode(await process.stderrOutput());
  try {
    // extract "2,555,555" from `dhat: At t-gmax: 2,555,555 bytes`
    const maxBytes = parseInt(
      output.match(/dhat: At t-gmax:\s+((\d+,?)+) bytes/)![1].replace(/,/g, ""),
    );
    const endBytes = parseInt(
      output.match(/dhat: At t-end:\s+((\d+,?)+) bytes/)![1].replace(/,/g, ""),
    );
    return {
      task,
      maxBytes,
      endBytes,
    };
  } catch (e) {
    console.error(e);
    console.log(output);
    throw e;
  }
}
