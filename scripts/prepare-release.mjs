import { copyFile, mkdir, readFile, writeFile } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import path from "node:path";

const version = process.argv[2];
if (!version || !/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(version)) {
  throw new Error("Usage: node scripts/prepare-release.mjs <semantic version>");
}

const manifestPath = "Cargo.toml";
const manifest = await readFile(manifestPath, "utf8");
const lines = manifest.split(/(?<=\n)/);
let inPackageSection = false;
let versionUpdated = false;

for (let index = 0; index < lines.length; index += 1) {
  if (/^\s*\[/.test(lines[index])) {
    inPackageSection = /^\[package\]\s*$/.test(lines[index].trim());
  } else if (inPackageSection && /^version\s*=\s*"[^"]+"/.test(lines[index])) {
    lines[index] = lines[index].replace(/("version"\s*=\s*"|^version\s*=\s*")[^"]+(")/, `$1${version}$2`);
    versionUpdated = true;
    break;
  }
}

if (!versionUpdated) {
  throw new Error(`Could not find the [package] version in ${manifestPath}`);
}
await writeFile(manifestPath, lines.join(""));

const cargo = spawnSync("cargo", ["package", "--allow-dirty", "--package", "dwgpdf"], {
  stdio: "inherit",
});
if (cargo.error) throw cargo.error;
if (cargo.status !== 0) process.exit(cargo.status ?? 1);

const assetsDirectory = ".release-assets";
await mkdir(assetsDirectory, { recursive: true });
const crateArchive = path.join("target", "package", `dwgpdf-${version}.crate`);
await copyFile(crateArchive, path.join(assetsDirectory, path.basename(crateArchive)));
