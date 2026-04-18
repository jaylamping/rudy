//! Typed loader for `config/actuators/inventory.yaml`.
//!
//! We only model the fields rudydae enforces or surfaces in the UI; the rest
//! are tolerated via `#[serde(flatten)]` into a catch-all map so the YAML
//! can grow without breaking rudydae.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::limb::JointKind;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Inventory {
    #[serde(default)]
    pub schema_version: Option<u32>,
    pub motors: Vec<Motor>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct Motor {
    pub role: String,
    pub can_bus: String,
    #[serde(with = "crate::util::serde_u8_flex")]
    #[ts(as = "u8")]
    pub can_id: u8,
    #[serde(default)]
    pub firmware_version: Option<String>,
    #[serde(default)]
    pub verified: bool,
    #[serde(default)]
    pub commissioned_at: Option<String>,
    /// Whether the physical motor is wired into the bus right now.
    ///
    /// Defaults to `true` so existing inventory entries keep behaving as before.
    /// Set to `false` for placeholder entries (motor planned but not yet on the
    /// bus) or for temporarily-removed motors. Affects:
    ///
    ///   * Real-CAN telemetry: rudydae skips polling absent motors so an
    ///     unanswered iface doesn't fill the SocketCAN txqueue and start
    ///     returning ENOBUFS (errno 105) on every send.
    ///   * Control plane: enable / stop / save / set_zero on an absent motor
    ///     are rejected at the API layer with a clean `409 Conflict` rather
    ///     than queuing CAN frames that will never get an ACK.
    ///   * Mock CAN + the REST `/api/motors` listing still include absent
    ///     motors so the UI can show them with an "absent" badge.
    #[serde(default = "default_true")]
    pub present: bool,
    /// Per-motor soft travel-limits band, in radians. None ≡ "use the spec
    /// default" (which is currently the full RS03 ±2-turn envelope from
    /// `protocol.position_min_rad / position_max_rad`).
    ///
    /// Edited via `PUT /api/motors/:role/travel_limits`; persisted by
    /// rewriting `inventory.yaml` atomically (see `inventory::write_atomic`).
    /// Enforced by `crate::can::travel::enforce_travel_band` on every
    /// commanded move (jog now, future move-to).
    #[serde(default)]
    pub travel_limits: Option<TravelLimits>,
    /// Free-form limb identifier (`left_arm`, `right_leg`, `torso`, `head`).
    /// Optional today: motors without `limb` are skipped by `POST /home_all`.
    /// Once set, the role becomes a derived identifier of the form
    /// `{limb}.{joint_kind}`; see [`Self::canonical_role`].
    #[serde(default)]
    pub limb: Option<String>,
    /// Canonical position in the kinematic chain. When set, `limb` must
    /// also be set and `role` must equal `{limb}.{joint_kind.as_snake_case}`.
    #[serde(default)]
    pub joint_kind: Option<JointKind>,
    /// Everything else in the YAML entry. Preserved for server-side logic
    /// but opaque to the UI (hence ts(skip)).
    #[serde(flatten)]
    #[ts(skip)]
    pub extra: BTreeMap<String, serde_yaml::Value>,
}

/// Per-actuator soft travel-limits band (radians).
///
/// Stored on each [`Motor`] in `config/actuators/inventory.yaml` and enforced
/// by rudydae on every commanded move (jog, future move-to). Semantically
/// this is a software-side inner cap; the firmware-level position envelope
/// remains authoritative.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct TravelLimits {
    pub min_rad: f32,
    pub max_rad: f32,
    /// ISO 8601 timestamp (UTC) of the most recent change. None on entries
    /// authored by hand or imported from a pre-rudydae inventory.
    #[serde(default)]
    pub updated_at: Option<String>,
}

fn default_true() -> bool {
    true
}

impl Motor {
    /// Canonical role derived from `limb` + `joint_kind`. Returns `None`
    /// when either field is absent — those motors are "ungrouped" and
    /// must be assigned via `POST /api/motors/:role/assign` before they
    /// can participate in `home_all`.
    pub fn canonical_role(&self) -> Option<String> {
        Some(format!(
            "{}.{}",
            self.limb.as_ref()?,
            self.joint_kind?.as_snake_case()
        ))
    }
}

