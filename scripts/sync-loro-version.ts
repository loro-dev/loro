import { compare as semverCompare, parse as semverParse } from "npm:semver";
import { readFileSync, writeFileSync } from "node:fs";

/**
 * Syncs the version between package.json and a version file,
 * using a provided target version when specified, otherwise the higher
 * version number between the two sources.
 */
function ensureSemver(version: string, label: string) {
  if (!semverParse(version)) {
    throw new Error(`Invalid ${label} version ${version}`);
  }

  return version;
}

function writePackageJson(path: string, json: unknown) {
  writeFileSync(path, JSON.stringify(json, null, 2) + "\n");
}

function syncCargoVersion(cargoTomlPath: string, targetVersion: string) {
  const contents = readFileSync(cargoTomlPath, "utf-8");
  const lines = contents.split(/\r?\n/);
  let inPackageSection = false;
  let updated = false;
  let foundVersion = false;

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const trimmed = line.trim();

    if (trimmed.startsWith("[")) {
      inPackageSection = trimmed === "[package]";
      continue;
    }

    if (!inPackageSection || !trimmed.startsWith("version")) {
      continue;
    }

    foundVersion = true;
    const match = line.match(/^(\s*version\s*=\s*")([^"]+)(".*)$/);
    if (!match) {
      throw new Error(`Unable to parse version line in ${cargoTomlPath}`);
    }

    const currentVersion = ensureSemver(match[2], cargoTomlPath);
    if (currentVersion === targetVersion) {
      break;
    }

    lines[i] = `${match[1]}${targetVersion}${match[3]}`;
    updated = true;
    break;
  }

  if (!foundVersion) {
    throw new Error(`Could not locate package version in ${cargoTomlPath}`);
  }

  if (updated) {
    writeFileSync(cargoTomlPath, lines.join("\n"));
    console.log(`Synchronized to ${targetVersion}: updated ${cargoTomlPath}`);
  } else {
    console.log(`Versions already match ${targetVersion} for ${cargoTomlPath}`);
  }
}

/**
 * @param packageJsonPath path to the package.json file whose version may change
 * @param checkVersion optional explicit version to force across both sources
 */
export function syncLoroVersion(
  packageJsonPath: string,
  checkVersion: string = "",
) {
  const packageJson = JSON.parse(readFileSync(packageJsonPath, "utf-8"));
  const packageVersion = ensureSemver(
    (packageJson as { version: string }).version,
    packageJsonPath,
  );

  const targetVersion = checkVersion
    ? ensureSemver(checkVersion, "target")
    : packageVersion;

  if (packageVersion === targetVersion) {
    console.log(`Versions already match ${targetVersion} for ${packageJsonPath}`);
    return targetVersion;
  }

  (packageJson as { version: string }).version = targetVersion;
  writePackageJson(packageJsonPath, packageJson);
  console.log(`Synchronized to ${targetVersion}: updated ${packageJsonPath}`);

  return targetVersion;
}

export function runSyncLoroVersion(checkVersion: string = "") {
  const wasmVersion = syncLoroVersion(
    "./crates/loro-wasm/package.json",
    checkVersion,
  );

  if (checkVersion && checkVersion !== wasmVersion) {
    throw new Error(
      `Expected version ${checkVersion} but found ${wasmVersion} in ./crates/loro-wasm/package.json`,
    );
  }

  syncCargoVersion("./crates/loro-wasm/Cargo.toml", wasmVersion);

  syncLoroVersion(
    "./crates/loro-wasm-map/package.json",
    wasmVersion,
  );
}

if (import.meta.main) {
  runSyncLoroVersion();
}
