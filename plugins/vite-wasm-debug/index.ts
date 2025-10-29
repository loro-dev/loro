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
  /** Workspace root used to resolve relative paths and embed Rust sources. */
  workspaceRoot?: string;
  /**
   * When true (default), embed Rust `sourcesContent` into the sourcemap so the
   * browser doesn't need filesystem access to display original code.
   */
  embedSources?: boolean;
  /**
   * When false, the plugin will not attempt to read Rust source files from the
   * local filesystem while processing sourcemaps. Defaults to `embedSources`.
   */
  readSourcesFromDisk?: boolean;
  /**
   * When true (default), rewrite `sources` entries to use a virtual scheme so
   * DevTools does not attempt to fetch local filesystem paths.
   */
  virtualizeSources?: boolean;
}

interface ResolvedArtifact {
  wasm: string;
  map: string;
  debug: string;
  wasmBase: string;
  mapBase: string;
  debugBase: string;
  stem: string;
  searchBases: string[];
  virtualSourceKeys: Set<string>;
}

const DEFAULT_DEBUG_SUFFIX = ".debug.wasm";

type CacheEntry = { mtime: number; buffer: Buffer };

type ArtifactKind = "map" | "debug";

/**
 * Normalise Vite module ids (e.g. `\0`, `file://`, `@fs/`) into absolute file
 * system paths. Returns `null` for virtual modules.
 */
const normalizePath = (input: string): string | null => {
  let id = input;
  if (id.startsWith("\u0000")) {
    id = id.slice(1);
  }
  if (id.startsWith("file://")) {
    try {
      return fileURLToPath(id);
    } catch {
      return null;
    }
  }
  if (id.startsWith("/@fs/")) {
    return "/" + id.slice("/@fs/".length);
  }
  if (id.startsWith("@fs/")) {
    return "/" + id.slice("@fs/".length);
  }
  if (path.isAbsolute(id)) {
    return id;
  }
  return null;
};

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

const resolveSidecarPaths = (
  wasmPath: string,
  mapPath: string | undefined,
  debugPath: string | undefined,
  rootDir: string,
): { map: string; debug: string } => {
  const resolvedMap = mapPath
    ? path.resolve(rootDir, mapPath)
    : path.join(path.dirname(wasmPath), `${path.basename(wasmPath)}.map`);

  const resolvedDebug = debugPath
    ? path.resolve(rootDir, debugPath)
    : path.join(
        path.dirname(wasmPath),
        `${path.basename(wasmPath, ".wasm")}${DEFAULT_DEBUG_SUFFIX}`,
      );

  return { map: resolvedMap, debug: resolvedDebug };
};

const readWithCache = (
  filePath: string,
  cache: Map<string, CacheEntry>,
  reader: () => Buffer,
): Buffer => {
  const stat = fs.statSync(filePath);
  const cached = cache.get(filePath);
  if (cached && cached.mtime === stat.mtimeMs) {
    return cached.buffer;
  }
  const buffer = reader();
  cache.set(filePath, { mtime: stat.mtimeMs, buffer });
  return buffer;
};

