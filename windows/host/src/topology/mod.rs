use crate::config::Position;
use anyhow::{Result, ensure};
use std::time::{Duration, Instant};
#[cfg(windows)]
mod win;
#[cfg(windows)]
pub use win::*;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}
impl Bounds {
    pub fn map(&self, x: f32, y: f32) -> Result<(i32, i32)> {
        ensure!(
            x.is_finite()
                && y.is_finite()
                && (0.0..=1.0).contains(&x)
                && (0.0..=1.0).contains(&y)
                && self.width > 0
                && self.height > 0,
            "invalid input coordinate or display bounds"
        );
        Ok((
            self.x + (x * (self.width - 1) as f32).round() as i32,
            self.y + (y * (self.height - 1) as f32).round() as i32,
        ))
    }
    pub fn adjacent(anchor: Self, width: u32, height: u32, pos: Position) -> Self {
        let (x, y) = match pos {
            Position::Left => (anchor.x - width as i32, anchor.y),
            Position::Right => (anchor.x + anchor.width as i32, anchor.y),
            Position::Above => (anchor.x, anchor.y - height as i32),
            Position::Below => (anchor.x, anchor.y + anchor.height as i32),
        };
        Self {
            x,
            y,
            width,
            height,
        }
    }
}
/// Coalesces display events and stops reapplying a failing topology indefinitely.
pub struct Reconciler {
    due: Instant,
    retries: u8,
    pub dirty: bool,
    last_applied: Option<Instant>,
}
impl Default for Reconciler {
    fn default() -> Self {
        Self {
            due: Instant::now(),
            retries: 0,
            dirty: true,
            last_applied: None,
        }
    }
}
impl Reconciler {
    pub fn changed(&mut self, now: Instant) {
        if self
            .last_applied
            .is_some_and(|t| now.duration_since(t) < Duration::from_millis(750))
        {
            return;
        }
        self.due = now + Duration::from_millis(200);
        self.retries = 0;
        self.dirty = true;
    }
    pub fn ready(&self, now: Instant) -> bool {
        self.dirty && self.retries < 3 && now >= self.due
    }
    pub fn result(&mut self, now: Instant, matched: bool) {
        self.dirty = !matched;
        if !matched {
            self.retries += 1;
            self.due = now + Duration::from_millis(750);
            self.last_applied = Some(now);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn negative_origin_and_last_pixel() {
        let b = Bounds::adjacent(
            Bounds {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            1600,
            900,
            Position::Left,
        );
        assert_eq!(b.map(0., 0.).unwrap(), (-1600, 0));
        assert_eq!(b.map(1., 1.).unwrap(), (-1, 899));
        assert!(b.map(f32::NAN, 0.).is_err());
    }
    #[test]
    fn retry_is_bounded() {
        let mut r = Reconciler::default();
        let t = Instant::now();
        for i in 0..3 {
            let now = t + Duration::from_secs(i);
            assert!(r.ready(now));
            r.result(now, false)
        }
        assert!(!r.ready(t + Duration::from_secs(9)));
    }
}