/// Validate `role` matches the canonical form `[a-z][a-z_]*\.[a-z][a-z_]*`.
/// Used at inventory load time and at the API boundary so a malformed role
/// can never propagate through the system.
///
/// Existing legacy roles (e.g. `shoulder_actuator_a` from before the canonical
/// naming scheme) are accepted by this validator — see
/// [`Inventory::validate_strict`] for the stricter check that requires
/// canonical form.
pub fn validate_role_format(role: &str) -> Result<()> {
    if role.is_empty() {
        return Err(anyhow!("role is empty"));
    }
    // Legacy form: just `[a-z][a-z_0-9]*` (snake_case identifier).
    // Canonical form: `[a-z][a-z_]*\.[a-z][a-z_]*` — exactly one dot.
    let bytes = role.as_bytes();
    if !bytes[0].is_ascii_lowercase() {
        return Err(anyhow!("role {role} must start with a lowercase letter"));
    }
    let dots = role.matches('.').count();
    if dots > 1 {
        return Err(anyhow!("role {role} contains more than one dot"));
    }
    for &b in bytes {
        let ok = b.is_ascii_lowercase() || b == b'_' || b == b'.' || b.is_ascii_digit();
        if !ok {
            return Err(anyhow!(
                "role {role} contains illegal character `{}`",
                b as char
            ));
        }
    }
    Ok(())
}

/// Stricter validation: requires the role to be in canonical
/// `{limb}.{joint_kind}` form. Used by the rename / assign endpoints.
pub fn validate_canonical_role(role: &str) -> Result<()> {
    validate_role_format(role)?;
    if !role.contains('.') {
        return Err(anyhow!(
            "role {role} is not canonical (expected `{{limb}}.{{joint_kind}}`)"
        ));
    }
    let parts: Vec<&str> = role.split('.').collect();
    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
        return Err(anyhow!("role {role} must have exactly one non-empty dot"));
    }
    Ok(())
}

