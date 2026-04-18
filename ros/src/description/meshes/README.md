# meshes

glTF binary (`.glb`) meshes referenced from
[`../urdf/robot.urdf.xacro`](../urdf/robot.urdf.xacro) via
`<mesh filename="package://description/meshes/<name>.glb"/>`.

## Regeneration

These files are **outputs**. Do not hand-edit. To regenerate:

```bash
node scripts/convert-meshes-to-glb.mjs   # from repo root
```

The script reads
[`../../../../scripts/mesh-allowlist.json`](../../../../scripts/mesh-allowlist.json),
which maps source CAD/STL files in [`../../../../cad/`](../../../../cad/) to
output names + scale + RPY transforms.

## Why glTF binary

- Single-request load (no separate manifest + asset round-trips).
- 30-60% smaller than equivalent ASCII STL.
- Browser-native via Three.js `GLTFLoader`; no STL-decoder shipped to the
  client.

## Conventions

- Units: meters (URDF). The conversion script applies a `0.001` scale by
  default for SolidWorks-mm STL inputs.
- Frame: URDF (X forward, Y left, Z up). Per-mesh `rpy` overrides sit in
  the allow-list when needed.
- Tracked under Git LFS (see [`../../../../.gitattributes`](../../../../.gitattributes)).
