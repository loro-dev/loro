import { walk } from "https://deno.land/std/fs/mod.ts";

const DIRS_TO_SCAN = ["./nodejs", "./bundler", "./web"];
const FILES_TO_PROCESS = ["index.js", "index.d.ts"];

async function replaceInFile(filePath: string) {
    try {
        let content = await Deno.readTextFile(filePath);

        // Replace various import/require patterns for 'loro-wasm'
        const isWebIndexJs = filePath.includes("web") && filePath.endsWith("index.js");
        const target = isWebIndexJs ? "./loro_wasm.js" : "./loro_wasm";

        content = content.replace(
            /from ["']loro-wasm["']/g,
            `from "${target}"`
        );
        content = content.replace(
            /require\(["']loro-wasm["']\)/g,
            `require("${target}")`
        );
        content = content.replace(
            /import\(["']loro-wasm["']\)/g,
            `import("${target}")`
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

async function main() {
    for (const dir of DIRS_TO_SCAN) {
        try {
            for await (const entry of walk(dir, {
                includeDirs: false,
                match: [/index\.(js|d\.ts)$/],
            })) {
                if (FILES_TO_PROCESS.includes(entry.name)) {
                    await replaceInFile(entry.path);
                }
            }
        } catch (error) {
            console.error(`Error scanning directory ${dir}:`, error);
        }
    }
}

if (import.meta.main) {
    main();
}
