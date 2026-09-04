use crate::{protocol::InputEvent, topology::Bounds};
use anyhow::{Result, ensure};
use std::collections::{BTreeMap, BTreeSet};
use windows::Win32::{
    Foundation::{POINT, RECT},
    UI::{
        Controls::*,
        Input::{KeyboardAndMouse::*, Pointer::*},
        WindowsAndMessaging::*,
    },
};
pub struct Injector {
    device: HSYNTHETICPOINTERDEVICE,
    touches: BTreeMap<u16, POINTER_TOUCH_INFO>,
    keys: BTreeSet<u16>,
    buttons: u32,
    pub bounds: Bounds,
}
impl Injector {
    pub fn new(bounds: Bounds) -> Result<Self> {
        let device = unsafe { CreateSyntheticPointerDevice(PT_TOUCH, 10, POINTER_FEEDBACK_NONE)? };
        Ok(Self {
            device,
            touches: BTreeMap::new(),
            keys: BTreeSet::new(),
            buttons: 0,
            bounds,
        })
    }
    pub fn event(&mut self, e: &InputEvent) -> Result<()> {
        ensure!(
            e.phase <= 3 && e.buttons <= 7 && e.is_repeat <= 1,
            "invalid input flags"
        );
        match e.kind {
            0 => self.touch(e),
            1 | 2 => self.mouse(e),
            3 => self.keyboard(e),
            _ => anyhow::bail!("unsupported input kind"),
        }
    }
    fn touch(&mut self, e: &InputEvent) -> Result<()> {
        ensure!(e.contact_id < 10, "too many touch contacts");
        let (mut x, mut y) = self.bounds.map(e.x, e.y)?;
        if e.phase >= 2
            && let Some(previous) = self.touches.get(&e.contact_id)
        {
            x = previous.pointerInfo.ptPixelLocation.x;
            y = previous.pointerInfo.ptPixelLocation.y;
        }
        if e.phase == 0 {
            ensure!(
                !self.touches.contains_key(&e.contact_id),
                "duplicate touch down"
            );
        } else {
            ensure!(
                self.touches.contains_key(&e.contact_id),
                "touch update without down"
            );
        }
        let flags = match e.phase {
            0 => POINTER_FLAG_DOWN | POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT,
            1 => POINTER_FLAG_UPDATE | POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT,
            2 => POINTER_FLAG_UP,
            _ => POINTER_FLAG_UP | POINTER_FLAG_CANCELED,
        };
        let t = POINTER_TOUCH_INFO {
            pointerInfo: POINTER_INFO {
                pointerType: PT_TOUCH,
                pointerId: e.contact_id as u32 + 1,
                pointerFlags: flags,
                ptPixelLocation: POINT { x, y },
                ..Default::default()
            },
            touchMask: 1,
            rcContact: RECT {
                left: x - 2,
                top: y - 2,
                right: x + 2,
                bottom: y + 2,
            },
            ..Default::default()
        };
        self.touches.insert(e.contact_id, t);
        self.inject_touches()?;
        if e.phase >= 2 {
            self.touches.remove(&e.contact_id);
        }
        for t in self.touches.values_mut() {
            t.pointerInfo.pointerFlags =
                POINTER_FLAG_UPDATE | POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT;
        }
        Ok(())
    }
    fn inject_touches(&self) -> Result<()> {
        let p: Vec<_> = self
            .touches
            .values()
            .map(|t| POINTER_TYPE_INFO {
                r#type: PT_TOUCH,
                Anonymous: POINTER_TYPE_INFO_0 { touchInfo: *t },
            })
            .collect();
        if !p.is_empty() {
            unsafe {
                InjectSyntheticPointerInput(self.device, &p)?;
            }
        }
        Ok(())
    }
    fn send(input: INPUT) -> Result<()> {
        ensure!(
            unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) } == 1,
            "input injection failed (check interactive session and target integrity level)"
        );
        Ok(())
    }
    fn mouse_packet(dx: i32, dy: i32, data: u32, flags: MOUSE_EVENT_FLAGS) -> Result<()> {
        Self::send(INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx,
                    dy,
                    mouseData: data,
                    dwFlags: flags,
                    ..Default::default()
                },
            },
        })
    }
    fn mouse(&mut self, e: &InputEvent) -> Result<()> {
        ensure!(
            e.dx.is_finite() && e.dy.is_finite() && e.dx.abs() <= 10000. && e.dy.abs() <= 10000.,
            "invalid mouse delta"
        );
        if !self.touches.is_empty() {
            return Ok(());
        }
        if e.kind == 1 {
            let (x, y) = self.bounds.map(e.x, e.y)?;
            unsafe {
                let left = GetSystemMetrics(SM_XVIRTUALSCREEN);
                let top = GetSystemMetrics(SM_YVIRTUALSCREEN);
                let w = GetSystemMetrics(SM_CXVIRTUALSCREEN).max(2);
                let h = GetSystemMetrics(SM_CYVIRTUALSCREEN).max(2);
                Self::mouse_packet(
                    ((x - left) as i64 * 65535 / (w - 1) as i64) as i32,
                    ((y - top) as i64 * 65535 / (h - 1) as i64) as i32,
                    0,
                    MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
                )?;
            }
        } else {
            Self::mouse_packet(e.dx as i32, e.dy as i32, 0, MOUSEEVENTF_MOVE)?;
        }
        for (mask, down, up) in [
            (1, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
            (2, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
            (4, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
        ] {
            if (e.buttons ^ self.buttons) & mask != 0 {
                Self::mouse_packet(0, 0, 0, if e.buttons & mask != 0 { down } else { up })?;
            }
        }
        self.buttons = e.buttons;
        if e.kind == 1 {
            if e.dy != 0. {
                Self::mouse_packet(0, 0, e.dy as i32 as u32, MOUSEEVENTF_WHEEL)?;
            }
            if e.dx != 0. {
                Self::mouse_packet(0, 0, e.dx as i32 as u32, MOUSEEVENTF_HWHEEL)?;
            }
        }
        Ok(())
    }
    fn key(code: u16, up: bool) -> Result<()> {
        let mut flags = KEYEVENTF_SCANCODE;
        if code & 0x100 != 0 {
            flags |= KEYEVENTF_EXTENDEDKEY
        }
        if up {
            flags |= KEYEVENTF_KEYUP
        }
        Self::send(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wScan: code & 0xff,
                    dwFlags: flags,
                    ..Default::default()
                },
            },
        })
    }
    fn keyboard(&mut self, e: &InputEvent) -> Result<()> {
        let Some(scan) = super::scan_code(e.physical_key) else {
            return Ok(());
        };
        for (mask, left, right) in [
            (1, 0x1d, 0x11d),
            (2, 0x2a, 0x36),
            (4, 0x38, 0x138),
            (8, 0x15b, 0x15c),
        ] {
            if scan == left || scan == right {
                continue;
            }
            if e.modifiers & mask != 0 {
                if !self.keys.contains(&left) && !self.keys.contains(&right) {
                    Self::key(left, false)?;
                    self.keys.insert(left);
                }
            } else {
                for code in [left, right] {
                    if self.keys.remove(&code) {
                        Self::key(code, true)?;
                    }
                }
            }
        }
        let up = e.phase >= 2;
        if up && !self.keys.contains(&scan) {
            return Ok(());
        }
        Self::key(scan, up)?;
        if up {
            self.keys.remove(&scan);
        } else {
            self.keys.insert(scan);
        }
        Ok(())
    }
    pub fn release_all(&mut self) {
        for t in self.touches.values_mut() {
            t.pointerInfo.pointerFlags = POINTER_FLAG_UP | POINTER_FLAG_CANCELED;
        }
        let _ = self.inject_touches();
        self.touches.clear();
        for &key in &self.keys {
            let _ = Self::key(key, true);
        }
        self.keys.clear();
        for (mask, flag) in [
            (1, MOUSEEVENTF_LEFTUP),
            (2, MOUSEEVENTF_RIGHTUP),
            (4, MOUSEEVENTF_MIDDLEUP),
        ] {
            if self.buttons & mask != 0 {
                let _ = Self::mouse_packet(0, 0, 0, flag);
            }
        }
        self.buttons = 0;
    }
}
impl Drop for Injector {
    fn drop(&mut self) {
        self.release_all();
        unsafe {
            DestroySyntheticPointerDevice(self.device);
        }
    }
}
