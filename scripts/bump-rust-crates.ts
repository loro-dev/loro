import { parse as parseToml, stringify as stringifyToml } from "@std/toml";
import { walk } from "@std/fs/";

const CRATES = {
    "loro": "loro",
    "loro-internal": "loro-internal",
    "loro-common": "loro-common",
    "rle": "loro-rle",
    "delta": "loro-delta",
    "fractional_index": "loro_fractional_index",
};

async function updateCargoToml(filePath: string, targetVersion: string) {
    let content = await Deno.readTextFile(filePath);
    const crates = Object.values(CRATES);

    // Update package version
    content = content.replace(
        /^\s*version\s*=\s*"[^"]*"/m,
        `version = "${targetVersion}"`,
    );

    // Update dependencies
    const depRegex = new RegExp(
        `^(\\s*)(${
            crates.join("|")
        })\\s*=\\s*(?:("\\S+"|\\{[^}]*version\\s*=\\s*)("[^"]*"))`,
        "gm",
    );
    content = content.replace(depRegex, `$1$2 = $3"${targetVersion}"`);

    // Handle package rename cases and path+version cases
    for (const [oldName, newName] of Object.entries(CRATES)) {
        const packageRegex = new RegExp(
            `^(\\s*${oldName}\\s*=\\s*\\{[^}]*(?:package\\s*=\\s*"${newName}")?[^}]*version\\s*=\\s*)("[^"]*")`,
            "gm",
        );
        content = content.replace(
            packageRegex,
            `$1"${targetVersion}"`,
        );
    }

    // Write updated content back to file
    await Deno.writeTextFile(filePath, content);
    console.log(`Updated ${filePath}`);
}

async function main() {
    const targetVersion = Deno.args[0];
    if (!targetVersion) {
        console.error("Please provide a target version as an argument.");
        Deno.exit(1);
    }

    for (const [key, _] of Object.entries(CRATES)) {
        const cargoTomlPath = `../crates/${key}/Cargo.toml`;
        try {
            await updateCargoToml(cargoTomlPath, targetVersion);
        } catch (error) {
            console.error(`Error updating ${cargoTomlPath}:`, error);
        }
    }

    const crates = Object.values(CRATES);
    // Update dependencies in all Cargo.toml files
    for await (const entry of walk("../crates", { exts: [".toml"] })) {
        if (entry.name === "Cargo.toml") {
            if (
                crates.every((x) => !entry.path.includes("crates/" + x + "/"))
            ) {
                continue;
            }

            try {
                await updateCargoToml(entry.path, targetVersion);
            } catch (error) {
                console.error(`Error updating ${entry.path}:`, error);
            }
        }
    }
}

if (import.meta.main) {
    main();
}
