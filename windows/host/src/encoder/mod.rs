use crate::{display::Slot, protocol::VideoFrame};
use anyhow::Result;
#[derive(Clone, Copy)]
pub struct EncoderSettings {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate: u32,
}
pub trait VideoEncoder {
    fn configure(&mut self, settings: EncoderSettings) -> Result<()>;
    fn encode(&mut self, slot: usize, metadata: Slot) -> Result<()>;
    fn request_keyframe(&mut self) -> Result<()>;
    fn reconfigure(&mut self, settings: EncoderSettings) -> Result<()>;
    fn poll(&mut self) -> Result<Option<VideoFrame>>;
}
#[cfg(windows)]
mod mf;
#[cfg(windows)]
pub use mf::*;
