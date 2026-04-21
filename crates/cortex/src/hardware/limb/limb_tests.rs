use super::JointKind;

#[test]
fn arm_order_is_proximal_to_distal() {
    assert!(JointKind::ShoulderPitch.home_order() < JointKind::ElbowPitch.home_order());
    assert!(JointKind::ElbowPitch.home_order() < JointKind::WristPitch.home_order());
    assert!(JointKind::WristPitch.home_order() < JointKind::Gripper.home_order());
}

#[test]
fn leg_order_is_proximal_to_distal() {
    assert!(JointKind::HipYaw.home_order() < JointKind::KneePitch.home_order());
    assert!(JointKind::KneePitch.home_order() < JointKind::AnklePitch.home_order());
}

#[test]
fn torso_homes_before_arms_and_legs() {
    assert!(JointKind::WaistRotation.is_torso());
    assert!(JointKind::SpinePitch.is_torso());
    assert!(!JointKind::ShoulderPitch.is_torso());
    assert!(!JointKind::HipYaw.is_torso());
    assert!(JointKind::WaistRotation.home_order() < JointKind::ShoulderPitch.home_order());
}
