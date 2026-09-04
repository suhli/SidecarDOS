#[cfg(windows)]
pub mod runtime;
#[cfg(windows)]
pub mod tray;
use std::time::{Duration, Instant};
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionState {
    Idle,
    Streaming { device: [u8; 16] },
    Grace { device: [u8; 16], deadline: Instant },
}
impl SessionState {
    pub fn accepts(&self, device: [u8; 16]) -> bool {
        match self {
            Self::Idle => true,
            Self::Streaming { .. } => false,
            Self::Grace { device: d, .. } => *d == device,
        }
    }
    pub fn disconnect(&mut self, now: Instant, grace: Duration) {
        if let Self::Streaming { device } = *self {
            *self = Self::Grace {
                device,
                deadline: now + grace,
            };
        }
    }
    pub fn expired(&self, now: Instant) -> bool {
        matches!(self,Self::Grace{deadline,..} if now>=*deadline)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grace_reserves_identity() {
        let t = Instant::now();
        let mut s = SessionState::Streaming { device: [1; 16] };
        s.disconnect(t, Duration::from_secs(4));
        assert!(s.accepts([1; 16]));
        assert!(!s.accepts([2; 16]));
        assert!(!s.expired(t));
        assert!(s.expired(t + Duration::from_secs(5)));
    }
}
