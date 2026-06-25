#!/usr/bin/env node

const { spawnSync } = require("child_process");
const fs = require("fs");
const path = require("path");

const packages = {
  "darwin arm64": "@qinshower/bore-darwin-arm64",
  "darwin x64": "@qinshower/bore-darwin-x64",
  "linux arm": {
    6: "@qinshower/bore-linux-arm-gnueabi",
    7: "@qinshower/bore-linux-armv7-gnueabihf",
    default: "@qinshower/bore-linux-arm-gnueabi"
  },
  "linux arm64": "@qinshower/bore-linux-arm64-musl",
  "linux ia32": "@qinshower/bore-linux-ia32-musl",
  "linux x64": "@qinshower/bore-linux-x64-musl",
  "win32 ia32": "@qinshower/bore-win32-ia32-msvc",
  "win32 x64": "@qinshower/bore-win32-x64-msvc"
};

function selectedPackage() {
  const key = `${process.platform} ${process.arch}`;
  const value = packages[key];

  if (!value) {
    return null;
  }

  if (typeof value === "string") {
    return value;
  }

  const armVersion = Number(process.config && process.config.variables && process.config.variables.arm_version);
  return value[armVersion] || value.default;
}

function packageBinary(packageName) {
  let packageJson;
  try {
    packageJson = require.resolve(`${packageName}/package.json`);
  } catch (error) {
    const localPackage = path.join(__dirname, "..", "..", packageName.replace("@qinshower/", ""), "package.json");
    if (!fs.existsSync(localPackage)) {
      throw error;
    }
    packageJson = localPackage;
  }
  const extension = process.platform === "win32" ? ".exe" : "";
  return path.join(path.dirname(packageJson), "bin", `bore${extension}`);
}

const packageName = selectedPackage();

if (!packageName) {
  console.error(`Unsupported platform for @qinshower/bore: ${process.platform}/${process.arch}`);
  console.error(`Supported platforms: ${Object.keys(packages).join(", ")}`);
  process.exit(1);
}

let binary;

try {
  binary = packageBinary(packageName);
} catch (error) {
  console.error(`Missing optional dependency ${packageName}.`);
  console.error("Install @qinshower/bore with optional dependencies enabled, or run npm install again.");
  process.exit(1);
}

if (!fs.existsSync(binary)) {
  console.error(`Missing bore binary at ${binary}`);
  process.exit(1);
}

const result = spawnSync(binary, process.argv.slice(2), { stdio: "inherit" });

if (result.error) {
  console.error(result.error.message);
  process.exit(1);
}

if (result.signal) {
  process.kill(process.pid, result.signal);
} else {
  process.exit(result.status === null ? 1 : result.status);
}
