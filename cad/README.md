# cad

Mechanical design files for Rudy. Imported from `OneDrive\Desktop\robot\` so
the CAD lives next to the firmware, driver, and operator console that drive
it. All binary CAD assets are tracked via **Git LFS** (see
[`.gitattributes`](../.gitattributes) at the repo root).

```
cad/
  actuators/                 third-party + reference actuator designs
    cycloidal/               printable cycloidal drive (reference design)
    openqdd/                 OpenQDD V1 (BLDC + planetary gearbox + ODrive)
    robstride/
      rs02/                  STEP + datasheet for RS02
      rs03/                  STEP + datasheet for RS03  <-- in-bench actuator
  parts/                     Rudy's own SolidWorks parts
    shoulders/               shoulder housing + cap + RS03 mount
    torso/                   torso profile + actuator mounts
    structure/               2020 aluminium extrusion + reference photos
    testing/                 jigs / fixtures
  assemblies/                top-level SolidWorks assemblies
```

## Cross-references

| CAD path | Repo concept |
|----------|--------------|
| `cad/actuators/robstride/rs03/` | [`config/actuators/robstride_rs03.yaml`](../config/actuators/robstride_rs03.yaml) (firmware spec), [`crates/cortex/src/can/`](../crates/cortex/src/can/) (driver) |
| `cad/actuators/robstride/rs02/` | RS02 reference (not currently in the inventory) |
| `cad/actuators/cycloidal/` | Reference printed actuator; not used on Rudy today |
| `cad/actuators/openqdd/` | Reference QDD design + ODrive pinouts |
| `cad/parts/shoulders/` | Shoulder links in [`ros/src/description/urdf/robot.urdf.xacro`](../ros/src/description/urdf/robot.urdf.xacro) -> exported to [`ros/src/description/meshes/shoulder_housing.glb`](../ros/src/description/meshes/) |
| `cad/parts/torso/` | Torso links in `robot.urdf.xacro` -> `ros/src/description/meshes/torso_*.glb` |
| `cad/parts/structure/` | Aluminium extrusion frame (visual reference; not yet in the URDF) |
| `cad/assemblies/torso_assembly.SLDASM` | Top-level pose alignment for the URDF torso link frames |

## Mesh export convention

The browser-side 3D viewer ([`link/src/components/viz/`](../link/src/components/viz/))
loads meshes as **glTF binary (`.glb`)** from
`ros/src/description/meshes/`. Conversion is done by
[`scripts/convert-meshes-to-glb.mjs`](../scripts/convert-meshes-to-glb.mjs)
which reads [`scripts/mesh-allowlist.json`](../scripts/mesh-allowlist.json)
and produces one `.glb` per entry.

Conventions:

- **Units.** SolidWorks parts are authored in **millimeters**; STL exports
  preserve that. URDF is **meters**. The conversion script applies a
  `0.001` scale by default; per-mesh overrides go in the allow-list.
- **Frame.** SolidWorks default frame (Y up) is rotated to URDF convention
  (Z up, X forward) via per-mesh `rpy` in the allow-list. Defaults to
  `[1.5708, 0, 0]` (90 deg about X).
- **STEP -> STL.** STEP files (`.stp`/`.step`) cannot be programmatically
  converted to STL without a CAD kernel. For the RS03 actuator and the
  2020 aluminium profile we ship a **placeholder box** in the URDF until
  an STL is exported by hand from SolidWorks (or FreeCAD's `Mesh.export`).
  Drop the resulting STL into the corresponding `cad/<area>/` folder, add
  it to the allow-list, and re-run the script.

## Regenerating meshes

```bash
node scripts/convert-meshes-to-glb.mjs
# -> writes ros/src/description/meshes/*.glb
```

The script is idempotent: it skips outputs that are newer than their source.

## Why these are imported (and MotorStudio is not)

The vendor `MotorStudio` Windows binary that originally lived alongside
this directory (~30 MB of Qt DLLs + `motor_tool.exe`) is **deliberately
not** imported. Replacing Motor Studio is the entire point of `cortex`
+ `link/` per [ADR-0004](../docs/decisions/0004-operator-console.md).
Bundling the vendor app into the repo would put it on life support and
inflate every clone. If you need it: it is still in
`OneDrive\Desktop\robot\Software\MotorStudio\`.
