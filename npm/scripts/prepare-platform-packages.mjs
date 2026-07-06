import { copyFileSync, chmodSync, existsSync, mkdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../..", import.meta.url));

const targets = [
  ["aarch64-apple-darwin", "bore-darwin-arm64", "bore"],
  ["x86_64-apple-darwin", "bore-darwin-x64", "bore"],
  ["arm-unknown-linux-gnueabi", "bore-linux-arm-gnueabi", "bore"],
  ["arm-unknown-linux-musleabi", "bore-linux-arm-musleabi", "bore"],
  ["aarch64-unknown-linux-musl", "bore-linux-arm64-musl", "bore"],
  ["armv7-unknown-linux-gnueabihf", "bore-linux-armv7-gnueabihf", "bore"],
  ["armv7-unknown-linux-musleabihf", "bore-linux-armv7-musleabihf", "bore"],
  ["i686-unknown-linux-musl", "bore-linux-ia32-musl", "bore"],
  ["x86_64-unknown-linux-musl", "bore-linux-x64-musl", "bore"],
  ["i686-pc-windows-msvc", "bore-win32-ia32-msvc", "bore.exe"],
  ["x86_64-pc-windows-msvc", "bore-win32-x64-msvc", "bore.exe"]
];

for (const [target, packageDir, executable] of targets) {
  const source = join(root, "target", target, "release", executable);
  if (!existsSync(source)) {
    throw new Error(`Missing release binary for ${target}: ${source}`);
  }

  const binDir = join(root, "npm", packageDir, "bin");
  rmSync(binDir, { recursive: true, force: true });
  mkdirSync(binDir, { recursive: true });

  const destination = join(binDir, executable);
  copyFileSync(source, destination);

  if (executable === "bore") {
    chmodSync(destination, 0o755);
  }
}

console.log("Prepared platform npm packages");
