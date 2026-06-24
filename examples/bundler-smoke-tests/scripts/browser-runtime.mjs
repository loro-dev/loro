import { spawn } from "node:child_process";
import { createReadStream, existsSync } from "node:fs";
import { readFile, readdir, stat } from "node:fs/promises";
import { createServer } from "node:http";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const packageDir = path.resolve(__dirname, "..");
const tmpRoot = path.join(packageDir, ".tmp");
const expectedSmokeJson = { map: { text: "mergeable-smoke" } };
const expectedSmokeJsonLiteral = JSON.stringify(expectedSmokeJson);

const defaultCases = [
  "vite5",
  "vite6",
  "vite7",
  "vite8",
  "rolldown-vite",
  "webpack5",
  "rsbuild2",
  "rspack2",
  "esbuild-default-copy",
  "esbuild-base64",
  "rollup-default-copy",
  "rollup-base64",
  "parcel2",
  "next16-turbopack",
  "next16-webpack",
];

const mimeTypes = new Map([
  [".html", "text/html; charset=utf-8"],
  [".js", "text/javascript; charset=utf-8"],
  [".mjs", "text/javascript; charset=utf-8"],
  [".css", "text/css; charset=utf-8"],
  [".json", "application/json; charset=utf-8"],
  [".map", "application/json; charset=utf-8"],
  [".wasm", "application/wasm"],
]);

function findFreePort(start) {
  return new Promise((resolve, reject) => {
    const server = createServer();
    server.once("error", (error) => {
      if (error.code === "EADDRINUSE") {
        findFreePort(start + 1).then(resolve, reject);
      } else {
        reject(error);
      }
    });
    server.listen(start, "127.0.0.1", () => {
      const { port } = server.address();
      server.close(() => resolve(port));
    });
  });
}

async function findEntrypoint(distDir) {
  const preferred = ["index.html", "bundle.js", "main.js", "index.js"];
  for (const name of preferred) {
    const file = path.join(distDir, name);
    if (existsSync(file)) {
      return name;
    }
  }

  const entries = await readdir(distDir);
  const js = entries.find((entry) => entry.endsWith(".js"));
  if (js) {
    return js;
  }

  throw new Error(`No browser entrypoint found in ${distDir}`);
}

function startStaticServer(distDir, port, entrypoint) {
  const server = createServer(async (request, response) => {
    try {
      const url = new URL(request.url ?? "/", `http://127.0.0.1:${port}`);
      let pathname = decodeURIComponent(url.pathname);

      if (pathname === "/favicon.ico") {
        response.writeHead(204);
        response.end();
        return;
      }

      if (pathname === "/") {
        if (existsSync(path.join(distDir, "index.html"))) {
          pathname = "/index.html";
        } else {
          const isModule = entrypoint !== "bundle.js";
          response.writeHead(200, {
            "content-type": "text/html; charset=utf-8",
          });
          response.end(
            `<!doctype html><meta charset="utf-8"><div id="app"></div><script ${
              isModule ? 'type="module" ' : ""
            }src="/${entrypoint}"></script>`,
          );
          return;
        }
      }

      const target = path.normalize(path.join(distDir, pathname));
      if (!target.startsWith(distDir + path.sep) && target !== distDir) {
        response.writeHead(403);
        response.end("Forbidden");
        return;
      }

      if (!existsSync(target) || !(await stat(target)).isFile()) {
        response.writeHead(404);
        response.end("Not found");
        return;
      }

      const contentType =
        mimeTypes.get(path.extname(target)) ?? "application/octet-stream";
      response.writeHead(200, { "content-type": contentType });

      if (path.basename(target) === "index.html") {
        const html = await readFile(target, "utf8");
        response.end(
          html.replace(
            /<script\s+type="module"\s+src="\/src\/main\.js"><\/script>\s*/g,
            "",
          ),
        );
        return;
      }

      createReadStream(target).pipe(response);
    } catch (error) {
      response.writeHead(500);
      response.end(String(error.stack ?? error));
    }
  });

  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(port, "127.0.0.1", () => resolve(server));
  });
}

