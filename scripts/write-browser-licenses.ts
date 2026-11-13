import { execFileSync } from "node:child_process";
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

type PackageLicense = { name: string; license: string; paths: string[] };

function parsePackageLicenses(value: unknown): PackageLicense[] {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error("Unexpected pnpm license report");
  }
  const packages: PackageLicense[] = [];
  for (const group of Object.values(value)) {
    if (!Array.isArray(group)) throw new Error("Unexpected pnpm license group");
    for (const item of group) {
      if (
        typeof item !== "object" ||
        item === null ||
        typeof item.name !== "string" ||
        typeof item.license !== "string" ||
        !Array.isArray(item.paths) ||
        item.paths.length === 0 ||
        !item.paths.every((path: unknown) => typeof path === "string")
      ) {
        throw new Error("Unexpected pnpm license entry");
      }
      packages.push({
        name: item.name,
        license: item.license,
        paths: item.paths,
      });
    }
  }
  if (packages.length === 0) throw new Error("Empty pnpm license report");
  return packages;
}

const root = resolve(import.meta.dirname, "..");
const report: unknown = JSON.parse(
  execFileSync("pnpm", ["licenses", "list", "--prod", "--json"], {
    cwd: root,
    encoding: "utf8",
  }),
);
const notices = [
  `LiveKit camera tools\n\n${readFileSync(join(root, "LICENSE"), "utf8").trim()}\n\n` +
    "Third-party licenses for the browser camera's production dependencies.\n" +
    "This inventory may include packages removed by bundler tree-shaking.",
];

for (const entry of parsePackageLicenses(report).sort((a, b) =>
  a.name.localeCompare(b.name),
)) {
  for (const packagePath of entry.paths) {
    const manifest: unknown = JSON.parse(
      readFileSync(join(packagePath, "package.json"), "utf8"),
    );
    if (
      typeof manifest !== "object" ||
      manifest === null ||
      !("version" in manifest) ||
      typeof manifest.version !== "string"
    ) {
      throw new Error(`Missing package version: ${entry.name}`);
    }
    let licenseDirectory = packagePath;
    let licenseFiles = readdirSync(licenseDirectory).filter((name) =>
      /^(license|licence|notice|copying)(\.|-|$)/i.test(name),
    );
    if (
      licenseFiles.length === 0 &&
      entry.name === "@bufbuild/protobuf" &&
      manifest.version === "1.10.1"
    ) {
      licenseDirectory = join(
        root,
        "licenses/browser/bufbuild-protobuf-1.10.1",
      );
      licenseFiles = ["LICENSE-APACHE", "LICENSE-GOOGLE-BSD"];
    }
    if (licenseFiles.length === 0) {
      throw new Error(
        `No license or notice file for ${entry.name}@${manifest.version}`,
      );
    }
    notices.push(
      `\n${"=".repeat(72)}\n${entry.name}@${manifest.version} (${entry.license})`,
      ...licenseFiles
        .sort()
        .map(
          (file) =>
            `\n--- ${file} ---\n${readFileSync(join(licenseDirectory, file), "utf8").trim()}`,
        ),
    );
  }
}

writeFileSync(
  join(root, "dist/third-party-licenses.txt"),
  `${notices.join("\n")}\n`,
);
