import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { createServer } from "node:http";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const packageDir = path.resolve(__dirname, "..");
const tmpRoot = path.join(packageDir, ".tmp");

const defaultCases = [
  "vite5-dev",
  "vite6-dev",
  "vite7-dev",
  "vite8-dev",
  "rolldown-vite-dev",
  "webpack5-dev",
  "rsbuild2-dev",
  "rspack2-dev",
  "parcel2-dev",
  "next16-turbopack-dev",
  "next16-webpack-dev",
  "vite5-web-mirror-dev",
];

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

async function waitForHttp(url, child, logs, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  let lastError;

  while (Date.now() < deadline) {
    if (child.exitCode != null) {
      throw new Error(`vite exited early:\n${logs.join("")}`);
    }

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

function devCommand(name, port) {
  const host = "127.0.0.1";
  if (name.startsWith("vite") || name.startsWith("rolldown-vite")) {
    return ["pnpm", "exec", "vite", "--host", host, "--port", String(port), "--strictPort"];
  }
  if (name.startsWith("webpack5")) {
    return [
      "pnpm",
      "exec",
      "webpack",
      "serve",
      "--config",
      "webpack.config.cjs",
      "--host",
      host,
      "--port",
      String(port),
    ];
  }
  if (name.startsWith("rsbuild2")) {
    return ["pnpm", "exec", "rsbuild", "dev", "--host", host, "--port", String(port)];
  }
  if (name.startsWith("rspack2")) {
    return [
      "pnpm",
      "exec",
      "rspack",
      "serve",
      "--config",
      "rspack.config.cjs",
      "--host",
      host,
      "--port",
      String(port),
    ];
  }
  if (name.startsWith("parcel2")) {
    return [
      "pnpm",
      "exec",
      "parcel",
      "index.html",
      "--host",
      host,
      "--port",
      String(port),
      "--no-cache",
    ];
  }
  if (name === "next16-webpack-dev") {
    return ["pnpm", "exec", "next", "dev", "--webpack", "-H", host, "-p", String(port)];
  }
  if (name.startsWith("next16")) {
    return ["pnpm", "exec", "next", "dev", "-H", host, "-p", String(port)];
  }

  throw new Error(`No dev command configured for ${name}`);
}

function startDevServer(name, caseDir, port) {
  const [bin, ...args] = devCommand(name, port);
  const child = spawn(bin, args, {
    cwd: caseDir,
    stdio: ["ignore", "pipe", "pipe"],
    env: {
      ...process.env,
      FORCE_COLOR: "0",
      NEXT_TELEMETRY_DISABLED: "1",
    },
  });
  const logs = [];
  child.stdout.on("data", (chunk) => logs.push(chunk.toString()));
  child.stderr.on("data", (chunk) => logs.push(chunk.toString()));
  child.once("exit", (code, signal) => {
    logs.push(`\n[dev server exited code=${code} signal=${signal}]\n`);
  });

  return { child, logs };
}

async function verifyPage(browser, name, url) {
  const page = await browser.newPage();
  const pageErrors = [];
  const consoleErrors = [];

  page.on("pageerror", (error) => {
    pageErrors.push({
      name: error.name,
      message: error.message,
      stack: error.stack,
      value: String(error),
    });
  });
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
      () => {
        const value = globalThis.__LORO_JSON_SMOKE__;
        return value?.t === "hi" && Object.keys(value).length === 1;
      },
      null,
      { timeout: 10_000 },
    );
    await page.waitForTimeout(250);

    if (pageErrors.length || consoleErrors.length) {
      throw new Error(
        `${name}: browser errors\npageErrors=${JSON.stringify(
          pageErrors,
          null,
          2,
        )}\nconsoleErrors=${JSON.stringify(consoleErrors, null, 2)}`,
      );
    }

    const jsonSmokeValue = await page.evaluate(
      () => globalThis.__LORO_JSON_SMOKE__ ?? null,
    );
    if (JSON.stringify(jsonSmokeValue) !== JSON.stringify({ t: "hi" })) {
      throw new Error(
        `${name}: unexpected JSON smoke value ${JSON.stringify(jsonSmokeValue)}`,
      );
    }

    return { jsonSmokeValue };
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
      `Missing generated case ${name}. Run \`node ./scripts/run.mjs ${name}\` first.`,
    );
  }

  const port = await findFreePort(45200 + index * 10);
  const url = `http://127.0.0.1:${port}/`;
  const server = startDevServer(name, caseDir, port);
  try {
    await waitForHttp(url, server.child, server.logs, 60_000);
    const result = await verifyPage(browser, name, url);
    return { name, ...result };
  } catch (error) {
    throw new Error(`${error.message}\ndevServerLogs=${server.logs.join("")}`);
  } finally {
    if (server.child.exitCode == null) {
      server.child.kill("SIGTERM");
      await new Promise((resolve) => server.child.once("exit", resolve));
    }
  }
}

async function main() {
  const selected = process.argv.slice(2);
  const cases = selected.length > 0 ? selected : defaultCases;
  const unknown = cases.filter((name) => !defaultCases.includes(name));
  if (unknown.length > 0) {
    throw new Error(`Unknown dev runtime case(s): ${unknown.join(", ")}`);
  }

  const browser = await chromium.launch();
  const results = [];

  try {
    for (const [index, name] of cases.entries()) {
      const result = await runCase(browser, name, index);
      results.push(result);
      console.log(`ok ${result.name} json=${JSON.stringify(result.jsonSmokeValue)}`);
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
