#[cfg(windows)]
mod driver;
use crate::protocol::{DisplayCapabilities, DisplayMode};
use anyhow::{Result, ensure};
#[cfg(windows)]
pub use driver::*;

pub fn modes(c: &DisplayCapabilities, fps: u32) -> Result<Vec<DisplayMode>> {
    ensure!(
        (320..=4094).contains(&c.physical_width) && (320..=4094).contains(&c.physical_height),
        "unsupported iPad dimensions"
    );
    ensure!(
        (1.0..=4.0).contains(&c.native_scale) && c.max_fps >= 30,
        "invalid display capabilities"
    );
    ensure!(
        c.logical_width > 0 && c.logical_height > 0,
        "invalid logical resolution"
    );
    let fps = fps.min(c.max_fps).min(60);
    let w = c.physical_width;
    let h = c.physical_height;
    let mut sizes = vec![];
    for max_edge in [1280, 1920, w.max(h)] {
        let scale = (max_edge as f64 / w.max(h) as f64).min(1.0);
        let pair = (
            ((w as f64 * scale) as u32) & !1,
            ((h as f64 * scale) as u32) & !1,
        );
        if !sizes.contains(&pair) {
            sizes.push(pair)
        }
    }
    let full_hd = if w >= h { (1920, 1080) } else { (1080, 1920) };
    if full_hd.0 <= w && full_hd.1 <= h && !sizes.contains(&full_hd) {
        sizes.push(full_hd);
    }
    ensure!(
        sizes
            .iter()
            .all(|&(w, h)| u64::from(w + 160) * u64::from(h + 30) * u64::from(fps) <= 655_350_000),
        "display timing exceeds EDID clock limit"
    );
    Ok(sizes
        .into_iter()
        .map(|(width, height)| DisplayMode {
            width,
            height,
            fps,
            scale_percent: if width >= 2300 { 150 } else { 100 },
        })
        .collect())
}
pub fn preferred(modes: &[DisplayMode]) -> Option<&DisplayMode> {
    modes
        .iter()
        .min_by_key(|m| (i64::from(m.width) * i64::from(m.height) - 1920 * 1080).abs())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_aspect_and_bounds() {
        let c = DisplayCapabilities {
            physical_width: 2732,
            physical_height: 2048,
            logical_width: 1366,
            logical_height: 1024,
            max_fps: 120,
            native_scale: 2.,
            safe_top: 0.,
            safe_right: 0.,
            safe_bottom: 0.,
            safe_left: 0.,
            orientation: 0,
        };
        let m = modes(&c, 60).unwrap();
        assert!(m.len() > 1);
        assert_eq!(preferred(&m).unwrap().width, 1920);
        assert!(
            m.iter()
                .all(|m| m.width % 2 == 0 && m.height % 2 == 0 && m.fps == 60)
        );
    }
}
