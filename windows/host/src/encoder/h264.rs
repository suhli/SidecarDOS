use anyhow::{Result, ensure};
pub fn units(data: &[u8]) -> Result<Vec<&[u8]>> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 3 <= data.len() {
        if data[i] == 0 && data[i + 1] == 0 {
            if data[i + 2] == 1 {
                starts.push((i, i + 3));
                i += 3;
                continue;
            }
            if i + 4 <= data.len() && data[i + 2] == 0 && data[i + 3] == 1 {
                starts.push((i, i + 4));
                i += 4;
                continue;
            }
        }
        i += 1;
    }
    ensure!(
        !starts.is_empty() && starts.len() <= 4096,
        "invalid H.264 Annex B access unit"
    );
    starts
        .iter()
        .enumerate()
        .map(|(i, &(_, begin))| {
            let end = starts.get(i + 1).map(|s| s.0).unwrap_or(data.len());
            ensure!(end > begin, "empty H.264 NAL");
            Ok(&data[begin..end])
        })
        .collect()
}
#[derive(Default)]
pub struct ParameterSets {
    sps: Vec<u8>,
    pps: Vec<u8>,
}
impl ParameterSets {
    pub fn prepare(&mut self, data: &[u8]) -> Result<(Vec<u8>, bool)> {
        let nals = units(data)?;
        let (mut sps, mut pps, mut idr) = (false, false, false);
        for n in nals {
            match n[0] & 31 {
                7 => {
                    self.sps = n.to_vec();
                    sps = true
                }
                8 => {
                    self.pps = n.to_vec();
                    pps = true
                }
                5 => idr = true,
                _ => {}
            }
        }
        let mut out = Vec::new();
        if idr {
            ensure!(
                !self.sps.is_empty() && !self.pps.is_empty(),
                "H.264 keyframe lacks parameter sets"
            );
            if !sps {
                out.extend([0, 0, 0, 1]);
                out.extend(&self.sps);
            }
            if !pps {
                out.extend([0, 0, 0, 1]);
                out.extend(&self.pps);
            }
        }
        out.extend(data);
        ensure!(
            out.len() <= crate::protocol::MAX_FRAME,
            "H.264 access unit exceeds maximum"
        );
        Ok((out, idr))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn idr_repeats_parameter_sets() {
        let mut p = ParameterSets::default();
        let initial = [
            0, 0, 0, 1, 0x67, 0xaa, 0, 0, 1, 0x68, 0xbb, 0, 0, 1, 0x65, 0xcc,
        ];
        assert!(p.prepare(&initial).unwrap().1);
        let (next, key) = p.prepare(&[0, 0, 0, 1, 0x65, 0xdd]).unwrap();
        assert!(key);
        assert_eq!(
            units(&next)
                .unwrap()
                .iter()
                .map(|n| n[0] & 31)
                .collect::<Vec<_>>(),
            vec![7, 8, 5]
        );
        assert!(units(&[0, 0, 1]).is_err());
        assert!(units(&[5, 6, 7]).is_err());
    }
}
