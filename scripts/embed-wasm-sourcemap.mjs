#!/usr/bin/env node

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const VIRTUAL_SCHEME = "loro";
const VIRTUAL_PREFIX = `${VIRTUAL_SCHEME}:///`;
const ABSOLUTE_PREFIX = "__abs__";

const resolveArgPath = (value) =>
  value ? path.resolve(process.cwd(), value) : undefined;

const parseArgs = (argv) => {
  const result = new Map();
  for (let i = 0; i < argv.length; i++) {
    const raw = argv[i];
    if (!raw.startsWith("--")) continue;
    const [flag, inline] = raw.slice(2).split("=", 2);
    if (inline !== undefined) {
      result.set(flag, inline);
      continue;
    }
    const next = argv[i + 1];
    if (next && !next.startsWith("--")) {
      result.set(flag, next);
      i += 1;
    } else {
      result.set(flag, "true");
    }
  }
  return result;
};

const args = parseArgs(process.argv.slice(2));
const mapPath =
  resolveArgPath(args.get("map")) ??
  path.resolve("crates/loro-wasm/bundler/loro_wasm_bg.wasm.map");
const wasmPath =
  resolveArgPath(args.get("wasm")) ?? mapPath.replace(/\.wasm\.map$/, ".wasm");
const workspaceRoot =
  resolveArgPath(args.get("workspace-root")) ?? process.cwd();
const outPath = resolveArgPath(args.get("out")) ?? mapPath;

const sanitizePathSeparators = (value) => value.replace(/\\/g, "/");
const stripLeadingSlashes = (value) => value.replace(/^\/+/, "");
const stripLeadingDotSegments = (value) => {
  let output = value;
  while (output.startsWith("./")) {
    output = output.slice(2);
  }
  while (output.startsWith("../")) {
    output = output.slice(3);
  }
  return output;
};

const homeDir = typeof os.homedir === "function" ? os.homedir() : undefined;
const homeDirSanitized = homeDir ? sanitizePathSeparators(homeDir) : undefined;
const driveLetterPattern = /^[A-Za-z]:\//;

if (!fs.existsSync(mapPath)) {
  console.error(`sourcemap not found: ${mapPath}`);
  process.exitCode = 1;
  process.exit();
}
if (!fs.existsSync(wasmPath)) {
  console.error(`wasm binary not found: ${wasmPath}`);
  process.exitCode = 1;
  process.exit();
}

const computeSearchBases = (wasmFile, rootDir) => {
  const bases = new Set([rootDir, path.dirname(mapPath)]);
  let current = path.dirname(wasmFile);
  while (!bases.has(current)) {
    bases.add(current);
    const parent = path.dirname(current);
    if (parent === current) break;
    current = parent;
  }
  return Array.from(bases);
};

const searchBases = computeSearchBases(wasmPath, workspaceRoot);

const makeVirtualUrl = (payload) => {
  const sanitized = sanitizePathSeparators(payload ?? "");
  const trimmed = sanitized.startsWith("/")
    ? sanitized.slice(1)
    : sanitized;
  return `${VIRTUAL_PREFIX}${trimmed}`;
};

const resolveAbsolutePayload = (payload) => {
  let cleaned = stripLeadingSlashes(sanitizePathSeparators(payload));
  cleaned = stripLeadingDotSegments(cleaned);
  if (!cleaned) return null;
  if (cleaned.startsWith("~/")) {
    if (!homeDir) return null;
    const tail = cleaned.slice(2);
    const parts = tail ? tail.split("/") : [];
    return path.resolve(homeDir, ...parts);
  }
  if (driveLetterPattern.test(cleaned)) {
    return path.normalize(cleaned);
  }
  const root =
    path.parse(workspaceRoot).root ||
    path.parse(process.cwd()).root ||
    path.sep;
  const parts = cleaned.split("/");
  return path.resolve(root, ...parts);
};

const resolveVirtualSource = (value) => {
  if (!value.startsWith(VIRTUAL_PREFIX)) {
    return null;
  }
  return value.slice(VIRTUAL_PREFIX.length);
};

