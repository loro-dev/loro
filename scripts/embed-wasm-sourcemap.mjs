#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const resolveArgPath = (value) =>
  value ? path.resolve(process.cwd(), value) : undefined;

const parseArgs = (argv) => {
  const result = new Map();
  for (let i = 0; i < argv.length; i++) {
    const raw = argv[i];
    if (!raw.startsWith("--")) {
      continue;
    }
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
const baseArg = args.get("base") ?? args.get("scheme") ?? "@loro-source";
const absBaseArg =
  args.get("abs-base") ?? args.get("abs-scheme") ?? `${baseArg}-abs`;

const normalizeVirtualBase = (value) => {
  const trimmed = value.trim();
  if (!trimmed) return "/@loro-source";
  const withoutSlashes = trimmed.replace(/^\/+|\/+$/g, "");
  return `/${withoutSlashes}`;
};

const sanitizePathSeparators = (value) => value.replace(/\\/g, "/");
const virtualSourceBase = normalizeVirtualBase(baseArg);
const virtualAbsoluteBase = normalizeVirtualBase(absBaseArg);

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
    if (parent === current) {
      break;
    }
    current = parent;
  }
  return Array.from(bases);
};

const searchBases = computeSearchBases(wasmPath, workspaceRoot);

const resolveSource = (source) => {
  let candidate = sanitizePathSeparators(source);
  if (candidate === virtualSourceBase) {
    candidate = "";
  } else if (candidate.startsWith(`${virtualSourceBase}/`)) {
    candidate = candidate.slice(virtualSourceBase.length + 1);
  } else if (candidate === virtualAbsoluteBase) {
    candidate = "/";
  } else if (candidate.startsWith(`${virtualAbsoluteBase}/`)) {
    candidate = `/${candidate.slice(virtualAbsoluteBase.length + 1)}`;
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
  const raw = fs.readFileSync(mapPath, "utf8");
  map = JSON.parse(raw);
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

const result = [];
const existing = Array.isArray(map.sourcesContent) ? map.sourcesContent : [];

let missed = 0;
for (let i = 0; i < map.sources.length; i++) {
  const resolved = resolveSource(map.sources[i]);
  if (!resolved) {
    result[i] = existing[i] ?? null;
    if (result[i] == null) {
      missed += 1;
    }
    continue;
  }
  try {
    result[i] = fs.readFileSync(resolved, "utf8");
  } catch {
    result[i] = existing[i] ?? null;
    if (result[i] == null) {
      missed += 1;
    }
  }
}

map.sourcesContent = result;
map.sourceRoot = "";

try {
  fs.writeFileSync(outPath, JSON.stringify(map));
} catch (error) {
  console.error(`failed to write sourcemap ${outPath}: ${error}`);
  process.exitCode = 1;
  process.exit();
}

const inlined = result.length - missed;
console.log(
  `embedded sources for ${inlined}/${result.length} entries into ${outPath}`,
);
