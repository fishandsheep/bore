import { chmodSync, copyFileSync, existsSync, mkdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../..", import.meta.url));
const assetDir = process.argv[2] || join(root, "npm-dist");
const tag = process.env.GITHUB_REF_NAME || process.env.TAG_NAME;

if (!tag) {
  throw new Error("Set GITHUB_REF_NAME or TAG_NAME to the release tag");
}

const assets = [
  ["aarch64-apple-darwin", "bore-darwin-arm64", "tar.gz", "bore"],
  ["x86_64-apple-darwin", "bore-darwin-x64", "tar.gz", "bore"],
  ["arm-unknown-linux-gnueabi", "bore-linux-arm-gnueabi", "tar.gz", "bore"],
  ["arm-unknown-linux-musleabi", "bore-linux-arm-musleabi", "tar.gz", "bore"],
  ["aarch64-unknown-linux-musl", "bore-linux-arm64-musl", "tar.gz", "bore"],
  ["armv7-unknown-linux-gnueabihf", "bore-linux-armv7-gnueabihf", "tar.gz", "bore"],
  ["armv7-unknown-linux-musleabihf", "bore-linux-armv7-musleabihf", "tar.gz", "bore"],
  ["i686-unknown-linux-musl", "bore-linux-ia32-musl", "tar.gz", "bore"],
  ["x86_64-unknown-linux-musl", "bore-linux-x64-musl", "tar.gz", "bore"],
  ["i686-pc-windows-msvc", "bore-win32-ia32-msvc", "zip", "bore.exe"],
  ["x86_64-pc-windows-msvc", "bore-win32-x64-msvc", "zip", "bore.exe"]
];

function run(command, args) {
  const result = spawnSync(command, args, { stdio: "inherit" });
  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(" ")} failed`);
  }
}

for (const [target, packageDir, extension, executable] of assets) {
  const asset = join(assetDir, `bore-${tag}-${target}.${extension}`);
  if (!existsSync(asset)) {
    throw new Error(`Missing release asset: ${asset}`);
  }

  const extractDir = join(tmpdir(), `bore-npm-${target}`);
  rmSync(extractDir, { recursive: true, force: true });
  mkdirSync(extractDir, { recursive: true });

  if (extension === "zip") {
    run("unzip", ["-q", asset, "-d", extractDir]);
  } else {
    run("tar", ["-xzf", asset, "-C", extractDir]);
  }

  const source = join(extractDir, executable);
  if (!existsSync(source)) {
    throw new Error(`Missing binary in ${asset}: ${executable}`);
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

console.log(`Prepared npm platform packages from ${assetDir}`);
