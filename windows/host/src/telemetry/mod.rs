use crate::protocol::Statistics;
use std::time::{Duration, Instant};
pub fn now_us() -> u64 {
    #[cfg(windows)]
    {
        use windows::Win32::System::Performance::{
            QueryPerformanceCounter, QueryPerformanceFrequency,
        };
        let (mut q, mut f) = (0i64, 0i64);
        unsafe {
            if QueryPerformanceCounter(&mut q).is_ok()
                && QueryPerformanceFrequency(&mut f).is_ok()
                && f > 0
            {
                return ((q / f) * 1_000_000 + (q % f) * 1_000_000 / f) as u64;
            }
        }
    }
    static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    EPOCH.get_or_init(Instant::now).elapsed().as_micros() as u64
}
pub struct AdaptiveBitrate {
    pub current: u32,
    min: u32,
    max: u32,
    healthy: u8,
    last: Instant,
}
impl AdaptiveBitrate {
    pub fn new(current: u32, min: u32, max: u32) -> Self {
        Self {
            current,
            min,
            max,
            healthy: 0,
            last: Instant::now(),
        }
    }
    pub fn update(&mut self, s: &Statistics, now: Instant) -> Option<u32> {
        if now.duration_since(self.last) < Duration::from_secs(1) {
            return None;
        }
        self.last = now;
        let before = self.current;
        if s.loss_ppm > 20_000 || s.rtt_us > 60_000 || s.decode_queue > 1 || s.dropped_frames > 3 {
            self.current = (self.current * 3 / 4).max(self.min);
            self.healthy = 0;
        } else if s.loss_ppm < 2000 && s.rtt_us < 30_000 && s.decode_queue == 0 {
            self.healthy += 1;
            if self.healthy >= 5 {
                self.current = (self.current + self.current / 20).min(self.max);
                self.healthy = 0;
            }
        } else {
            self.healthy = 0;
        }
        (before != self.current).then_some(self.current)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drops_quality_under_loss() {
        let mut a = AdaptiveBitrate::new(12_000_000, 2_000_000, 30_000_000);
        let s = Statistics {
            fps: 60.,
            bitrate: 0,
            rtt_us: 100_000,
            loss_ppm: 0,
            decode_queue: 0,
            dropped_frames: 0,
            received_bytes: 0,
            decode_us: 0,
            render_us: 0,
            estimated_latency_us: 0,
        };
        assert_eq!(
            a.update(&s, Instant::now() + Duration::from_secs(2)),
            Some(9_000_000)
        );
    }
}
