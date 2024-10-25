const LORO_VERSION = "1.0.7";

export interface CodeBlock {
    filename: string;
    filePath: string;
    lineNumber: number;
    lang: string;
    content: string;
}

export function extractCodeBlocks(
    fileContent: string,
    codeBlocks: CodeBlock[],
    name: string,
    path: string,
) {
    // Regular expression to detect TypeScript code blocks
    const codeBlockRegex = /```(typescript|ts|js|javascript)\n([\s\S]*?)```/g;
    let match;
    while ((match = codeBlockRegex.exec(fileContent)) !== null) {
        const startLine =
            fileContent.substring(0, match.index).split("\n").length;
        let content = match[2];
        content = content.replace(/^\s*\*/g, "");
        content = content.replace(/\n\s*\*/g, "\n");
        content = content.replace(/^\s*\/\/\//g, "");
        content = content.replace(/\n\s*\/\/\//g, "\n");
        content = replaceImportVersion(content, LORO_VERSION);
        if (!content.includes("loro-crdt")) {
            content = IMPORTS + content;
        }
        codeBlocks.push({
            filename: name,
            filePath: path,
            lineNumber: startLine,
            content,
            lang: match[1],
        });
    }
}

function replaceImportVersion(input: string, targetVersion: string): string {
    const regex = /from "loro-crdt"/g;
    const replacement = `from "npm:loro-crdt@${targetVersion}"`;
    return input.replace(regex, replacement);
}

const IMPORTS =
    `import { Loro, LoroDoc, LoroMap, LoroText, LoroList, Delta, UndoManager, getType, isContainer } from "npm:loro-crdt@${LORO_VERSION}";
import { expect } from "npm:expect@29.7.0";\n
`;

Deno.test("extract doc tests", async () => {
    const filePath = "./doc-tests-tests/example.txt";
    const fileContent = await Deno.readTextFile(filePath);
    const codeBlocks: CodeBlock[] = [];
    extractCodeBlocks(fileContent, codeBlocks, "example.txt", filePath);
    for (const block of codeBlocks) {
        console.log(block.content);
        console.log("==============================");
    }
    await runCodeBlocks(codeBlocks);
});

export async function runDocTests(paths: string[]) {
    const codeBlocks: CodeBlock[] = [];
    for (const path of paths) {
        const fileContent = await Deno.readTextFile(path);
        extractCodeBlocks(fileContent, codeBlocks, path, path);
    }

    await runCodeBlocks(codeBlocks);
}

async function runCodeBlocks(codeBlocks: CodeBlock[]) {
    let testCases = 0;
    let passed = 0;
    let failed = 0;
    for (const block of codeBlocks) {
        try {
            const command = new Deno.Command("deno", {
                args: ["eval", "--ext=ts", block.content],
                stdout: "null",
                stderr: "inherit",
            });
            const process = command.spawn();
            const status = await process.status;
            testCases += 1;
            if (status.success) {
                passed += 1;
            } else {
                console.log("----------------");
                console.log(block.content);
                console.log("-----------------");
                console.error(
                    `\x1b[31;1mError in \x1b[4m${block.filePath}:${block.lineNumber}\x1b[0m\n\n\n\n\n`,
                );
                failed += 1;
            }
        } catch (error) {
            console.error("Error:", error);
        }
        await Deno.stdout.write(
            new TextEncoder().encode(
                `\r🧪 ${testCases} tests, ✅ ${passed} passed,${
                    failed > 0 ? " ❌" : ""
                } ${failed} failed`,
            ),
        );
    }
}

if (Deno.args.length > 0) {
    await runDocTests(Deno.args);
} else {
    console.log("No paths provided. Please provide paths as arguments.");
}
