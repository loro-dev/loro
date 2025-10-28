import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import type { Plugin } from "vite";

/**
 * Vite plug-in that keeps wasm-bindgen sourcemaps (and optional DWARF debug
 * companions) working seamlessly in both dev and production builds. It detects
 * `.wasm` binaries automatically, serves the corresponding `.wasm.map` files,
 * mirrors `.debug.wasm` when present, and optionally inlines Rust source code
 * into the sourcemap so browser devtools can display it without filesystem
 * access.
 */

export interface WasmArtifact {
  /** Path to the compiled Wasm binary (absolute or relative to `workspaceRoot`). */
  wasm: string;
  /** Optional sourcemap path; defaults to `${wasm}.map`. */
  map?: string;
  /** Optional debug Wasm path; defaults to `${basename(wasm)}.debug.wasm` beside the binary. */
  debug?: string;
}

export interface ViteWasmDebugOptions {
  /** Optional list of Wasm artifacts to manage explicitly. */
  artifacts?: WasmArtifact[];
  /**
   * Workspace root used to resolve relative paths and embed Rust sources.
   * Defaults to the directory containing the Vite config.
   */
  workspaceRoot?: string;
  /**
   * When true (default), embed Rust `sourcesContent` into the sourcemap so the
   * browser doesn't need filesystem access to display original code.
   */
  embedSources?: boolean;
}

interface ResolvedArtifact {
  wasm: string;
  map: string;
  debug: string;
  wasmBase: string;
  mapBase: string;
  debugBase: string;
  searchBases: string[];
}

const DEFAULT_DEBUG_SUFFIX = ".debug.wasm";

/**
 * Normalise Vite module ids (e.g. `\0`, `file://`, `@fs/`) into absolute file
 * system paths. Returns `null` for virtual modules.
 */
const normalizePath = (input: string): string | null => {
  let id = input;
  if (id.startsWith('\u0000')) {
    id = id.slice(1);
  }
  if (id.startsWith('file://')) {
    try {
      return fileURLToPath(id);
    } catch {
      return null;
    }
  }
  if (id.startsWith('/@fs/')) {
    return '/' + id.slice('/@fs/'.length);
  }
  if (id.startsWith('@fs/')) {
    return '/' + id.slice('@fs/'.length);
  }
  if (path.isAbsolute(id)) {
    return id;
  }
  return null;
};

