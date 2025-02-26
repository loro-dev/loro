import { parse as semverParse } from "npm:semver";
import { readFileSync } from "node:fs";

/**
 * Syncs the version between package.json and a version file,
 * using the higher version number
 */
export function checkLoroVersion(
  packageJsonPath: string,
  checkVersion: string,
) {
  // Read package.json
  const packageJson = JSON.parse(readFileSync(packageJsonPath, "utf-8"));
  const packageVersion = packageJson.version;

  // Parse and compare versions
  const parsedPackageVersion = semverParse(packageVersion);

  if (!parsedPackageVersion) {
    throw new Error("Invalid version format found");
  }

  if (checkVersion && checkVersion !== packageVersion) {
    throw new Error(`Version mismatch: Expected version ${checkVersion} but found ${packageVersion} in package.json`)
  }
}

export function runCheckLoroVersion(checkVersion: string) {
  checkLoroVersion(
    "./crates/loro-wasm/package.json",
    checkVersion
  );
}