async function waitForHttp(url, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  let lastError;

  while (Date.now() < deadline) {
    try {
      const response = await fetch(url);
      if (response.status < 500) {
        return;
      }
      lastError = new Error(`HTTP ${response.status}`);
    } catch (error) {
      lastError = error;
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }

  throw lastError ?? new Error(`Timed out waiting for ${url}`);
}

async function startNext(caseDir, port) {
  const child = spawn(
    "pnpm",
    ["exec", "next", "start", "-H", "127.0.0.1", "-p", String(port)],
    {
      cwd: caseDir,
      stdio: ["ignore", "pipe", "pipe"],
      env: { ...process.env, NEXT_TELEMETRY_DISABLED: "1" },
    },
  );

  const logs = [];
  const collect = (chunk) => logs.push(chunk.toString());
  child.stdout.on("data", collect);
  child.stderr.on("data", collect);

  let exited = false;
  child.once("exit", (code, signal) => {
    exited = true;
    logs.push(`\n[next exited code=${code} signal=${signal}]\n`);
  });

  await waitForHttp(`http://127.0.0.1:${port}/`, 60_000).catch((error) => {
    if (exited) {
      throw new Error(`next start exited early:\n${logs.join("")}`);
    }
    throw new Error(`${error.message}\n${logs.join("")}`);
  });

  return {
    stop: async () => {
      if (!exited) {
        child.kill("SIGTERM");
        await new Promise((resolve) => child.once("exit", resolve));
      }
    },
  };
}

async function verifyPage(browser, name, url) {
  const page = await browser.newPage();
  const pageErrors = [];
  const consoleErrors = [];

  page.on("pageerror", (error) =>
    pageErrors.push(error.stack ?? error.message),
  );
  page.on("console", (message) => {
    if (message.type() === "error") {
      consoleErrors.push(message.text());
    }
  });

  try {
    const response = await page.goto(url, {
      waitUntil: "domcontentloaded",
      timeout: 60_000,
    });
    if (!response || !response.ok()) {
      throw new Error(
        `${name}: navigation failed with ${response?.status() ?? "no response"}`,
      );
    }

    await page.waitForFunction(
      (expected) => {
        const value = globalThis.__LORO_JSON_SMOKE__;
        return JSON.stringify(value) === JSON.stringify(expected);
      },
      expectedSmokeJson,
      { timeout: 60_000 },
    );
    await page.waitForTimeout(250);

    const bodyText = (await page.locator("body").innerText())
      .trim()
      .replace(/\s+/g, " ");
    const jsonSmokeValue = await page.evaluate(
      () => globalThis.__LORO_JSON_SMOKE__ ?? null,
    );

    if (pageErrors.length || consoleErrors.length) {
      throw new Error(
        `${name}: browser errors\npageErrors=${JSON.stringify(
          pageErrors,
          null,
          2,
        )}\nconsoleErrors=${JSON.stringify(consoleErrors, null, 2)}`,
      );
    }

    if (JSON.stringify(jsonSmokeValue) !== expectedSmokeJsonLiteral) {
      throw new Error(
        `${name}: unexpected JSON smoke value ${JSON.stringify(jsonSmokeValue)}`,
      );
    }

    if (!bodyText.includes(expectedSmokeJsonLiteral)) {
      throw new Error(
        `${name}: expected body to include ${expectedSmokeJsonLiteral}, got ${JSON.stringify(bodyText)}`,
      );
    }

    return { bodyText, jsonSmokeValue };
  } catch (error) {
    const bodyText = await page
      .locator("body")
      .innerText()
      .catch(() => "");
    throw new Error(
      `${name}: ${error.message}\nbody=${JSON.stringify(
        bodyText.trim().replace(/\s+/g, " "),
      )}\npageErrors=${JSON.stringify(
        pageErrors,
        null,
        2,
      )}\nconsoleErrors=${JSON.stringify(consoleErrors, null, 2)}`,
    );
  } finally {
    await page.close();
  }
}

async function runCase(browser, name, index) {
  const caseDir = path.join(tmpRoot, name);
  if (!existsSync(caseDir)) {
    throw new Error(
      `Missing generated case ${name}. Run \`pnpm --dir examples/bundler-smoke-tests run test:browser\` from the repo root.`,
    );
  }

  const port = await findFreePort(43100 + index * 10);
  const url = `http://127.0.0.1:${port}/`;

  if (name.startsWith("next")) {
    const server = await startNext(caseDir, port);
    try {
      const result = await verifyPage(browser, name, url);
      return { name, mode: "next start", ...result };
    } finally {
      await server.stop();
    }
  }

  const distDir = path.join(caseDir, "dist");
  const entrypoint = await findEntrypoint(distDir);
  const server = await startStaticServer(distDir, port, entrypoint);
  try {
    const result = await verifyPage(browser, name, url);
    return { name, mode: `static ${entrypoint}`, ...result };
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
}

async function main() {
  const selected = process.argv.slice(2);
  const cases = selected.length > 0 ? selected : defaultCases;
  const unknown = cases.filter((name) => !defaultCases.includes(name));
  if (unknown.length > 0) {
    throw new Error(`Unknown browser case(s): ${unknown.join(", ")}`);
  }

  const browser = await chromium.launch();
  const results = [];

  try {
    for (const [index, name] of cases.entries()) {
      const result = await runCase(browser, name, index);
      results.push(result);
      console.log(
        `ok ${result.name.padEnd(20)} ${result.mode.padEnd(
          18,
        )} body=${JSON.stringify(result.bodyText)}`,
      );
    }
  } finally {
    await browser.close();
  }

  console.log(JSON.stringify(results, null, 2));
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
