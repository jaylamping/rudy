#!/usr/bin/env node
// Convert STL meshes from cad/ to glTF binary (.glb) under
// ros/src/description/meshes/, driven by scripts/mesh-allowlist.json.
//
// Why this script:
// - Three.js's `URDFLoader` and the browser-side viewer load .glb in one
//   request and parse it natively; STL is parseable but ~30-60% larger.
// - Each entry in the allow-list pins a per-mesh `scale` (SolidWorks parts
//   are mm; URDF is meters) and `rpy` (rotate to URDF axes).
// - Idempotent: skips outputs whose mtime is newer than the source.
//
// Run: node scripts/convert-meshes-to-glb.mjs
//
// Reads three's loaders from `link/node_modules/three` so we don't need a
// separate install; if `link/node_modules/three` is missing, prints a hint.

import { readFile, writeFile, mkdir, stat } from "node:fs/promises";
import { existsSync } from "node:fs";
import { resolve, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

// Three's GLTFExporter targets the browser; in `binary: true` mode it calls
// `new FileReader()` on a Blob to materialise the binary chunk. We polyfill
// just enough of those two globals (Blob already exists in modern Node) so
// the exporter stays the same code path the browser uses.
if (typeof globalThis.FileReader === "undefined") {
  globalThis.FileReader = class FileReader {
    constructor() {
      this.result = null;
      this.onload = null;
      this.onloadend = null;
      this.onerror = null;
    }
    readAsArrayBuffer(blob) {
      blob
        .arrayBuffer()
        .then((buf) => {
          this.result = buf;
          if (typeof this.onload === "function") this.onload({ target: this });
          if (typeof this.onloadend === "function")
            this.onloadend({ target: this });
        })
        .catch((err) => {
          if (typeof this.onerror === "function") this.onerror(err);
        });
    }
    readAsDataURL(blob) {
      blob
        .arrayBuffer()
        .then((buf) => {
          const b64 = Buffer.from(buf).toString("base64");
          this.result = `data:${blob.type || "application/octet-stream"};base64,${b64}`;
          if (typeof this.onload === "function") this.onload({ target: this });
          if (typeof this.onloadend === "function")
            this.onloadend({ target: this });
        })
        .catch((err) => {
          if (typeof this.onerror === "function") this.onerror(err);
        });
    }
  };
}

const ROOT = resolve(fileURLToPath(import.meta.url), "..", "..");
const ALLOWLIST = join(ROOT, "scripts", "mesh-allowlist.json");

async function loadThree() {
  const candidate = join(ROOT, "link", "node_modules", "three");
  if (!existsSync(candidate)) {
    console.error(
      `error: three.js not found at ${candidate}.\n` +
        `  Run \`cd link && npm install\` first; this script reuses link/'s three install.`,
    );
    process.exit(1);
  }
  const threeUrl = pathToFileURL(join(candidate, "build", "three.module.js")).href;
  const stlUrl = pathToFileURL(
    join(candidate, "examples", "jsm", "loaders", "STLLoader.js"),
  ).href;
  const exporterUrl = pathToFileURL(
    join(candidate, "examples", "jsm", "exporters", "GLTFExporter.js"),
  ).href;
  const THREE = await import(threeUrl);
  const { STLLoader } = await import(stlUrl);
  const { GLTFExporter } = await import(exporterUrl);
  return { THREE, STLLoader, GLTFExporter };
}

function isUpToDate(srcMtime, outPath) {
  if (!existsSync(outPath)) return false;
  return stat(outPath).then((st) => st.mtimeMs >= srcMtime);
}

async function convertOne(THREE, STLLoader, GLTFExporter, entry, outDir) {
  const srcAbs = join(ROOT, entry.source);
  if (!existsSync(srcAbs)) {
    console.warn(`skip ${entry.name}: source missing (${entry.source})`);
    return { name: entry.name, status: "missing" };
  }
  const outAbs = join(outDir, `${entry.name}.glb`);
  const srcStat = await stat(srcAbs);
  if (await isUpToDate(srcStat.mtimeMs, outAbs)) {
    return { name: entry.name, status: "skip" };
  }

  const buf = await readFile(srcAbs);
  // STLLoader.parse takes an ArrayBuffer; Buffer.buffer would include the
  // whole pool, so slice to the exact range.
  const ab = buf.buffer.slice(buf.byteOffset, buf.byteOffset + buf.byteLength);
  const loader = new STLLoader();
  const geometry = loader.parse(ab);
  geometry.computeVertexNormals();

  const material = new THREE.MeshStandardMaterial({
    color: 0xb0b0b8,
    metalness: 0.1,
    roughness: 0.7,
  });
  const mesh = new THREE.Mesh(geometry, material);
  mesh.scale.setScalar(entry.scale ?? 0.001);
  const [rx, ry, rz] = entry.rpy ?? [0, 0, 0];
  mesh.rotation.set(rx, ry, rz);

  const scene = new THREE.Scene();
  scene.add(mesh);

  const exporter = new GLTFExporter();
  const gltfBin = await exporter.parseAsync(scene, { binary: true });

  await mkdir(outDir, { recursive: true });
  await writeFile(outAbs, Buffer.from(gltfBin));
  return {
    name: entry.name,
    status: "wrote",
    bytes: gltfBin.byteLength,
    triangles: geometry.attributes.position.count / 3,
  };
}

async function main() {
  const allowlist = JSON.parse(await readFile(ALLOWLIST, "utf8"));
  const outDir = resolve(ROOT, allowlist.outputDir);
  await mkdir(outDir, { recursive: true });

  const { THREE, STLLoader, GLTFExporter } = await loadThree();

  const results = [];
  for (const entry of allowlist.meshes) {
    try {
      const r = await convertOne(THREE, STLLoader, GLTFExporter, entry, outDir);
      results.push(r);
    } catch (err) {
      console.error(`fail ${entry.name}: ${err.message}`);
      results.push({ name: entry.name, status: "fail", error: err.message });
    }
  }

  const wrote = results.filter((r) => r.status === "wrote");
  const skipped = results.filter((r) => r.status === "skip");
  const missing = results.filter((r) => r.status === "missing");
  const failed = results.filter((r) => r.status === "fail");

  for (const r of wrote) {
    console.log(
      `wrote ${r.name}.glb  ${(r.bytes / 1024).toFixed(1)} KiB  ` +
        `${Math.round(r.triangles).toLocaleString()} tri`,
    );
  }
  for (const r of skipped) console.log(`skip  ${r.name}.glb (up to date)`);
  for (const r of missing) console.log(`miss  ${r.name} (source not found)`);
  for (const r of failed) console.log(`fail  ${r.name}: ${r.error}`);

  console.log(
    `\n${wrote.length} written, ${skipped.length} up-to-date, ` +
      `${missing.length} missing, ${failed.length} failed`,
  );
  if (failed.length) process.exit(1);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
