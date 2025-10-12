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

    const legacyPattern =
        /\{\s*const wkmod = await import\('\.\/loro_wasm_bg-([^']+)\.js'\);\s*const instance = new WebAssembly\.Instance\(wkmod\.default, \{\s*"\.\/loro_wasm_bg\.js": imports,\s*\}\);\s*__wbg_set_wasm\(instance\.exports\);\s*\}/;
    const legacyReplacement = (match: string, hash: string) => `
import loro_wasm_bg_js from './loro_wasm_bg-${hash}.js';
const instance = new WebAssembly.Instance(loro_wasm_bg_js(), {
    "./loro_wasm_bg.js": imports,
});
__wbg_set_wasm(instance.exports);
`;

    const modernPattern =
        /const wkmod = await Promise\.resolve\(\)\.then\(function \(\) { return wasm\$1; }\);\s*const instance = new WebAssembly\.Instance\(wkmod\.default, \{\s*"\.\/loro_wasm_bg\.js": imports,\s*\}\);\s*__wbg_set_wasm\(instance\.exports\);\s*/;
    const modernReplacement = () => `const instance = loro_wasm_bg({
    "./loro_wasm_bg.js": imports,
});
__wbg_set_wasm(instance.exports);
`;

    let nextContent = content;
    let replaced = false;

    if (legacyPattern.test(nextContent)) {
        nextContent = nextContent.replace(legacyPattern, legacyReplacement);
        replaced = true;
    } else if (modernPattern.test(nextContent)) {
        nextContent = nextContent.replace(modernPattern, modernReplacement);
        replaced = true;
    }

    if (!replaced) {
        throw new Error(
            `Could not find string to replace in ${base64IndexPath}`,
        );
    }

    await Deno.writeTextFile(base64IndexPath, nextContent);

    await Deno.copyFile("./bundler/loro_wasm.d.ts", "./base64/loro_wasm.d.ts");
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
