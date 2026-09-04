use super::*;
use anyhow::{bail, ensure};
use std::{ffi::c_void, ptr::NonNull};
#[repr(C)]
struct NativeOutput {
    frame: u64,
    capture_us: u64,
    keyframe: u32,
    size: u32,
}
unsafe extern "C" {
    fn sd_gpu_error() -> *const std::ffi::c_char;
    fn sd_gpu_create(
        low: u32,
        high: i32,
        width: u32,
        height: u32,
        fps: u32,
        bitrate: u32,
        prefix: *const u16,
        out: *mut *mut c_void,
    ) -> i32;
    fn sd_gpu_destroy(p: *mut c_void);
    fn sd_gpu_input(p: *mut c_void, slot: u32, frame: u64, capture: u64) -> i32;
    fn sd_gpu_poll(p: *mut c_void, bytes: *mut u8, capacity: u32, output: *mut NativeOutput)
    -> i32;
    fn sd_gpu_bitrate(p: *mut c_void, bitrate: u32) -> i32;
    fn sd_gpu_keyframe(p: *mut c_void) -> i32;
}
fn checked(code: i32, op: &str) -> Result<i32> {
    if code < 0 {
        bail!(
            "{op}: HRESULT 0x{:08x}: {}",
            code as u32,
            unsafe { std::ffi::CStr::from_ptr(sd_gpu_error()) }.to_string_lossy()
        )
    }
    Ok(code)
}
/// All COM and D3D operations stay on the dedicated capture thread.
pub struct MfEncoder {
    handle: NonNull<c_void>,
    settings: EncoderSettings,
    pub generation: u32,
    bytes: Vec<u8>,
    parameters: super::h264::ParameterSets,
}
impl MfEncoder {
    pub fn open(
        s: crate::display::Status,
        settings: EncoderSettings,
    ) -> Result<(Self, [String; 3])> {
        let prefix = format!(
            "Global\\SidecarDOS.{}",
            hex::encode(crate::pairing::random::<16>())
        );
        let wide: Vec<_> = prefix.encode_utf16().chain([0]).collect();
        let mut ptr = std::ptr::null_mut();
        unsafe {
            checked(
                sd_gpu_create(
                    s.luid_low,
                    s.luid_high,
                    settings.width,
                    settings.height,
                    settings.fps,
                    settings.bitrate,
                    wide.as_ptr(),
                    &mut ptr,
                ),
                "create hardware encoder",
            )?;
        }
        let handle = NonNull::new(ptr).ok_or_else(|| anyhow::anyhow!("encoder returned null"))?;
        Ok((
            Self {
                handle,
                settings,
                generation: s.generation,
                bytes: vec![0; crate::protocol::MAX_FRAME],
                parameters: super::h264::ParameterSets::default(),
            },
            std::array::from_fn(|i| format!("{prefix}.{i}")),
        ))
    }
}
impl VideoEncoder for MfEncoder {
    fn configure(&mut self, s: EncoderSettings) -> Result<()> {
        self.reconfigure(s)
    }
    fn encode(&mut self, slot: usize, m: Slot) -> Result<bool> {
        ensure!(slot < 3, "invalid GPU slot");
        let status = unsafe {
            checked(
                sd_gpu_input(self.handle.as_ptr(), slot as u32, m.frame, m.capture_us),
                "encode GPU surface",
            )?
        };
        Ok(status == 0)
    }
    fn request_keyframe(&mut self) -> Result<()> {
        unsafe {
            checked(sd_gpu_keyframe(self.handle.as_ptr()), "request keyframe")?;
        }
        Ok(())
    }
    fn reconfigure(&mut self, s: EncoderSettings) -> Result<()> {
        ensure!(
            s.width == self.settings.width
                && s.height == self.settings.height
                && s.fps == self.settings.fps,
            "resolution/fps change requires encoder recreation"
        );
        unsafe {
            checked(
                sd_gpu_bitrate(self.handle.as_ptr(), s.bitrate),
                "change bitrate",
            )?;
        }
        self.settings = s;
        Ok(())
    }
    fn poll(&mut self) -> Result<Option<VideoFrame>> {
        let mut o = NativeOutput {
            frame: 0,
            capture_us: 0,
            keyframe: 0,
            size: 0,
        };
        let result = unsafe {
            checked(
                sd_gpu_poll(
                    self.handle.as_ptr(),
                    self.bytes.as_mut_ptr(),
                    self.bytes.len() as u32,
                    &mut o,
                ),
                "poll encoder",
            )?
        };
        if result == 1 {
            return Ok(None);
        }
        ensure!(
            o.size as usize <= self.bytes.len(),
            "encoder output overflow"
        );
        let (data, keyframe) = self.parameters.prepare(&self.bytes[..o.size as usize])?;
        Ok(Some(VideoFrame {
            generation: self.generation,
            frame_id: o.frame,
            capture_timestamp: o.capture_us,
            encode_timestamp: crate::telemetry::now_us(),
            send_timestamp: 0,
            keyframe: u8::from(keyframe),
            data,
        }))
    }
}
impl Drop for MfEncoder {
    fn drop(&mut self) {
        unsafe {
            sd_gpu_destroy(self.handle.as_ptr());
        }
    }
}

#[cfg(test)]
mod tests {
    unsafe extern "C" {
        fn sd_gpu_self_test() -> i32;
    }
    #[test]
    #[ignore = "requires a physical GPU and a hardware H.264 MFT"]
    fn hardware_encoder_produces_idr() {
        let hr = unsafe { sd_gpu_self_test() };
        assert!(
            super::checked(hr, "hardware self-test").is_ok(),
            "{:?}",
            super::checked(hr, "hardware self-test")
        );
    }
}