export function viteWasmDebug(
  options: ViteWasmDebugOptions = {},
): Plugin {
  // If the caller didn't specify a root, use the cwd until Vite resolves it.
  const embedSources = options.embedSources ?? true;
  let workspaceRoot = options.workspaceRoot
    ? path.resolve(options.workspaceRoot)
    : process.cwd();

  const artifactMap = new Map<string, ResolvedArtifact>();
  const presetArtifacts = options.artifacts ? [...options.artifacts] : [];

/**
 * Build a list of directories to search when resolving sourcemap entries. We
 * include the workspace root and every ancestor directory of the wasm binary so
 * relative paths like `crates/foo/src/lib.rs` can always be found.
 */
const computeSearchBases = (wasmPath: string, rootDir: string): string[] => {
  const bases = new Set<string>();
  bases.add(rootDir);
  let current = path.dirname(wasmPath);
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

/**
 * Register a wasm artifact and its sidecars. This memoizes basic metadata and
 * avoids re-registering the same binary.
 */
const registerArtifact = (wasmPath: string, mapPath?: string, debugPath?: string) => {
  const normalizedWasm = path.resolve(workspaceRoot, wasmPath);
  if (!fs.existsSync(normalizedWasm)) {
    return;
  }
  if (artifactMap.has(normalizedWasm)) {
      return;
    }
    const map = mapPath
      ? path.resolve(workspaceRoot, mapPath)
      : fs.existsSync(`${normalizedWasm}.map`)
        ? `${normalizedWasm}.map`
        : path.resolve(
          path.dirname(normalizedWasm),
          `${path.basename(normalizedWasm)}.map`,
        );
    const baseName = path.basename(normalizedWasm, ".wasm");
    const debug = debugPath
      ? path.resolve(workspaceRoot, debugPath)
      : path.resolve(
        path.dirname(normalizedWasm),
        `${baseName}${DEFAULT_DEBUG_SUFFIX}`,
      );

    artifactMap.set(normalizedWasm, {
      wasm: normalizedWasm,
      map,
      debug,
      wasmBase: path.basename(normalizedWasm),
      mapBase: path.basename(map),
      debugBase: path.basename(debug),
      searchBases: computeSearchBases(normalizedWasm, workspaceRoot),
    });
  };

  const cachedMap = new Map<string, { mtime: number; buffer: Buffer }>();
  const cachedDebug = new Map<string, { mtime: number; buffer: Buffer }>();

  const resolveSource = (artifact: ResolvedArtifact, source: string): string | null => {
    if (source.startsWith("file://")) {
      source = fileURLToPath(source);
    }
    const absolute = path.isAbsolute(source)
      ? source
      : null;
    if (absolute && fs.existsSync(absolute)) {
      return absolute;
    }
    for (const base of artifact.searchBases) {
      const candidate = path.resolve(base, source);
      if (fs.existsSync(candidate)) {
        return candidate;
      }
    }
    return null;
  };

  const loadMapBuffer = (artifact: ResolvedArtifact): Buffer => {
    const stat = fs.statSync(artifact.map);
    const cached = cachedMap.get(artifact.map);
    if (cached && cached.mtime === stat.mtimeMs) {
      return cached.buffer;
    }
    const raw = fs.readFileSync(artifact.map, "utf8");
    if (!embedSources) {
      const buf = Buffer.from(raw);
      cachedMap.set(artifact.map, { mtime: stat.mtimeMs, buffer: buf });
      return buf;
    }
    let parsed: any;
    try {
      parsed = JSON.parse(raw);
    } catch {
      const buf = Buffer.from(raw);
      cachedMap.set(artifact.map, { mtime: stat.mtimeMs, buffer: buf });
      return buf;
    }

    if (Array.isArray(parsed.sources)) {
      if (!Array.isArray(parsed.sourcesContent)) {
        parsed.sourcesContent = new Array(parsed.sources.length).fill(null);
      }
      parsed.sourcesContent = parsed.sources.map(
        (source: string, idx: number) => {
          if (
            parsed.sourcesContent[idx] != null &&
            typeof parsed.sourcesContent[idx] === "string"
          ) {
            return parsed.sourcesContent[idx];
          }
          const resolved = resolveSource(artifact, source);
          if (!resolved) {
            return null;
          }
          try {
            return fs.readFileSync(resolved, "utf8");
          } catch {
            return null;
          }
        },
      );
    }
    const buffer = Buffer.from(JSON.stringify(parsed));
    cachedMap.set(artifact.map, { mtime: stat.mtimeMs, buffer });
    return buffer;
  };

  const loadDebugBuffer = (artifact: ResolvedArtifact): Buffer => {
    const stat = fs.statSync(artifact.debug);
    const cached = cachedDebug.get(artifact.debug);
    if (cached && cached.mtime === stat.mtimeMs) {
      return cached.buffer;
    }
    const buffer = fs.readFileSync(artifact.debug);
    cachedDebug.set(artifact.debug, { mtime: stat.mtimeMs, buffer });
    return buffer;
  };

  const matchArtifact = (
    url: string,
  ): { artifact: ResolvedArtifact; kind: "map" | "debug" } | undefined => {
    const pathname = decodeURIComponent(url.split("?")[0] ?? "");
    const fileName = path.basename(pathname);
    for (const artifact of artifactMap.values()) {
      const stem = artifact.wasmBase.replace(/\.wasm$/, "");
      if (
        fileName === artifact.mapBase ||
        (fileName.startsWith(`${stem}-`) && fileName.endsWith(".wasm.map"))
      ) {
        return { artifact, kind: "map" };
      }
      if (
        fileName === artifact.debugBase ||
        (fileName.startsWith(`${stem}-`) && fileName.endsWith(".debug.wasm"))
      ) {
        return { artifact, kind: "debug" };
      }
    }
    return undefined;
  };

  const devMiddleware = (req: { url?: string }, res: any, next: (err?: Error) => void) => {
    if (!req.url) {
      return next();
    }
    let match = matchArtifact(req.url ?? "");
    if (!match) {
      const rawPath = decodeURIComponent((req.url ?? "").split("?")[0] ?? "");
      const normalized = normalizePath(rawPath);
      if (normalized && fs.existsSync(normalized)) {
        if (normalized.endsWith(".wasm.map")) {
          const wasmCandidate = normalized.replace(/\.wasm\.map$/, ".wasm");
          const debugCandidate = normalized.replace(/\.wasm\.map$/, DEFAULT_DEBUG_SUFFIX);
          registerArtifact(wasmCandidate, normalized, fs.existsSync(debugCandidate) ? debugCandidate : undefined);
        } else if (normalized.endsWith(".debug.wasm")) {
          const wasmCandidate = normalized.replace(/\.debug\.wasm$/, ".wasm");
          registerArtifact(wasmCandidate, undefined, normalized);
        } else if (normalized.endsWith(".wasm")) {
          registerArtifact(normalized);
        }
        match = matchArtifact(req.url ?? "");
      }
    }
    if (!match) {
      return next();
    }
    const { artifact, kind } = match;
    try {
     if (kind === "map") {
        if (!fs.existsSync(artifact.map)) {
          return next();
        }
        const buffer = loadMapBuffer(artifact);
        res.statusCode = 200;
        res.setHeader("content-type", "application/json");
        res.end(buffer);
        return;
      }
      if (kind === "debug") {
        if (!fs.existsSync(artifact.debug)) {
          return next();
        }
        const buffer = loadDebugBuffer(artifact);
        res.statusCode = 200;
        res.setHeader("content-type", "application/wasm");
        res.end(buffer);
        return;
      }
    } catch (err) {
      next(err as Error);
      return;
    }
    next();
  };

  return {
    name: "vite-wasm-debug",
    configResolved(config) {
      workspaceRoot = options.workspaceRoot
        ? path.resolve(options.workspaceRoot)
        : config.root;
      for (const artifact of artifactMap.values()) {
        artifact.searchBases = computeSearchBases(artifact.wasm, workspaceRoot);
      }
      if (presetArtifacts.length) {
        for (const artifact of presetArtifacts) {
          registerArtifact(artifact.wasm, artifact.map, artifact.debug);
        }
      }
    },
    configureServer(server) {
      server.middlewares.use((req, res, next) => devMiddleware(req, res, next));
    },
    load(id) {
      const cleanId = id.split("?")[0] ?? "";
      if (!cleanId.endsWith(".wasm")) {
        return null;
      }
      const normalized = normalizePath(cleanId);
      if (normalized && fs.existsSync(normalized)) {
        registerArtifact(normalized);
      }
      return null;
    },
    generateBundle(_options, bundle) {
      for (const artifact of artifactMap.values()) {
        const hasMap = fs.existsSync(artifact.map);
        const hasDebug = fs.existsSync(artifact.debug);
        if (!hasMap && !hasDebug) {
          continue;
        }
        const mapSource = hasMap ? loadMapBuffer(artifact) : undefined;
        const debugSource = hasDebug ? loadDebugBuffer(artifact) : undefined;

        let emitted = false;
        for (const chunk of Object.values(bundle)) {
          if (
            chunk &&
            chunk.type === "asset" &&
            typeof chunk.fileName === "string" &&
            chunk.fileName.endsWith(".wasm") &&
            matchArtifact(chunk.fileName)?.artifact === artifact
          ) {
            emitted = true;
            const base = chunk.fileName.replace(/\.wasm$/, "");
            if (hasMap && mapSource) {
              this.emitFile({
                type: "asset",
                fileName: `${base}.wasm.map`,
                source: mapSource,
              });
            }
            if (hasDebug && debugSource) {
              this.emitFile({
                type: "asset",
                fileName: `${base}.debug.wasm`,
                source: debugSource,
              });
            }
          }
        }
        if (!emitted) {
          if (hasMap && mapSource) {
            this.emitFile({
              type: "asset",
              fileName: `assets/${artifact.mapBase}`,
              source: mapSource,
            });
          }
          if (hasDebug && debugSource) {
            this.emitFile({
              type: "asset",
              fileName: `assets/${artifact.debugBase}`,
              source: debugSource,
            });
          }
        }
      }
    },
  };
}

export default viteWasmDebug;
