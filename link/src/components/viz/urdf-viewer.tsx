// 3D viewer for `/robot/robot.urdf` (baked from
// `ros/src/description/urdf/robot.urdf.xacro` by `link/scripts/bake-urdf.mjs`).
//
// Stack:
//   - @react-three/fiber + drei (OrbitControls, Grid, Bounds) for the scene
//   - urdf-loader for kinematics; we override loadMeshCb to:
//       1. resolve each mesh URI through the IndexedDB asset cache
//          (see `@/lib/asset-cache`), keyed by sha256 from manifest.json,
//       2. hand the bytes to three's GLTFLoader.parseAsync.
//   - Joint angles flow in via the `jointStates` prop; we set them
//     imperatively on the URDFRobot to avoid a React render per CAN tick.
//
// The loader and the robot live in refs; setJointAngles bypasses React.

import { OrbitControls, Grid, Bounds } from "@react-three/drei";
import { Canvas, useThree } from "@react-three/fiber";
import { useEffect, useRef, useState } from "react";
import * as THREE from "three";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
import URDFLoader, { type URDFRobot } from "urdf-loader";
import {
  indexManifest,
  loadCachedAsset,
  loadCachedText,
  type Manifest,
} from "@/lib/asset-cache";
import { useAssetManifest } from "@/lib/use-asset-cache";
import type { JointStateMap } from "./use-joint-states";

const URDF_URL = "/robot/robot.urdf";
const MESH_BASE = "/robot/meshes/";

export interface UrdfViewerProps {
  jointStates?: JointStateMap;
  showGrid?: boolean;
  wireframe?: boolean;
  background?: string;
  /** Optional: extra class for the wrapping div (e.g. for the dashboard card). */
  className?: string;
}

export function UrdfViewer({
  jointStates,
  showGrid = true,
  wireframe = false,
  background = "transparent",
  className,
}: UrdfViewerProps) {
  const manifest = useAssetManifest();

  return (
    <div
      className={className ?? "h-full w-full"}
      style={{ background }}
    >
      <Canvas
        camera={{ position: [1.6, 1.2, 1.6], fov: 45, near: 0.05, far: 50 }}
        dpr={[1, 2]}
      >
        <ambientLight intensity={0.55} />
        <directionalLight position={[5, 8, 4]} intensity={0.9} />
        <directionalLight position={[-4, 3, -3]} intensity={0.35} />
        {showGrid && (
          <Grid
            args={[10, 10]}
            cellSize={0.1}
            sectionSize={0.5}
            cellColor="#374151"
            sectionColor="#6b7280"
            infiniteGrid
            fadeDistance={6}
            fadeStrength={2}
          />
        )}
        <Bounds fit clip observe margin={1.2}>
          <RobotScene
            jointStates={jointStates}
            wireframe={wireframe}
            manifest={manifest.data ?? null}
          />
        </Bounds>
        <OrbitControls makeDefault target={[0, 0.6, 0]} />
      </Canvas>
    </div>
  );
}

