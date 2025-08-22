import { compare as semverCompare } from "npm:semver";
import { readFileSync, writeFileSync } from "node:fs";

/**
 * Syncs the version between package.json and a version file,
 * using the higher version number
 */
export function syncLoroVersion(
  versionFilePath: string,
  newVersion: string = "",
) {
  // Read version file
  const versionFileContent = readFileSync(versionFilePath, "utf-8");
  const versionFileVersion = versionFileContent.trim();

  // Compare versions
  console.log(`Comparing versions: ${newVersion} and ${versionFileVersion}`);
  const comparison = semverCompare(newVersion, versionFileVersion);

  if (comparison > 0) {
    // new version is higher
    writeFileSync(versionFilePath, newVersion);
    console.log(`Updated version file to ${newVersion}`);
  } else if (comparison < 0) {
    throw new Error(`The new version is lower than the current version in VERSION file`)
  } else {
    console.log("Versions are already in sync");
  }
}

export function runSyncLoroVersion(newVersion: string = "") {
  syncLoroVersion(
    "./crates/loro-internal/VERSION",
    newVersion
  );
}

if (import.meta.main) {
  runSyncLoroVersion();
}
