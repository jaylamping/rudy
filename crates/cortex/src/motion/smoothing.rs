//! One-pole LPF + optional minimum-jerk retiming for MIT position targets.

/// State for [`Self::smooth`], mirroring kscale-firmware policy filtering.
#[derive(Debug, Default)]
pub struct MitTargetSmoothing {
    initialized: bool,
    lpf_y: f32,
    mj_prev: f32,
    mj_elapsed_s: f32,
    mj_total_s: f32,
}

impl MitTargetSmoothing {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Returns filtered/blended target (rad). `feedback_pos_rad` seeds the
    /// filter on the first call after [`Self::reset`].
    pub fn smooth(
        &mut self,
        feedback_pos_rad: f32,
        raw_target_rad: f32,
        dt_s: f32,
        lpf_cutoff_hz: f32,
        min_jerk_blend_ms: f32,
    ) -> f32 {
        if !self.initialized {
            self.initialized = true;
            self.lpf_y = feedback_pos_rad;
            self.mj_prev = feedback_pos_rad;
            self.mj_elapsed_s = 0.0;
            self.mj_total_s = 0.0;
        }

        let unfiltered = raw_target_rad;
        let filtered = if lpf_cutoff_hz <= 0.0 || dt_s <= 0.0 {
            unfiltered
        } else {
            let alpha = 1.0 - (-2.0 * std::f32::consts::PI * lpf_cutoff_hz * dt_s).exp();
            self.lpf_y += alpha * (unfiltered - self.lpf_y);
            self.lpf_y
        };

        let blend_ms = min_jerk_blend_ms;
        let mut out = filtered;
        if blend_ms > 0.0 && dt_s > 0.0 {
            let t_total = (blend_ms / 1000.0).max(1e-6);
            let target = filtered;
            let prev = self.mj_prev;
            if (target - prev).abs() > 1e-6 {
                self.mj_elapsed_s = 0.0;
                self.mj_total_s = t_total;
            }
            let t = (self.mj_elapsed_s + dt_s).min(self.mj_total_s);
            self.mj_elapsed_s = t;
            let denom = self.mj_total_s;
            let s = if denom > 0.0 {
                (t / denom).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let s2 = s * s;
            let s3 = s2 * s;
            let s4 = s3 * s;
            let s5 = s4 * s;
            let mj = 10.0 * s3 - 15.0 * s4 + 6.0 * s5;
            out = prev + mj * (target - prev);
            if self.mj_elapsed_s >= self.mj_total_s - 1e-6 {
                out = target;
            }
            self.mj_prev = out;
        } else {
            self.mj_prev = out;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lpf_converges_toward_constant_target() {
        let mut s = MitTargetSmoothing::default();
        let mut y = 0.0_f32;
        for _ in 0..80 {
            y = s.smooth(0.0, 1.0, 0.01, 10.0, 0.0);
        }
        assert!(y > 0.95 && y <= 1.0, "y={y}");
    }

    #[test]
    fn min_jerk_blend_reaches_target_after_window() {
        let mut s = MitTargetSmoothing::default();
        // Single step spans full blend window so `(target - mj_prev)` does not
        // reset `mj_elapsed_s` mid-blend (constant target each call).
        let y = s.smooth(0.0, 1.0, 0.02, 0.0, 10.0);
        assert!((y - 1.0).abs() < 1e-3, "y={y}");
    }
}