impl Inventory {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let inv: Inventory = serde_yaml::from_str(&text)
            .with_context(|| format!("parsing YAML in {}", path.display()))?;
        inv.validate()
            .with_context(|| format!("validating {}", path.display()))?;
        Ok(inv)
    }

    /// Cross-motor sanity checks. Run on every load and after any atomic
    /// rewrite. Catches the "operator hand-edited inventory.yaml
    /// inconsistently" case at the earliest possible moment.
    pub fn validate(&self) -> Result<()> {
        let mut roles: BTreeSet<&str> = BTreeSet::new();
        for m in &self.motors {
            validate_role_format(&m.role)
                .with_context(|| format!("motor {} has invalid role format", m.role))?;
            if !roles.insert(m.role.as_str()) {
                return Err(anyhow!("duplicate role: {}", m.role));
            }
            if m.joint_kind.is_some() && m.limb.is_none() {
                return Err(anyhow!(
                    "motor {} has joint_kind set without limb",
                    m.role
                ));
            }
            if let (Some(_), Some(_)) = (&m.limb, m.joint_kind) {
                if let Some(canonical) = m.canonical_role() {
                    if m.role != canonical {
                        return Err(anyhow!(
                            "motor {} has limb+joint_kind but role does not match canonical form `{}`",
                            m.role,
                            canonical
                        ));
                    }
                }
            }
        }
        // Per-limb uniqueness on (limb, joint_kind).
        let mut seen: BTreeSet<(String, JointKind)> = BTreeSet::new();
        for m in &self.motors {
            if let (Some(limb), Some(jk)) = (&m.limb, m.joint_kind) {
                let key = (limb.clone(), jk);
                if !seen.insert(key) {
                    return Err(anyhow!(
                        "duplicate joint_kind {:?} within limb {} (motor {})",
                        jk,
                        limb,
                        m.role
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn by_role(&self, role: &str) -> Option<&Motor> {
        self.motors.iter().find(|m| m.role == role)
    }

    #[allow(dead_code)]
    pub fn by_can_id(&self, can_id: u8) -> Option<&Motor> {
        self.motors.iter().find(|m| m.can_id == can_id)
    }
}

/// Atomic YAML rewrite: read the on-disk inventory, hand the parsed value to
/// `mutate`, then write the result to a sibling tempfile and rename it into
/// place. Either the rename succeeds and the new file is fully visible, or
/// it fails and the original file is untouched.
///
/// Used by the per-motor PUT endpoints (`travel_limits`, `verified`) so a
/// crash mid-write can never produce a half-written `inventory.yaml`.
///
/// Returns the post-mutation `Inventory` so callers can refresh in-memory
/// state without a re-read.
pub fn write_atomic(
    path: &Path,
    mutate: impl FnOnce(&mut Inventory) -> Result<()>,
) -> Result<Inventory> {
    let mut inv = Inventory::load(path)
        .with_context(|| format!("re-reading {} for write_atomic", path.display()))?;
    mutate(&mut inv)?;
    inv.validate()
        .context("post-mutation inventory validation failed")?;

    let yaml = serde_yaml::to_string(&inv).context("serialising inventory back to YAML")?;

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_stem = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("inventory.yaml");

    // tempfile in the *same* directory so the rename is atomic on POSIX.
    let mut tmp = tempfile::Builder::new()
        .prefix(&format!(".{file_stem}."))
        .suffix(".tmp")
        .tempfile_in(parent)
        .with_context(|| format!("creating tempfile next to {}", path.display()))?;

    {
        use std::io::Write;
        tmp.write_all(yaml.as_bytes())
            .context("writing inventory YAML to tempfile")?;
        tmp.as_file()
            .sync_all()
            .context("fsync inventory tempfile")?;
    }

    // `persist` does the rename. On Windows it can fail if the target is
    // open; rudydae never holds a handle to inventory.yaml between writes.
    tmp.persist(path)
        .with_context(|| format!("rename tempfile -> {}", path.display()))?;
    Ok(inv)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_role_format_accepts_legacy_and_canonical() {
        assert!(validate_role_format("shoulder_actuator_a").is_ok());
        assert!(validate_role_format("left_arm.shoulder_pitch").is_ok());
    }

    #[test]
    fn validate_role_format_rejects_dashes_uppercase_double_dot() {
        assert!(validate_role_format("Bad-Role").is_err());
        assert!(validate_role_format("Bad_Role").is_err());
        assert!(validate_role_format("too.many.dots").is_err());
        assert!(validate_role_format("").is_err());
        assert!(validate_role_format("9starts_with_digit").is_err());
    }

    #[test]
    fn validate_canonical_role_requires_dot() {
        assert!(validate_canonical_role("shoulder_actuator_a").is_err());
        assert!(validate_canonical_role("left_arm.shoulder_pitch").is_ok());
        assert!(validate_canonical_role(".shoulder_pitch").is_err());
        assert!(validate_canonical_role("left_arm.").is_err());
    }

    #[test]
    fn motor_canonical_role_uses_snake_case_joint_kind() {
        let m = Motor {
            role: "left_arm.shoulder_pitch".into(),
            can_bus: "can0".into(),
            can_id: 1,
            firmware_version: None,
            verified: false,
            commissioned_at: None,
            present: true,
            travel_limits: None,
            limb: Some("left_arm".into()),
            joint_kind: Some(JointKind::ShoulderPitch),
            extra: BTreeMap::new(),
        };
        assert_eq!(m.canonical_role().as_deref(), Some("left_arm.shoulder_pitch"));
    }

    #[test]
    fn validate_rejects_duplicate_joint_kind_in_same_limb() {
        let inv = Inventory {
            schema_version: Some(1),
            motors: vec![
                Motor {
                    role: "left_arm.shoulder_pitch".into(),
                    can_bus: "can0".into(),
                    can_id: 1,
                    firmware_version: None,
                    verified: false,
                    commissioned_at: None,
                    present: true,
                    travel_limits: None,
                    limb: Some("left_arm".into()),
                    joint_kind: Some(JointKind::ShoulderPitch),
                    extra: BTreeMap::new(),
                },
                Motor {
                    role: "left_arm.shoulder_pitch_dup".into(),
                    can_bus: "can0".into(),
                    can_id: 2,
                    firmware_version: None,
                    verified: false,
                    commissioned_at: None,
                    present: true,
                    travel_limits: None,
                    limb: Some("left_arm".into()),
                    joint_kind: Some(JointKind::ShoulderPitch),
                    extra: BTreeMap::new(),
                },
            ],
        };
        assert!(inv.validate().is_err());
    }

    #[test]
    fn validate_rejects_role_mismatching_canonical_form() {
        let inv = Inventory {
            schema_version: Some(1),
            motors: vec![Motor {
                role: "wrong.shoulder_pitch".into(),
                can_bus: "can0".into(),
                can_id: 1,
                firmware_version: None,
                verified: false,
                commissioned_at: None,
                present: true,
                travel_limits: None,
                limb: Some("left_arm".into()),
                joint_kind: Some(JointKind::ShoulderPitch),
                extra: BTreeMap::new(),
            }],
        };
        assert!(inv.validate().is_err());
    }

    #[test]
    fn validate_accepts_legacy_motor_without_limb() {
        let inv = Inventory {
            schema_version: Some(1),
            motors: vec![Motor {
                role: "shoulder_actuator_a".into(),
                can_bus: "can1".into(),
                can_id: 8,
                firmware_version: None,
                verified: false,
                commissioned_at: None,
                present: true,
                travel_limits: None,
                limb: None,
                joint_kind: None,
                extra: BTreeMap::new(),
            }],
        };
        assert!(inv.validate().is_ok());
    }
}
