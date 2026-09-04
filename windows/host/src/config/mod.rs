use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Position {
    Left,
    #[default]
    Right,
    Above,
    Below,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Display {
    pub position: Position,
    pub primary: bool,
    pub reconnect_grace_ms: u64,
}
impl Default for Display {
    fn default() -> Self {
        Self {
            position: Position::Right,
            primary: false,
            reconnect_grace_ms: 4000,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Video {
    pub fps: u32,
    pub bitrate: u32,
    pub min_bitrate: u32,
    pub max_bitrate: u32,
    pub adaptive: bool,
}
impl Default for Video {
    fn default() -> Self {
        Self {
            fps: 60,
            bitrate: 12_000_000,
            min_bitrate: 2_000_000,
            max_bitrate: 30_000_000,
            adaptive: true,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub display: Display,
    pub video: Video,
    pub port: u16,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            display: Display::default(),
            video: Video::default(),
            port: 47736,
        }
    }
}
impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let c: Self = if path.exists() {
            toml::from_str(&std::fs::read_to_string(path)?)?
        } else {
            Self::default()
        };
        c.validate()?;
        Ok(c)
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (30..=60).contains(&self.video.fps),
            "MVP fps must be 30..60"
        );
        ensure!(self.port > 1024, "port must be above 1024");
        ensure!(
            (1000..=10000).contains(&self.display.reconnect_grace_ms),
            "grace period must be 1..10 seconds"
        );
        ensure!(
            self.video.min_bitrate >= 500_000
                && self.video.min_bitrate <= self.video.bitrate
                && self.video.bitrate <= self.video.max_bitrate
                && self.video.max_bitrate <= 100_000_000,
            "invalid bitrate bounds"
        );
        Ok(())
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        atomic_write(path, toml::to_string_pretty(self)?.as_bytes())
    }
}
pub fn directory() -> Result<PathBuf> {
    let p = PathBuf::from(
        std::env::var_os("LOCALAPPDATA")
            .context("LOCALAPPDATA missing; run in the interactive user session")?,
    )
    .join("SidecarDOS");
    std::fs::create_dir_all(&p)?;
    Ok(p)
}
pub fn atomic_write(path: &Path, data: &[u8]) -> Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("new");
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(data)?;
    f.sync_all()?;
    drop(f);
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::{
            Win32::Storage::FileSystem::{
                MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
            },
            core::PCWSTR,
        };
        let a: Vec<u16> = tmp.as_os_str().encode_wide().chain([0]).collect();
        let b: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        unsafe {
            MoveFileExW(
                PCWSTR(a.as_ptr()),
                PCWSTR(b.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )?;
        }
    }
    #[cfg(not(windows))]
    std::fs::rename(tmp, path)?;
    Ok(())
}
