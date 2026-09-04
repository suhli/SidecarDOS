use anyhow::{Result, ensure};
use hmac::{Hmac, Mac};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub fn random<const N: usize>() -> [u8; N] {
    let mut b = [0; N];
    rand::rngs::OsRng.fill_bytes(&mut b);
    b
}
pub fn proof(
    key: &[u8],
    certificate: &[u8],
    client: &[u8],
    nonce: &[u8],
    role: u8,
) -> Result<Vec<u8>> {
    let mut m = Hmac::<Sha256>::new_from_slice(key)?;
    m.update(b"SidecarDOS pairing v1");
    m.update(&[role]);
    m.update(&Sha256::digest(certificate));
    m.update(client);
    m.update(nonce);
    Ok(m.finalize().into_bytes().to_vec())
}
pub fn verify(
    key: &[u8],
    certificate: &[u8],
    client: &[u8],
    nonce: &[u8],
    role: u8,
    received: &[u8],
) -> Result<()> {
    ensure!(
        client.len() == 16 && nonce.len() == 32 && key.len() >= 16,
        "invalid authentication context"
    );
    let mut m = Hmac::<Sha256>::new_from_slice(key)?;
    m.update(b"SidecarDOS pairing v1");
    m.update(&[role]);
    m.update(&Sha256::digest(certificate));
    m.update(client);
    m.update(nonce);
    m.verify_slice(received)?;
    Ok(())
}
#[derive(Serialize, Deserialize)]
struct Stored {
    host_id: String,
    cert: String,
    key: String,
    devices: BTreeMap<String, String>,
}
pub struct Identity {
    pub host_id: [u8; 16],
    pub cert: Vec<u8>,
    pub key: Vec<u8>,
    devices: BTreeMap<String, String>,
    path: PathBuf,
}
impl Identity {
    pub fn load(path: &Path) -> Result<Self> {
        if path.exists() {
            let bytes = unprotect(&std::fs::read(path)?)?;
            let s: Stored = toml::from_str(std::str::from_utf8(&bytes)?)?;
            return Ok(Self {
                host_id: hex::decode(s.host_id)?
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("invalid host identity"))?,
                cert: hex::decode(s.cert)?,
                key: hex::decode(s.key)?,
                devices: s.devices,
                path: path.to_owned(),
            });
        }
        let c = rcgen::generate_simple_self_signed(vec!["sidecardos.local".into()])?;
        let s = Self {
            host_id: random(),
            cert: c.cert.der().to_vec(),
            key: c.key_pair.serialize_der(),
            devices: BTreeMap::new(),
            path: path.to_owned(),
        };
        s.save()?;
        Ok(s)
    }
    pub fn trusted(&self, id: &[u8; 16]) -> Result<Option<Vec<u8>>> {
        self.devices
            .get(&hex::encode(id))
            .map(|v| hex::decode(v).map_err(Into::into))
            .transpose()
    }
    pub fn trust(&mut self, id: &[u8; 16], key: &[u8]) -> Result<()> {
        ensure!(self.devices.len() < 64, "trusted device limit reached");
        self.devices.insert(hex::encode(id), hex::encode(key));
        self.save()
    }
    pub fn forget_all(&mut self) -> Result<()> {
        self.devices.clear();
        self.save()
    }
    fn save(&self) -> Result<()> {
        let s = Stored {
            host_id: hex::encode(self.host_id),
            cert: hex::encode(&self.cert),
            key: hex::encode(&self.key),
            devices: self.devices.clone(),
        };
        crate::config::atomic_write(&self.path, &protect(toml::to_string(&s)?.as_bytes())?)
    }
}
#[cfg(windows)]
fn crypt(data: &[u8], encrypt: bool) -> Result<Vec<u8>> {
    use windows::Win32::{
        Foundation::{HLOCAL, LocalFree},
        Security::Cryptography::*,
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        if encrypt {
            CryptProtectData(
                &input,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )?;
        } else {
            CryptUnprotectData(
                &input,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )?;
        }
        let b = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(output.pbData.cast())));
        Ok(b)
    }
}
#[cfg(windows)]
fn protect(d: &[u8]) -> Result<Vec<u8>> {
    crypt(d, true)
}
#[cfg(windows)]
fn unprotect(d: &[u8]) -> Result<Vec<u8>> {
    crypt(d, false)
}
#[cfg(not(windows))]
fn protect(_: &[u8]) -> Result<Vec<u8>> {
    anyhow::bail!("device identities require Windows DPAPI")
}
#[cfg(not(windows))]
fn unprotect(_: &[u8]) -> Result<Vec<u8>> {
    anyhow::bail!("device identities require Windows DPAPI")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn proof_binds_certificate_nonce_and_role() {
        let key = [3; 32];
        let nonce = [4; 32];
        let id = [1; 16];
        let p = proof(&key, b"cert", &id, &nonce, 1).unwrap();
        assert!(verify(&key, b"cert", &id, &nonce, 1, &p).is_ok());
        assert!(verify(&key, b"mitm", &id, &nonce, 1, &p).is_err());
        assert!(verify(&key, b"cert", &id, &nonce, 2, &p).is_err());
        assert!(verify(&key, b"cert", &id, &[5; 32], 1, &p).is_err());
    }
}
