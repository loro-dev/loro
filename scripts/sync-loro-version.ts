import { compare as semverCompare, parse as semverParse } from "npm:semver";
import { readFileSync, writeFileSync } from "node:fs";

/**
 * Syncs the version between package.json and a version file,
 * using the higher version number
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

  // Compare versions
  const comparison = semverCompare(packageVersion, versionFileVersion);

  if (comparison > 0) {
    // package.json version is higher
    writeFileSync(versionFilePath, packageVersion);
    console.log(`Updated version file to ${packageVersion}`);
    if (checkVersion && checkVersion !== packageVersion) {
      throw new Error(
        `Version mismatch: Expected version ${checkVersion} but found ${packageVersion} in package.json and ${versionFileVersion} in VERSION file`,
      );
    }
  } else if (comparison < 0) {
    // version file version is higher
    packageJson.version = versionFileVersion;
    writeFileSync(packageJsonPath, JSON.stringify(packageJson, null, 2) + "\n");
    console.log(`Updated package.json to ${versionFileVersion}`);
    if (checkVersion && checkVersion !== versionFileVersion) {
      throw new Error(
        `Version mismatch: Expected version ${checkVersion} but found ${packageVersion} in package.json and ${versionFileVersion} in VERSION file`,
      );
    }
  } else {
    console.log("Versions are already in sync");
    if (checkVersion && checkVersion !== versionFileVersion) {
      throw new Error(
        `Version mismatch: Expected version ${checkVersion} but found ${packageVersion} in package.json and ${versionFileVersion} in VERSION file`,
      );
    }
  }
}

export function runSyncLoroVersion(checkVersion: string = "") {
  syncLoroVersion(
    "./crates/loro-wasm/package.json",
    "./crates/loro-wasm/VERSION",
    checkVersion,
  );
}

if (import.meta.main) {
  runSyncLoroVersion();
}