function RobotScene({
  jointStates,
  wireframe,
  manifest,
}: {
  jointStates?: JointStateMap;
  wireframe: boolean;
  manifest: Manifest | null;
}) {
  const { scene } = useThree();
  const robotRef = useRef<URDFRobot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);

  // Load the robot once. URDFLoader is mutable; we keep the URDFRobot in a
  // ref so the imperative setJointValue path doesn't re-render React.
  //
  // The dependency is `manifest` (not `manifest?.generated_at`) because a
  // fresh bake -> new manifest -> new mesh hashes should re-fetch and
  // re-parse the robot. The asset cache itself dedupes by hash so an
  // unchanged mesh is still a cache hit.
  useEffect(() => {
    let cancelled = false;
    const loader = new URDFLoader();

    // Tell the loader where to find package://-style mesh URIs after the
    // bake-urdf script has already rewritten them to /robot/meshes/...
    loader.packages = MESH_BASE;

    const byUrl = indexManifest(manifest);
    const gltfLoader = new GLTFLoader();

    // Override the mesh loader to flow through our IndexedDB cache. This
    // gets called once per <mesh filename="..."> in the URDF.
    loader.loadMeshCb = (path, _manager, onLoad) => {
      const lower = path.toLowerCase();
      if (lower.endsWith(".glb") || lower.endsWith(".gltf")) {
        // `path` is an absolute browser URL (e.g. "/robot/meshes/foo.glb"
        // on dev, or "https://host/robot/meshes/foo.glb" once urdf-loader
        // resolves it). Strip the origin so we match manifest URLs.
        const urlPath = stripOrigin(path);
        const meta = byUrl.get(urlPath);
        loadCachedAsset(path, meta?.sha256)
          .then((buf) => gltfLoader.parseAsync(buf, MESH_BASE))
          .then((gltf) => onLoad(gltf.scene))
          .catch((err) => {
            console.warn("urdf-viewer: mesh load failed", path, err);
            onLoad(new THREE.Object3D(), err as Error);
          });
        return;
      }
      // Defer to the loader's built-in path for STL/Collada; harmless until
      // the urdf starts referencing those again.
      loader.defaultMeshLoader(path, _manager, onLoad);
    };

    const urdfMeta = byUrl.get(URDF_URL);
    loadCachedText(URDF_URL, urdfMeta?.sha256)
      .then((urdf) => {
        if (cancelled) return;
        const robot = loader.parse(urdf);
        robotRef.current = robot;
        // URDF convention: z up. R3F default is y up; rotate the robot so
        // the camera angles in the wrapper still feel natural.
        robot.rotation.x = -Math.PI / 2;
        scene.add(robot);
        setLoaded(true);
      })
      .catch((e) => {
        if (cancelled) return;
        setError(e instanceof Error ? e.message : String(e));
      });

    return () => {
      cancelled = true;
      const r = robotRef.current;
      if (r) {
        scene.remove(r);
        robotRef.current = null;
      }
      setLoaded(false);
    };
  }, [scene, manifest]);

  // Imperatively apply joint angles. Skips React reconciliation on the
  // hot path; safe because URDFRobot mutates Three.js objects in place.
  useEffect(() => {
    const robot = robotRef.current;
    if (!robot || !jointStates) return;
    for (const [role, rad] of Object.entries(jointStates)) {
      // Inventory roles ↔ URDF joint names: today inventory is e.g.
      // `l_shoulder_pitch` and the URDF joint is `l_shoulder_pitch_joint`.
      // Try both spellings so either side can rename without breaking
      // the other immediately.
      const direct = robot.joints[role];
      const suffixed = robot.joints[`${role}_joint`];
      const target = direct ?? suffixed;
      if (!target) continue;
      // setJointValue is a no-op if the value is unchanged.
      target.setJointValue(rad);
    }
  }, [jointStates, loaded]);

  // Apply wireframe to all materials when toggled.
  useEffect(() => {
    const robot = robotRef.current;
    if (!robot) return;
    robot.traverse((o) => {
      if ((o as THREE.Mesh).isMesh) {
        const mesh = o as THREE.Mesh;
        const mats = Array.isArray(mesh.material) ? mesh.material : [mesh.material];
        for (const m of mats) {
          if (m && "wireframe" in m) {
            (m as THREE.MeshStandardMaterial).wireframe = wireframe;
          }
        }
      }
    });
  }, [wireframe, loaded]);

  if (error) {
    if (typeof window !== "undefined") {
      console.warn("UrdfViewer load error:", error);
    }
    return (
      <mesh>
        <boxGeometry args={[0.2, 0.2, 0.2]} />
        <meshStandardMaterial color="#7f1d1d" />
      </mesh>
    );
  }

  return null;
}

// `urdf-loader` resolves relative mesh paths against `loader.packages`,
// which gives us back the absolute URL at the call site. To look entries
// up in the manifest (which is keyed by site-root paths like
// "/robot/meshes/foo.glb") we trim the origin if present.
function stripOrigin(url: string): string {
  if (typeof window === "undefined") return url;
  const origin = window.location.origin;
  if (url.startsWith(origin)) return url.slice(origin.length);
  return url;
}
