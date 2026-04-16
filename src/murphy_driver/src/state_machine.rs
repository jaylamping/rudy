//! Minimal actuator lifecycle state machine (safe sequencing).

/// High-level actuator lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActuatorState {
    /// Powered but not configured.
    Idle,
    /// Parameters loaded, not enabled.
    Configured,
    /// Actively sending frames / expecting telemetry.
    Running,
    /// Fault latched; requires explicit reset.
    Fault,
}

/// Events that drive [`ActuatorStateMachine`] transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActuatorEvent {
    ConfigureOk,
    Enable,
    Disable,
    WatchdogTimeout,
    BusError,
    ClearFault,
}

#[derive(Debug, Default, Clone)]
pub struct ActuatorStateMachine {
    state: ActuatorState,
}

impl ActuatorStateMachine {
    pub fn state(&self) -> ActuatorState {
        self.state
    }

    pub fn apply(&mut self, ev: ActuatorEvent) {
        use ActuatorEvent::*;
        use ActuatorState::*;
        self.state = match (self.state, ev) {
            (Idle, ConfigureOk) => Configured,
            (Configured, Enable) => Running,
            (Running, Disable) => Configured,
            (_, WatchdogTimeout) | (_, BusError) => Fault,
            (Fault, ClearFault) => Idle,
            (s, _) => s,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path_configure_enable_disable() {
        let mut sm = ActuatorStateMachine::default();
        assert_eq!(sm.state(), ActuatorState::Idle);
        sm.apply(ActuatorEvent::ConfigureOk);
        assert_eq!(sm.state(), ActuatorState::Configured);
        sm.apply(ActuatorEvent::Enable);
        assert_eq!(sm.state(), ActuatorState::Running);
        sm.apply(ActuatorEvent::Disable);
        assert_eq!(sm.state(), ActuatorState::Configured);
    }

    #[test]
    fn watchdog_fault_requires_reset() {
        let mut sm = ActuatorStateMachine::default();
        sm.apply(ActuatorEvent::ConfigureOk);
        sm.apply(ActuatorEvent::Enable);
        sm.apply(ActuatorEvent::WatchdogTimeout);
        assert_eq!(sm.state(), ActuatorState::Fault);
        sm.apply(ActuatorEvent::Enable); // ignored in Fault
        assert_eq!(sm.state(), ActuatorState::Fault);
        sm.apply(ActuatorEvent::ClearFault);
        assert_eq!(sm.state(), ActuatorState::Idle);
    }
}
