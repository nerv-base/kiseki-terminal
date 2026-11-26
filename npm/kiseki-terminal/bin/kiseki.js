#!/usr/bin/env node
"use strict";

const { spawn } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

const PLATFORM_PACKAGES = {
  "darwin-arm64": { pkg: "kiseki-terminal-darwin-arm64", bin: "bin/kiseki" },
  "darwin-x64": { pkg: "kiseki-terminal-darwin-x64", bin: "bin/kiseki" },
  "linux-arm64": { pkg: "kiseki-terminal-linux-arm64", bin: "bin/kiseki" },
  "linux-x64": { pkg: "kiseki-terminal-linux-x64", bin: "bin/kiseki" },
  "win32-arm64": { pkg: "kiseki-terminal-win32-arm64", bin: "bin/kiseki.exe" },
  "win32-x64": { pkg: "kiseki-terminal-win32-x64", bin: "bin/kiseki.exe" },
};

function resolveBinary() {
  const override = process.env.KISEKI_BINARY_PATH;
  if (override) {
    if (!fs.existsSync(override)) {
      console.error(
        `[kiseki-terminal] KISEKI_BINARY_PATH points to a missing file: ${override}`
      );
      process.exit(1);
    }
    return override;
  }

  const key = `${process.platform}-${process.arch}`;
  const entry = PLATFORM_PACKAGES[key];
  if (!entry) {
    console.error(
      `[kiseki-terminal] Unsupported platform/architecture: ${key}.\n` +
        `Supported targets: ${Object.keys(PLATFORM_PACKAGES).join(", ")}.\n` +
        `You can still build from source with Cargo: https://github.com/nerv-base/kiseki-terminal`
    );
    process.exit(1);
  }

  const spec = `${entry.pkg}/${entry.bin}`;
  try {
    return require.resolve(spec);
  } catch {
    // Fallback check for hoisted / sibling node_modules layouts
    const candidates = [
      path.resolve(__dirname, "..", "..", entry.pkg, entry.bin),
      path.resolve(__dirname, "..", "node_modules", entry.pkg, entry.bin),
    ];
    for (const candidate of candidates) {
      if (fs.existsSync(candidate)) {
        return candidate;
      }
    }

    console.error(
      `[kiseki-terminal] Could not find the prebuilt binary package "${entry.pkg}" for ${key}.\n\n` +
        `This usually happens if optional dependencies were skipped (e.g. --no-optional / --omit=optional)\n` +
        `or if the package was installed on a different OS/architecture.\n\n` +
        `Try reinstalling with optional dependencies enabled:\n` +
        `  npm install -g kiseki-terminal\n` +
        `  bun add -g kiseki-terminal\n\n` +
        `Or install directly via Homebrew / Scoop / GitHub Releases:\n` +
        `  https://github.com/nerv-base/kiseki-terminal/releases`
    );
    process.exit(1);
  }
}

const binaryPath = resolveBinary();

const child = spawn(binaryPath, process.argv.slice(2), {
  stdio: "inherit",
  env: {
    ...process.env,
    KISEKI_INSTALL_METHOD: process.env.KISEKI_INSTALL_METHOD || "npm",
  },
});

// Let the child binary handle terminal signals cleanly (important for interactive TUI cleanup).
const forwardSignal = (signal) => {
  if (!child.killed) {
    try {
      child.kill(signal);
    } catch {
      // Child already exited.
    }
  }
};

process.on("SIGINT", () => forwardSignal("SIGINT"));
process.on("SIGTERM", () => forwardSignal("SIGTERM"));
if (process.platform !== "win32") {
  process.on("SIGHUP", () => forwardSignal("SIGHUP"));
}

child.on("error", (err) => {
  console.error(`[kiseki-terminal] Failed to launch ${binaryPath}: ${err.message}`);
  process.exit(1);
});

child.on("exit", (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
  } else {
    process.exit(code === null ? 1 : code);
  }
});
