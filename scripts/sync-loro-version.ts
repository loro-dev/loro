import { compare as semverCompare, parse as semverParse } from "npm:semver";
import { readFileSync, writeFileSync } from "node:fs";

/**
 * Syncs the version between package.json and a version file,
 * using a provided target version when specified, otherwise the higher
 * version number between the two sources.
 */
export function syncLoroVersion(
  packageJsonPath: string,
  versionFilePath: string,
  checkVersion: string = "",
) {
  // Read package.json
  const packageJson = JSON.parse(readFileSync(packageJsonPath, "utf-8"));
  const packageVersion = packageJson.version;

  // Read version file
  const versionFileContent = readFileSync(versionFilePath, "utf-8");
  const versionFileVersion = versionFileContent.trim();

  // Parse and compare versions
  const parsedPackageVersion = semverParse(packageVersion);
  const parsedFileVersion = semverParse(versionFileVersion);

  if (!parsedPackageVersion || !parsedFileVersion) {
    throw new Error("Invalid version format found");
  }

  if (checkVersion) {
    const parsedCheckVersion = semverParse(checkVersion);
    if (!parsedCheckVersion) {
      throw new Error(`Invalid target version ${checkVersion}`);
    }

    let updated = false;
    if (packageVersion !== checkVersion) {
      packageJson.version = checkVersion;
      writeFileSync(
        packageJsonPath,
        JSON.stringify(packageJson, null, 2) + "\n",
      );
      updated = true;
    }

    if (versionFileVersion !== checkVersion) {
      writeFileSync(versionFilePath, checkVersion);
      updated = true;
    }

    if (updated) {
      console.log(
        `Synchronized ${packageJsonPath} and ${versionFilePath} to ${checkVersion}`,
      );
    } else {
      console.log(
        `Versions already match the target ${checkVersion} for ${packageJsonPath} and ${versionFilePath}`,
      );
    }

    return;
  }

  // Compare versions
  const comparison = semverCompare(packageVersion, versionFileVersion);

  if (comparison > 0) {
    // package.json version is higher
    writeFileSync(versionFilePath, packageVersion);
    console.log(`Updated version file to ${packageVersion}`);
  } else if (comparison < 0) {
    // version file version is higher
    packageJson.version = versionFileVersion;
    writeFileSync(packageJsonPath, JSON.stringify(packageJson, null, 2) + "\n");
    console.log(`Updated package.json to ${versionFileVersion}`);
  } else {
    console.log("Versions are already in sync");
  }
}

export function runSyncLoroVersion(checkVersion: string = "") {
  syncLoroVersion(
    "./crates/loro-wasm/package.json",
    "./crates/loro-wasm/VERSION",
    checkVersion,
  );
  const wasmVersion = readFileSync("./crates/loro-wasm/VERSION", "utf-8").trim();

  if (checkVersion && checkVersion !== wasmVersion) {
    throw new Error(
      `Expected version ${checkVersion} but found ${wasmVersion} in ./crates/loro-wasm/VERSION`,
    );
  }

  syncLoroVersion(
    "./crates/loro-wasm-map/package.json",
    "./crates/loro-wasm-map/VERSION",
    wasmVersion,
  );
}

if (import.meta.main) {
  runSyncLoroVersion();
}