export function viteWasmDebug(options: ViteWasmDebugOptions = {}): Plugin {
  const embedSources = options.embedSources ?? true;
  const readSourcesFromDisk = options.readSourcesFromDisk ?? embedSources;
  const virtualizeSources = options.virtualizeSources ?? true;
  let workspaceRoot = options.workspaceRoot
    ? path.resolve(options.workspaceRoot)
    : process.cwd();

  const artifacts = new Map<string, ResolvedArtifact>();
  const presetArtifacts = options.artifacts ? [...options.artifacts] : [];
  const mapCache = new Map<string, CacheEntry>();
  const debugCache = new Map<string, CacheEntry>();
  const virtualSourceCache = new Map<string, string>();

  const registerArtifact = (
    wasmPath: string,
    mapPath?: string,
    debugPath?: string,
  ): ResolvedArtifact | undefined => {
    const absoluteWasm = path.resolve(workspaceRoot, wasmPath);
    if (!fs.existsSync(absoluteWasm)) {
      return undefined;
    }
    const existing = artifacts.get(absoluteWasm);
    if (existing) {
      return existing;
    }

    const { map, debug } = resolveSidecarPaths(
      absoluteWasm,
      mapPath,
      debugPath,
      workspaceRoot,
    );

    const artifact: ResolvedArtifact = {
      wasm: absoluteWasm,
      map,
      debug,
      wasmBase: path.basename(absoluteWasm),
      mapBase: path.basename(map),
      debugBase: path.basename(debug),
      stem: path.basename(absoluteWasm).replace(/\.wasm$/, ""),
      searchBases: computeSearchBases(absoluteWasm, workspaceRoot),
      virtualSourceKeys: new Set(),
    };

    artifacts.set(absoluteWasm, artifact);
    return artifact;
  };

  const resolveSource = (
    artifact: ResolvedArtifact,
    source: string,
  ): string | null => {
    if (!readSourcesFromDisk) {
      return null;
    }
    let candidate = source;
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
    for (const base of artifact.searchBases) {
      const resolved = path.resolve(base, candidate);
      if (fs.existsSync(resolved)) {
        return resolved;
      }
    }
    return null;
  };

  const loadMapBuffer = (artifact: ResolvedArtifact): Buffer =>
    readWithCache(artifact.map, mapCache, () => {
      const raw = fs.readFileSync(artifact.map, "utf8");
      if (!embedSources) {
        return Buffer.from(raw);
      }
      let parsed: any;
      try {
        parsed = JSON.parse(raw);
      } catch {
        return Buffer.from(raw);
      }
      if (Array.isArray(parsed.sources)) {
        if (!Array.isArray(parsed.sourcesContent)) {
          parsed.sourcesContent = new Array(parsed.sources.length).fill(null);
        }
        parsed.sourcesContent = parsed.sources.map(
          (source: string, idx: number) => {
            const existing = parsed.sourcesContent[idx];
            if (typeof existing === "string") {
              return existing;
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
        if (virtualizeSources) {
          parsed.sourceRoot = "";
          for (const key of artifact.virtualSourceKeys) {
            virtualSourceCache.delete(key);
          }
          artifact.virtualSourceKeys.clear();
        }
      }
      return Buffer.from(JSON.stringify(parsed));
    });

  const loadDebugBuffer = (artifact: ResolvedArtifact): Buffer =>
    readWithCache(artifact.debug, debugCache, () =>
      fs.readFileSync(artifact.debug),
    );

  const matchArtifact = (
    filePath: string,
  ): { artifact: ResolvedArtifact; kind: ArtifactKind } | undefined => {
    const name = path.basename(filePath);
    for (const artifact of artifacts.values()) {
      if (
        name === artifact.mapBase ||
        (name.startsWith(`${artifact.stem}-`) && name.endsWith(".wasm.map"))
      ) {
        return { artifact, kind: "map" };
      }
      if (
        name === artifact.debugBase ||
        (name.startsWith(`${artifact.stem}-`) &&
          name.endsWith(DEFAULT_DEBUG_SUFFIX))
      ) {
        return { artifact, kind: "debug" };
      }
    }
    return undefined;
  };

  const registerBySidecar = (filePath: string) => {
    if (!fs.existsSync(filePath)) {
      return;
    }
    if (filePath.endsWith(".wasm.map")) {
      const wasmCandidate = filePath.replace(/\.wasm\.map$/, ".wasm");
      const debugCandidate = filePath.replace(
        /\.wasm\.map$/,
        DEFAULT_DEBUG_SUFFIX,
      );
      registerArtifact(
        wasmCandidate,
        filePath,
        fs.existsSync(debugCandidate) ? debugCandidate : undefined,
      );
      return;
    }
    if (filePath.endsWith(DEFAULT_DEBUG_SUFFIX)) {
      const wasmCandidate = filePath.replace(/\.debug\.wasm$/, ".wasm");
      registerArtifact(wasmCandidate, undefined, filePath);
      return;
    }
    if (filePath.endsWith(".wasm")) {
      registerArtifact(filePath);
    }
  };

  const devMiddleware = (
    req: { url?: string },
    res: any,
    next: (err?: Error) => void,
  ) => {
    const url = req.url;
    if (!url) {
      next();
      return;
    }
    const pathname = decodeURIComponent(url.split("?")[0] ?? "");
    let match = matchArtifact(pathname);
    if (!match) {
      const normalized = normalizePath(pathname);
      if (normalized) {
        registerBySidecar(normalized);
        match = matchArtifact(pathname);
      }
    }
    if (!match) {
      next();
      return;
    }
    const { artifact, kind } = match;
    try {
      if (kind === "map") {
        if (!fs.existsSync(artifact.map)) {
          next();
          return;
        }
        const buffer = loadMapBuffer(artifact);
        res.statusCode = 200;
        res.setHeader("content-type", "application/json");
        res.end(buffer);
        return;
      }
      if (!fs.existsSync(artifact.debug)) {
        next();
        return;
      }
      const buffer = loadDebugBuffer(artifact);
      res.statusCode = 200;
      res.setHeader("content-type", "application/wasm");
      res.end(buffer);
    } catch (error) {
      next(error as Error);
    }
  };

  return {
    name: "vite-wasm-debug",
    configResolved(config) {
      workspaceRoot = options.workspaceRoot
        ? path.resolve(options.workspaceRoot)
        : config.root;
      for (const artifact of artifacts.values()) {
        artifact.searchBases = computeSearchBases(artifact.wasm, workspaceRoot);
      }
      if (presetArtifacts.length) {
        for (const artifact of presetArtifacts) {
          registerArtifact(artifact.wasm, artifact.map, artifact.debug);
        }
      }
    },
    configureServer(server) {
      server.middlewares.use(devMiddleware);
    },
    load(id) {
      const cleanId = id.split("?")[0] ?? "";
      if (!cleanId.endsWith(".wasm")) {
        return null;
      }
      const normalized = normalizePath(cleanId);
      if (normalized) {
        registerBySidecar(normalized);
      }
      return null;
    },
    generateBundle(_options, bundle) {
      for (const artifact of artifacts.values()) {
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
                fileName: `${base}${DEFAULT_DEBUG_SUFFIX}`,
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
