//! Group actuators by limb for `POST /api/home_all`.

use std::collections::BTreeMap;

use crate::inventory::{Inventory, Motor};

/// Group present actuators by `limb`, returning each limb's actuators sorted in
/// proximal-to-distal home order. Actuators without `limb` are excluded.
pub fn ordered_motors_per_limb(inv: &Inventory) -> BTreeMap<String, Vec<&Motor>> {
    let mut by_limb: BTreeMap<String, Vec<&Motor>> = BTreeMap::new();
    for m in inv.actuators() {
        if !m.common.present {
            continue;
        }
        let Some(limb) = m.common.limb.as_ref() else {
            continue;
        };
        by_limb.entry(limb.clone()).or_default().push(m);
    }
    for motors in by_limb.values_mut() {
        motors.sort_by_key(|m| m.common.joint_kind.map(|jk| jk.home_order()).unwrap_or(255));
    }
    by_limb
}

/// Like [`ordered_motors_per_limb`] but clones each [`Motor`] so callers can
/// `tokio::spawn` without holding a borrow of [`Inventory`].
pub fn ordered_motors_per_limb_owned(inv: &Inventory) -> BTreeMap<String, Vec<Motor>> {
    let mut by_limb: BTreeMap<String, Vec<Motor>> = BTreeMap::new();
    for m in inv.actuators() {
        if !m.common.present {
            continue;
        }
        let Some(limb) = m.common.limb.as_ref() else {
            continue;
        };
        by_limb.entry(limb.clone()).or_default().push(m.clone());
    }
    for motors in by_limb.values_mut() {
        motors.sort_by_key(|m| m.common.joint_kind.map(|jk| jk.home_order()).unwrap_or(255));
    }
    by_limb
}
