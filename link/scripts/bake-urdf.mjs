#!/usr/bin/env node
// Bake the xacro URDF + meshes into link/public/robot/ for the SPA.
//
// Pipeline:
//   1. Run `xacro ../ros/src/description/urdf/robot.urdf.xacro -o public/robot/robot.urdf`.
//      If `xacro` is not on PATH (typical on Windows dev), copy the
//      checked-in fallback at scripts/baked-robot.urdf instead. The
//      fallback is regenerated and committed whenever the xacro changes
//      meaningfully; CI on Linux runs `xacro` for real.
//   2. Copy ros/src/description/meshes/*.glb to public/robot/meshes/.
//   3. Rewrite `package://description/meshes/<name>.glb` URIs in the
//      baked URDF to `/robot/meshes/<name>.glb` so the browser can fetch
//      them as static assets (no `package://` resolver needed in the SPA).
//   4. Emit public/robot/manifest.json describing every baked file with
//      its sha256, byte size, and source mtime. The browser uses sha256
//      to invalidate its IndexedDB asset cache (mtime is for humans /
//      "last updated" badges only - it doesn't survive `git checkout`).
//
// Output is git-ignored (link/public/robot/ is built artifact territory).

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, statSync } from "node:fs";
import {
  copyFile,
  mkdir,
  readdir,
  readFile,
  writeFile,
  rm,
} from "node:fs/promises";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const LINK = resolve(HERE, "..");
const REPO = resolve(LINK, "..");
const XACRO_SRC = join(REPO, "ros", "src", "description", "urdf", "robot.urdf.xacro");
const MESH_DIR = join(REPO, "ros", "src", "description", "meshes");
const OUT_DIR = join(LINK, "public", "robot");
const OUT_URDF = join(OUT_DIR, "robot.urdf");
const OUT_MESH_DIR = join(OUT_DIR, "meshes");
const FALLBACK_URDF = join(HERE, "baked-robot.urdf");
// Byte-identical to `ros/src/description/meshes/*.glb`, stored outside LFS so
// `npm run build` works when meshes are still LFS pointer files (CI budget).
const CI_LFS_FALLBACK_MESH_DIR = join(HERE, "ci-lfs-fallback", "meshes");
const OUT_MANIFEST = join(OUT_DIR, "manifest.json");

const MANIFEST_VERSION = 1;

function tryRunXacro() {
  // Try `xacro` first, then `python -m xacro` as a courtesy.
  const candidates = [
    ["xacro", [XACRO_SRC]],
    ["python", ["-m", "xacro", XACRO_SRC]],
    ["python3", ["-m", "xacro", XACRO_SRC]],
  ];
  for (const [cmd, args] of candidates) {
    const result = spawnSync(cmd, args, { encoding: "utf8" });
    if (result.error) continue;
    if (result.status === 0) {
      return { ok: true, urdf: result.stdout, via: cmd };
    }
  }
  return { ok: false };
}

// Detect Git LFS pointer files. They're tiny (<200 B) and start with a
// fixed magic line. We refuse to bake these because they'd ship as the
// "asset" and the GLTFLoader in the operator console would try to
// JSON.parse `version https://git-lfs.github.com/spec/v1\n...` and
// throw `Unexpected token 'v', "version ht"... is not valid JSON`.
//
// This usually means LFS isn't installed locally, or the file was
// checked out without `git lfs pull` (e.g. CI without `lfs: true`).
const LFS_POINTER_PREFIX = "version https://git-lfs.github.com/spec/";

async function isLfsPointer(absPath) {
  // Real GLBs start with the 4-byte magic "glTF"; LFS pointers are tiny
  // text. Read just enough to disambiguate without slurping the whole
  // mesh.
  const head = await readFile(absPath, { encoding: "utf8", flag: "r" }).catch(
    () => null,
  );
  if (head === null) return false;
  return head.startsWith(LFS_POINTER_PREFIX);
}

