# Migrating inventory.yaml from v1 to v2

The daemon requires `schema_version: 2` and a top-level `devices:` list. Version 1 files (`motors:` only) are rejected at startup with a clear schema error.

## Steps

1. **Backup**  
   `cp config/actuators/inventory.yaml config/actuators/inventory.yaml.v1.bak`

2. **Run the migration tool** (from the workspace `crates/` directory, or adjust paths):

   ```bash
   cargo run -p rudydae --bin migrate_inventory -- ../config/actuators/inventory.yaml
   ```

   This reads the v1 file and writes `config/actuators/inventory.yaml.v2`. It refuses to overwrite an existing `.v2` file.

3. **Review** the generated file and the stdout preview.

4. **Swap in** the v2 file:

   ```bash
   mv config/actuators/inventory.yaml config/actuators/inventory.yaml.v1.bak
   mv config/actuators/inventory.yaml.v2 config/actuators/inventory.yaml
   ```

5. **Restart** `rudydae` and confirm the SPA still lists your actuators.

## What changes

- Each v1 motor becomes an **actuator** device with `family: { kind: robstride, model: rs03 }`.
- Any keys that previously lived in the flattened `extra` map are preserved as a YAML string in `notes_yaml` (`ActuatorCommon`) so nothing is dropped silently.

## Rollback

Restore the v1 backup as `inventory.yaml` and run a rudydae build that still accepts v1 (pre–schema v2 migration).
