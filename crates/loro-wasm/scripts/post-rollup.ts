import { walk } from "https://deno.land/std@0.224.0/fs/mod.ts";

const DIRS_TO_SCAN = ["./nodejs", "./bundler", "./web"];
const FILES_TO_PROCESS = ["index.js", "index.d.ts"];

async function replaceInFile(filePath: string) {
    try {
        let content = await Deno.readTextFile(filePath);

        // Replace various import/require patterns for 'loro-wasm'
        const isWebIndexJs = filePath.includes("web") &&
            filePath.endsWith("index.js");
        const target = isWebIndexJs ? "./loro_wasm.js" : "./loro_wasm";

        content = content.replace(
            /from ["']loro-wasm["']/g,
            `from "${target}"`,
        );
        content = content.replace(
            /require\(["']loro-wasm["']\)/g,
            `require("${target}")`,
        );
        content = content.replace(
            /import\(["']loro-wasm["']\)/g,
            `import("${target}")`,
        );

        if (isWebIndexJs) {
            content = `export { default } from "./loro_wasm.js";\n${content}`;
        }

        await Deno.writeTextFile(filePath, content);
        console.log(`✓ Processed: ${filePath}`);
    } catch (error) {
        console.error(`Error processing ${filePath}:`, error);
    }
}

async function transform(dir: string) {
    try {
        for await (
            const entry of walk(dir, {
                includeDirs: false,
                match: [/index\.(js|d\.ts)$/],
            })
        ) {
            if (FILES_TO_PROCESS.includes(entry.name)) {
                await replaceInFile(entry.path);
            }
        }
    } catch (error) {
        console.error(`Error scanning directory ${dir}:`, error);
    }
}

async function rollupBase64() {
    const command = new Deno.Command("rollup", {
        args: ["--config", "./rollup.base64.config.mjs"],
    });

    try {
        const { code, stdout, stderr } = await command.output();
        if (code === 0) {
            console.log("✓ Rollup base64 build completed successfully");
        } else {
            console.error("Error running rollup base64 build:");
            console.error(new TextDecoder().decode(stdout));
            console.error(new TextDecoder().decode(stderr));
        }
    } catch (error) {
        console.error("Failed to execute rollup command:", error);
    }

    const base64IndexPath = "./base64/index.js";
    const content = await Deno.readTextFile(base64IndexPath);
    const nextContent = injectBase64WasmBranch(content, base64IndexPath);
    await Deno.writeTextFile(base64IndexPath, nextContent);

    await Deno.copyFile("./bundler/loro_wasm.d.ts", "./base64/loro_wasm.d.ts");
}

function injectBase64WasmBranch(content: string, filePath: string): string {
    const alreadyPatched =
        content.includes("typeof wasmModuleOrExports === \"function\"");
    if (alreadyPatched) {
        return content;
    }

    const bunBranchPattern = /}\s*else if\s*\(\s*(['"])Bun\1\s+in\s+globalThis\s*\)\s*\{/;
    if (!bunBranchPattern.test(content)) {
        throw new Error(
            `Could not locate Bun branch while patching ${filePath}`,
        );
    }

    const base64Branch = `} else if (typeof wasmModuleOrExports === "function") {
  const moduleOrInstance = wasmModuleOrExports({
    "./loro_wasm_bg.js": imports,
  });
  const instance =
    moduleOrInstance instanceof WebAssembly.Instance
      ? moduleOrInstance
      : new WebAssembly.Instance(moduleOrInstance, {
        "./loro_wasm_bg.js": imports,
      });
  finalize(instance.exports ?? instance);
} else if ("Bun" in globalThis) {`;

    return content.replace(bunBranchPattern, base64Branch);
}

async function main() {
    for (const dir of DIRS_TO_SCAN) {
        await transform(dir);
    }

    await rollupBase64();
    transform("./base64");
}

if (import.meta.main) {
    main();
}