async function copyMeshes() {
  if (!existsSync(MESH_DIR)) {
    console.warn(`bake-urdf: no mesh dir at ${MESH_DIR}; skipping mesh copy`);
    return [];
  }
  await mkdir(OUT_MESH_DIR, { recursive: true });
  const entries = (await readdir(MESH_DIR)).filter(
    (n) => n.endsWith(".glb") || n.endsWith(".gltf"),
  );
  const pointers = [];
  for (const name of entries) {
    const src = join(MESH_DIR, name);
    const dst = join(OUT_MESH_DIR, name);
    if (await isLfsPointer(src)) {
      const fallback = join(CI_LFS_FALLBACK_MESH_DIR, name);
      if (existsSync(fallback) && !(await isLfsPointer(fallback))) {
        console.warn(
          `bake-urdf: ${src} is an LFS pointer; using plain-git fallback ${fallback}`,
        );
        await copyFile(fallback, dst);
        continue;
      }
      pointers.push(name);
      continue;
    }
    await copyFile(src, dst);
  }
  if (pointers.length > 0) {
    console.error(
      `bake-urdf: ${pointers.length} mesh(es) are unresolved Git LFS pointers:\n` +
        pointers.map((n) => `  - ${join(MESH_DIR, n)}`).join("\n") +
        "\n  Install Git LFS and run `git lfs pull` in the repo root, or add a matching file under\n" +
        `  ${CI_LFS_FALLBACK_MESH_DIR}/ (see .gitattributes). Refusing to bake placeholder bytes.`,
    );
    process.exit(1);
  }
  return entries;
}

function rewriteMeshUris(urdf) {
  // package://description/meshes/<name>.glb  ->  /robot/meshes/<name>.glb
  return urdf.replace(
    /package:\/\/description\/meshes\/([^"']+)/g,
    "/robot/meshes/$1",
  );
}

// Build a manifest entry for one baked output file. `url` is the path the
// browser uses to fetch this asset; `srcPath` is the upstream file we use
// for the human-readable mtime (falls back to the baked file's own mtime).
async function manifestEntry({ absPath, url, srcPath }) {
  const bytes = await readFile(absPath);
  const sha256 = createHash("sha256").update(bytes).digest("hex");
  let mtimeMs = null;
  try {
    mtimeMs = Math.floor(statSync(srcPath ?? absPath).mtimeMs);
  } catch {
    // Best-effort - manifest still useful without mtime.
  }
  return {
    url,
    sha256,
    bytes: bytes.length,
    mtime_ms: mtimeMs,
  };
}

async function writeManifest({ urdfVia, meshNames }) {
  const entries = [];
  // URDF: source-of-truth mtime is the xacro (or the fallback URDF).
  entries.push(
    await manifestEntry({
      absPath: OUT_URDF,
      url: "/robot/robot.urdf",
      srcPath: existsSync(XACRO_SRC) ? XACRO_SRC : FALLBACK_URDF,
    }),
  );
  for (const name of meshNames) {
    const absPath = join(OUT_MESH_DIR, name);
    const srcPath = join(MESH_DIR, name);
    entries.push(
      await manifestEntry({
        absPath,
        url: `/robot/meshes/${name}`,
        srcPath,
      }),
    );
  }

  const manifest = {
    version: MANIFEST_VERSION,
    generated_at: new Date().toISOString(),
    via: urdfVia,
    entries,
  };
  await writeFile(OUT_MANIFEST, JSON.stringify(manifest, null, 2) + "\n");
  return manifest;
}

async function main() {
  await rm(OUT_DIR, { recursive: true, force: true });
  await mkdir(OUT_DIR, { recursive: true });

  let urdf;
  let via;
  const xacro = tryRunXacro();
  if (xacro.ok) {
    urdf = xacro.urdf;
    via = `xacro (${xacro.via})`;
  } else if (existsSync(FALLBACK_URDF)) {
    urdf = await readFile(FALLBACK_URDF, "utf8");
    via = "checked-in baked-robot.urdf fallback";
    console.warn(
      "bake-urdf: `xacro` not on PATH; using committed snapshot " +
        `${FALLBACK_URDF}.\n` +
        "  Run `pip install xacro` (or in a ROS env: `apt install ros-${ROS_DISTRO}-xacro`)\n" +
        "  to bake from source instead.",
    );
  } else {
    console.error(
      `bake-urdf: \`xacro\` not on PATH and no fallback at ${FALLBACK_URDF}.\n` +
        "  Either install xacro or commit a baked snapshot.",
    );
    process.exit(1);
  }

  urdf = rewriteMeshUris(urdf);
  await writeFile(OUT_URDF, urdf);

  const meshes = await copyMeshes();

  const manifest = await writeManifest({ urdfVia: via, meshNames: meshes });

  console.log(
    `bake-urdf: wrote ${relative(LINK, OUT_URDF)} via ${via}; ` +
      `${meshes.length} mesh(es); manifest has ${manifest.entries.length} entries.`,
  );
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