const resolveSource = (source) => {
  if (typeof source !== "string") {
    return null;
  }

  let candidate = sanitizePathSeparators(source);
  const virtualPayload = resolveVirtualSource(candidate);
  if (virtualPayload != null) {
    if (virtualPayload.startsWith(`${ABSOLUTE_PREFIX}/`)) {
      const absolutePayload = virtualPayload.slice(ABSOLUTE_PREFIX.length + 1);
      return resolveAbsolutePayload(absolutePayload);
    }
    const relative = stripLeadingDotSegments(
      stripLeadingSlashes(virtualPayload),
    );
    if (!relative) {
      return null;
    }
    const parts = relative.split("/").filter(Boolean);
    return path.resolve(workspaceRoot, ...parts);
  }

  if (candidate.startsWith("file://")) {
    try {
      candidate = fileURLToPath(candidate);
    } catch {
      return null;
    }
  }
  if (path.isAbsolute(candidate) && fs.existsSync(candidate)) {
    return candidate;
  }
  for (const base of searchBases) {
    const resolved = path.resolve(base, candidate);
    if (fs.existsSync(resolved)) {
      return resolved;
    }
  }
  return null;
};

let map;
try {
  map = JSON.parse(fs.readFileSync(mapPath, "utf8"));
} catch (error) {
  console.error(`failed to read sourcemap ${mapPath}: ${error}`);
  process.exitCode = 1;
  process.exit();
}

if (!Array.isArray(map.sources)) {
  console.error(`missing sources array in ${mapPath}`);
  process.exitCode = 1;
  process.exit();
}

const sourcesContent = Array.isArray(map.sourcesContent)
  ? map.sourcesContent
  : [];
const nextSources = new Array(map.sources.length);
const nextSourcesContent = new Array(map.sources.length).fill(null);

const virtualizeWorkspacePath = (resolvedPath) => {
  const relative = path.relative(workspaceRoot, resolvedPath);
  const inside =
    relative === "" || (!relative.startsWith("..") && !path.isAbsolute(relative));
  if (!inside) {
    return null;
  }
  const payload = relative === ""
    ? sanitizePathSeparators(path.basename(resolvedPath))
    : sanitizePathSeparators(relative);
  return makeVirtualUrl(payload || path.basename(resolvedPath));
};

const virtualizeAbsolutePath = (resolvedPath) => {
  let sanitized = sanitizePathSeparators(resolvedPath);
  if (homeDirSanitized) {
    if (sanitized === homeDirSanitized) {
      sanitized = "~";
    } else if (sanitized.startsWith(`${homeDirSanitized}/`)) {
      sanitized = `~/${sanitized.slice(homeDirSanitized.length + 1)}`;
    }
  }
  return makeVirtualUrl(`${ABSOLUTE_PREFIX}/${sanitized}`);
};

const fallbackVirtualSource = (source) => {
  if (typeof source !== "string") {
    return makeVirtualUrl("unknown");
  }
  const cleaned = stripLeadingDotSegments(sanitizePathSeparators(source));
  return makeVirtualUrl(cleaned || "unknown");
};

let missed = 0;
for (let i = 0; i < map.sources.length; i++) {
  const originalSource = map.sources[i];
  const resolved = resolveSource(originalSource);

  if (resolved) {
    const workspaceVirtual = virtualizeWorkspacePath(resolved);
    nextSources[i] = workspaceVirtual ?? virtualizeAbsolutePath(resolved);
  } else {
    nextSources[i] = fallbackVirtualSource(originalSource);
  }

  if (resolved) {
    try {
      nextSourcesContent[i] = fs.readFileSync(resolved, "utf8");
    } catch {
      nextSourcesContent[i] = sourcesContent[i] ?? null;
      if (nextSourcesContent[i] == null) missed += 1;
    }
  } else {
    nextSourcesContent[i] = sourcesContent[i] ?? null;
    if (nextSourcesContent[i] == null) missed += 1;
  }
}

map.sources = nextSources;
map.sourcesContent = nextSourcesContent;
map.sourceRoot = "";

try {
  fs.writeFileSync(outPath, JSON.stringify(map));
} catch (error) {
  console.error(`failed to write sourcemap ${outPath}: ${error}`);
  process.exitCode = 1;
  process.exit();
}

const inlined = nextSourcesContent.length - missed;
console.log(
  `embedded sources for ${inlined}/${nextSourcesContent.length} entries into ${outPath}`,
);
