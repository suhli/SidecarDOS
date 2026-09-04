use crate::protocol::DisplayMode;
use anyhow::{Context, Result, ensure};
use windows::{
    Win32::{
        Devices::DeviceAndDriverInstallation::*,
        Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
        Storage::FileSystem::{CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_MODE, OPEN_EXISTING},
        System::IO::DeviceIoControl,
    },
    core::{GUID, PCWSTR},
};

const INTERFACE: GUID = GUID::from_u128(0x777d8d93_591a_44f4_8b3c_756e20dd9c58);
const START: u32 = 0x22a000;
const STOP: u32 = 0x22a004;
const STATUS: u32 = 0x226008;
const SURFACES: u32 = 0x22a00c;
#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct Mode {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}
#[repr(C)]
struct Start {
    abi: u32,
    count: u32,
    identity: [u8; 16],
    modes: [Mode; 4],
}
#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct Slot {
    pub frame: u64,
    pub capture_us: u64,
}
#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct Status {
    pub abi: u32,
    pub generation: u32,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub luid_low: u32,
    pub luid_high: i32,
    pub reserved: u32,
    pub slots: [Slot; 3],
}
#[repr(C)]
struct Surfaces {
    abi: u32,
    generation: u32,
    names: [[u16; 128]; 3],
}
pub struct Driver {
    handle: HANDLE,
}
impl Driver {
    pub fn open() -> Result<Self> {
        unsafe {
            let set = SetupDiGetClassDevsW(
                Some(&INTERFACE),
                None,
                None,
                DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
            )?;
            struct Set(HDEVINFO);
            impl Drop for Set {
                fn drop(&mut self) {
                    unsafe {
                        let _ = SetupDiDestroyDeviceInfoList(self.0);
                    }
                }
            }
            let set = Set(set);
            let mut interface = SP_DEVICE_INTERFACE_DATA {
                cbSize: std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
                ..Default::default()
            };
            SetupDiEnumDeviceInterfaces(set.0, None, &INTERFACE, 0, &mut interface)
                .context("SidecarDOS driver not installed or not started")?;
            let mut size = 0;
            let _ =
                SetupDiGetDeviceInterfaceDetailW(set.0, &interface, None, 0, Some(&mut size), None);
            ensure!((8..65536).contains(&size), "invalid driver interface path");
            let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
            let detail = buffer
                .as_mut_ptr()
                .cast::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>();
            (*detail).cbSize = std::mem::size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
            SetupDiGetDeviceInterfaceDetailW(set.0, &interface, Some(detail), size, None, None)?;
            let handle = CreateFileW(
                PCWSTR((*detail).DevicePath.as_ptr()),
                0xc0000000,
                FILE_SHARE_MODE(0),
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )?;
            ensure!(handle != INVALID_HANDLE_VALUE, "invalid driver handle");
            Ok(Self { handle })
        }
    }
    fn ioctl<I, O>(&self, code: u32, input: Option<&I>, output: Option<&mut O>) -> Result<()> {
        let mut returned = 0;
        let output_len = if output.is_some() {
            std::mem::size_of::<O>() as u32
        } else {
            0
        };
        unsafe {
            DeviceIoControl(
                self.handle,
                code,
                input.map(|v| (v as *const I).cast()),
                if input.is_some() {
                    std::mem::size_of::<I>() as u32
                } else {
                    0
                },
                output.map(|v| (v as *mut O).cast()),
                output_len,
                Some(&mut returned),
                None,
            )?;
        }
        if output_len > 0 {
            ensure!(returned == output_len, "driver ABI length mismatch");
        }
        Ok(())
    }
    pub fn start(&self, id: [u8; 16], modes: &[DisplayMode]) -> Result<()> {
        ensure!(
            !modes.is_empty() && modes.len() <= 4,
            "invalid display mode count"
        );
        let mut s = Start {
            abi: 1,
            count: modes.len() as u32,
            identity: id,
            modes: [Mode::default(); 4],
        };
        for (out, m) in s.modes.iter_mut().zip(modes) {
            *out = Mode {
                width: m.width,
                height: m.height,
                fps: m.fps,
            };
        }
        self.ioctl::<_, ()>(START, Some(&s), None)
            .context("virtual monitor arrival")
    }
    pub fn stop(&self) -> Result<()> {
        self.ioctl::<(), ()>(STOP, None, None)
    }
    pub fn status(&self) -> Result<Status> {
        let mut s = Status::default();
        self.ioctl::<(), _>(STATUS, None, Some(&mut s))?;
        ensure!(s.abi == 1, "driver ABI incompatible");
        Ok(s)
    }
    pub fn surfaces(&self, generation: u32, names: &[String; 3]) -> Result<()> {
        let mut s = Surfaces {
            abi: 1,
            generation,
            names: [[0; 128]; 3],
        };
        for (out, name) in s.names.iter_mut().zip(names) {
            let utf: Vec<_> = name.encode_utf16().collect();
            ensure!(utf.len() < 128, "surface name too long");
            out[..utf.len()].copy_from_slice(&utf);
        }
        self.ioctl::<_, ()>(SURFACES, Some(&s), None)
    }
}
impl Drop for Driver {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}
