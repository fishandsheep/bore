import { readFileSync } from "node:fs";
import { join } from "node:path";

const root = new URL("../..", import.meta.url).pathname;
const cargoToml = readFileSync(join(root, "Cargo.toml"), "utf8");
const versionMatch = cargoToml.match(/^version\s*=\s*"([^"]+)"/m);

if (!versionMatch) {
  throw new Error("Could not find package version in Cargo.toml");
}

const cargoVersion = versionMatch[1];
const packageNames = [
  "bore",
  "bore-darwin-arm64",
  "bore-darwin-x64",
  "bore-linux-arm-gnueabi",
  "bore-linux-arm-musleabi",
  "bore-linux-arm64-musl",
  "bore-linux-armv7-gnueabihf",
  "bore-linux-armv7-musleabihf",
  "bore-linux-ia32-musl",
  "bore-linux-x64-musl",
  "bore-win32-ia32-msvc",
  "bore-win32-x64-msvc"
];

for (const packageDir of packageNames) {
  const packageJsonPath = join(root, "npm", packageDir, "package.json");
  const packageJson = JSON.parse(readFileSync(packageJsonPath, "utf8"));

  if (packageJson.version !== cargoVersion) {
    throw new Error(`${packageJson.name} version ${packageJson.version} does not match Cargo.toml ${cargoVersion}`);
  }

  if (packageDir === "bore") {
    for (const [name, version] of Object.entries(packageJson.optionalDependencies)) {
      if (version !== cargoVersion) {
        throw new Error(`${packageJson.name} optional dependency ${name} uses ${version}, expected ${cargoVersion}`);
      }
    }
  }
}

const tag = process.env.GITHUB_REF_NAME || process.env.TAG_NAME;
if (tag && tag.startsWith("v") && tag.slice(1) !== cargoVersion) {
  throw new Error(`Git tag ${tag} does not match Cargo.toml ${cargoVersion}`);
}

console.log(`npm package versions match Cargo.toml ${cargoVersion}`);
