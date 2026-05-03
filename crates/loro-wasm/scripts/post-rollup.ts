import { walk } from "https://deno.land/std@0.224.0/fs/mod.ts";

const DIRS_TO_SCAN = ["./nodejs", "./bundler", "./browser", "./web"];
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
    let nextContent = injectBase64WasmBranch(content, base64IndexPath);
    nextContent = simplifyBase64WasmInitialization(
        nextContent,
        base64IndexPath,
    );
    nextContent = patchBase64NodeRequires(nextContent, base64IndexPath);
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

function simplifyBase64WasmInitialization(
    content: string,
    filePath: string,
): string {
    const startMarker =
        "// Normalize how bundlers expose the wasm module/exports.";
    const endMarker = `\n\n/**
 * @deprecated Please use LoroDoc
 */`;
    const start = content.indexOf(startMarker);
    const end = start === -1 ? -1 : content.indexOf(endMarker, start);
    if (start === -1 || end === -1) {
        throw new Error(
            `Could not locate wasm initialization block while patching ${filePath}`,
        );
    }

    const replacement = `// Instantiate the inlined base64 wasm synchronously.
const wasmModuleOrInstance = rawWasm.default({
  "./loro_wasm_bg.js": imports,
});
const wasmInstance =
  wasmModuleOrInstance instanceof WebAssembly.Instance
    ? wasmModuleOrInstance
    : new WebAssembly.Instance(wasmModuleOrInstance, {
      "./loro_wasm_bg.js": imports,
    });
__wbg_set_wasm(wasmInstance.exports ?? wasmInstance);
if (typeof imports.__wbindgen_start === "function") {
  imports.__wbindgen_start();
}`;

    return content.slice(0, start) + replacement + content.slice(end);
}

function patchBase64NodeRequires(content: string, filePath: string): string {
    const directRequires = `var fs = require("fs");
var path = require("path");`;
    const indirectRequires = `var nodeRequire = typeof require === "function" ? require : null;
var fs = nodeRequire && nodeRequire("fs");
var path = nodeRequire && nodeRequire("path");`;
    const browserSafeRequires = `var fs = null;
var path = null;`;

    if (content.includes(browserSafeRequires)) {
        return content;
    }

    if (content.includes(directRequires)) {
        return content.replace(directRequires, browserSafeRequires);
    }

    if (content.includes(indirectRequires)) {
        return content.replace(indirectRequires, browserSafeRequires);
    }

    throw new Error(
        `Could not locate Node require block while patching ${filePath}`,
    );
}

async function main() {
    for (const dir of DIRS_TO_SCAN) {
        await transform(dir);
    }

    await rollupBase64();
    await transform("./base64");
}

if (import.meta.main) {
    main();
}
