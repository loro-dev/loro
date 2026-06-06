import { spawn } from "node:child_process";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const workspaceRoot = path.resolve(__dirname, "..");

const packagesToPublish = [
  "crates/loro-wasm",
  "crates/loro-wasm-map",
  "packages/fractional-index",
];

function run(command, args, options = {}) {
  return new Promise((resolve) => {
    const child = spawn(command, args, {
      cwd: workspaceRoot,
      env: process.env,
      stdio: options.capture ? ["ignore", "pipe", "pipe"] : "inherit",
    });

    let stdout = "";
    let stderr = "";
    if (options.capture) {
      child.stdout.on("data", (chunk) => {
        stdout += chunk;
      });
      child.stderr.on("data", (chunk) => {
        stderr += chunk;
      });
    }

    child.on("close", (code) => {
      resolve({ code, stdout, stderr });
    });
    child.on("error", (error) => {
      resolve({ code: 1, stdout, stderr: `${stderr}\n${error.message}` });
    });
  });
}

async function readPackageJson(packageDir) {
  const packageJsonPath = path.join(workspaceRoot, packageDir, "package.json");
  return JSON.parse(await readFile(packageJsonPath, "utf8"));
}

function isNotPublished(output) {
  return (
    output.includes("E404") ||
    output.includes("No match found for version") ||
    output.includes("could not be found")
  );
}

async function isPublished(name, version) {
  const spec = `${name}@${version}`;
  const result = await run("npm", ["view", spec, "version", "--json"], {
    capture: true,
  });

  if (result.code === 0) {
    console.log(`skip: ${spec} already exists on npm`);
    return true;
  }

  const output = `${result.stdout}\n${result.stderr}`;
  if (isNotPublished(output)) {
    console.log(`publish: ${spec} is not published yet`);
    return false;
  }

  console.error(output);
  throw new Error(`Failed to check npm publication state for ${spec}`);
}

async function publishPackage(packageDir) {
  const packageJson = await readPackageJson(packageDir);
  if (packageJson.private) {
    console.log(`skip: ${packageJson.name} is private`);
    return;
  }

  const { name, version } = packageJson;
  if (await isPublished(name, version)) {
    return;
  }

  const access = packageJson.publishConfig?.access ?? "public";
  console.log(`publishing ${name}@${version} from ${packageDir}`);
  const result = await run("npm", [
    "publish",
    packageDir,
    "--access",
    access,
    "--tag",
    "latest",
  ]);

  if (result.code !== 0) {
    throw new Error(`npm publish failed for ${name}@${version}`);
  }

  await createLocalTag(`${name}@${version}`);
}

async function createLocalTag(tagName) {
  const existingTag = await run(
    "git",
    ["rev-parse", "-q", "--verify", `refs/tags/${tagName}`],
    { capture: true },
  );

  if (existingTag.code !== 0) {
    const tagResult = await run("git", ["tag", tagName]);
    if (tagResult.code !== 0) {
      throw new Error(`Failed to create git tag ${tagName}`);
    }
  }

  console.log(`New tag:  ${tagName}`);
}

for (const packageDir of packagesToPublish) {
  await publishPackage(packageDir);
}
