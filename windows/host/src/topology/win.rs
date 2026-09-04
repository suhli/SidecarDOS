use super::*;
use anyhow::{Context, bail};
use windows::Win32::{
    Devices::Display::*,
    Foundation::{ERROR_INSUFFICIENT_BUFFER, POINTL},
};
const DISPLAYCONFIG_PATH_ACTIVE: u32 = 1;
const DISPLAYCONFIG_PATH_MODE_IDX_INVALID: u32 = u32::MAX;
pub struct Snapshot {
    paths: Vec<DISPLAYCONFIG_PATH_INFO>,
    modes: Vec<DISPLAYCONFIG_MODE_INFO>,
}
fn query() -> Result<Snapshot> {
    unsafe {
        for _ in 0..4 {
            let (mut np, mut nm) = (0, 0);
            GetDisplayConfigBufferSizes(QDC_ALL_PATHS, &mut np, &mut nm).ok()?;
            ensure!(
                np < 1024 && nm < 4096,
                "display topology exceeds safety limit"
            );
            let (mut paths, mut modes) = (
                vec![DISPLAYCONFIG_PATH_INFO::default(); np as usize],
                vec![DISPLAYCONFIG_MODE_INFO::default(); nm as usize],
            );
            let e = QueryDisplayConfig(
                QDC_ALL_PATHS,
                &mut np,
                paths.as_mut_ptr(),
                &mut nm,
                modes.as_mut_ptr(),
                None,
            );
            if e == ERROR_INSUFFICIENT_BUFFER {
                continue;
            }
            e.ok()?;
            paths.truncate(np as usize);
            modes.truncate(nm as usize);
            return Ok(Snapshot { paths, modes });
        }
        bail!("display topology changed repeatedly during query")
    }
}
fn sidecar(p: &DISPLAYCONFIG_PATH_INFO) -> bool {
    unsafe {
        if p.targetInfo.outputTechnology != DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INDIRECT_WIRED {
            return false;
        }
        let mut name = DISPLAYCONFIG_TARGET_DEVICE_NAME::default();
        name.header = DISPLAYCONFIG_DEVICE_INFO_HEADER {
            r#type: DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
            size: std::mem::size_of_val(&name) as u32,
            adapterId: p.targetInfo.adapterId,
            id: p.targetInfo.id,
        };
        if DisplayConfigGetDeviceInfo(&mut name.header) != 0 {
            return false;
        }
        let path = String::from_utf16_lossy(&name.monitorDevicePath).to_ascii_lowercase();
        path.contains("sdc0001")
    }
}
fn source_index(p: &DISPLAYCONFIG_PATH_INFO, m: &[DISPLAYCONFIG_MODE_INFO]) -> Option<usize> {
    let idx = unsafe { p.sourceInfo.Anonymous.modeInfoIdx } as usize;
    m.get(idx)
        .filter(|v| {
            v.infoType == DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE
                && v.id == p.sourceInfo.id
                && v.adapterId == p.sourceInfo.adapterId
        })
        .map(|_| idx)
}
fn bounds(p: &DISPLAYCONFIG_PATH_INFO, m: &[DISPLAYCONFIG_MODE_INFO]) -> Option<Bounds> {
    let s = unsafe { m.get(source_index(p, m)?)?.Anonymous.sourceMode };
    Some(Bounds {
        x: s.position.x,
        y: s.position.y,
        width: s.width,
        height: s.height,
    })
}
pub fn current_bounds() -> Result<Bounds> {
    let s = query()?;
    s.paths
        .iter()
        .find(|p| p.flags & DISPLAYCONFIG_PATH_ACTIVE != 0 && sidecar(p))
        .and_then(|p| bounds(p, &s.modes))
        .context("SidecarDOS display is not active")
}
pub fn reconcile(
    position: Position,
    primary: bool,
    requested: Option<(u32, u32, u32)>,
) -> Result<(Bounds, bool)> {
    let mut s = query()?;
    let candidate = s
        .paths
        .iter()
        .find(|p| sidecar(p) && p.flags & DISPLAYCONFIG_PATH_ACTIVE != 0)
        .or_else(|| {
            s.paths
                .iter()
                .find(|p| sidecar(p) && p.targetInfo.targetAvailable.as_bool())
        })
        .copied()
        .context("virtual monitor not yet enumerated by Windows")?;
    let mut active: Vec<_> = s
        .paths
        .iter()
        .filter(|p| p.flags & DISPLAYCONFIG_PATH_ACTIVE != 0)
        .copied()
        .collect();
    let virtual_index = active.iter().position(sidecar);
    let physical = active
        .iter()
        .filter(|p| !sidecar(p))
        .filter_map(|p| bounds(p, &s.modes))
        .collect::<Vec<_>>();
    let anchor = physical
        .iter()
        .find(|b| b.x == 0 && b.y == 0)
        .or_else(|| physical.first())
        .copied()
        .context("no physical display anchor")?;
    let (width, height, fps) = requested.unwrap_or_else(|| {
        bounds(&candidate, &s.modes)
            .map(|b| (b.width, b.height, 60))
            .unwrap_or((1920, 1080, 60))
    });
    let desired = Bounds::adjacent(anchor, width, height, position);
    let i = if let Some(i) = virtual_index {
        i
    } else {
        let mut p = candidate;
        p.flags |= DISPLAYCONFIG_PATH_ACTIVE;
        active.push(p);
        active.len() - 1
    };
    let matched = virtual_index.is_some()
        && bounds(&active[i], &s.modes) == Some(desired)
        && !primary
        && requested.is_none();
    if matched {
        return Ok((desired, true));
    }
    let source = if let Some(j) = source_index(&active[i], &s.modes) {
        j
    } else {
        let j = s.modes.len();
        let mut m = DISPLAYCONFIG_MODE_INFO {
            infoType: DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE,
            id: active[i].sourceInfo.id,
            adapterId: active[i].sourceInfo.adapterId,
            ..Default::default()
        };
        m.Anonymous.sourceMode = DISPLAYCONFIG_SOURCE_MODE {
            width,
            height,
            pixelFormat: DISPLAYCONFIG_PIXELFORMAT_32BPP,
            position: POINTL {
                x: desired.x,
                y: desired.y,
            },
        };
        s.modes.push(m);
        active[i].sourceInfo.Anonymous.modeInfoIdx = j as u32;
        j
    };
    let old = unsafe { s.modes[source].Anonymous.sourceMode };
    if old.width != width || old.height != height || virtual_index.is_none() || requested.is_some()
    {
        active[i].targetInfo.Anonymous.modeInfoIdx = DISPLAYCONFIG_PATH_MODE_IDX_INVALID;
        active[i].targetInfo.refreshRate = DISPLAYCONFIG_RATIONAL {
            Numerator: fps,
            Denominator: 1,
        };
    }
    s.modes[source].Anonymous.sourceMode = DISPLAYCONFIG_SOURCE_MODE {
        width,
        height,
        pixelFormat: DISPLAYCONFIG_PIXELFORMAT_32BPP,
        position: POINTL {
            x: desired.x,
            y: desired.y,
        },
    };
    let target = if primary {
        for m in &mut s.modes {
            if m.infoType == DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE {
                unsafe {
                    m.Anonymous.sourceMode.position.x -= desired.x;
                    m.Anonymous.sourceMode.position.y -= desired.y;
                }
            }
        }
        Bounds {
            x: 0,
            y: 0,
            ..desired
        }
    } else {
        desired
    };
    if virtual_index.is_some() && current_bounds().ok() == Some(target) && requested.is_none() {
        return Ok((target, true));
    }
    let flags = SDC_USE_SUPPLIED_DISPLAY_CONFIG | SDC_ALLOW_CHANGES;
    unsafe {
        let result = SetDisplayConfig(Some(&active), Some(&s.modes), flags | SDC_VALIDATE);
        ensure!(result == 0, "topology validation failed: {result}");
        let result = SetDisplayConfig(
            Some(&active),
            Some(&s.modes),
            flags | SDC_APPLY | SDC_SAVE_TO_DATABASE,
        );
        ensure!(result == 0, "topology apply failed: {result}");
    }
    tracing::info!(target:"topology",x=target.x,y=target.y,width,height,"applied virtual display placement");
    Ok((target, false))
}
