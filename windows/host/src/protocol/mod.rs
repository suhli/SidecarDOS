//! Length-delimited, little-endian binary protocol. Limits apply before allocation.
#[rustfmt::skip]
mod generated;
use anyhow::{bail, ensure};
pub use generated::*;
pub type Result<T> = anyhow::Result<T>;
pub const MAJOR: u8 = 1;
pub const MINOR: u8 = 0;
pub const HEADER: usize = 36;
pub const MAX_CONTROL: usize = 65_536;
pub const MAX_FRAME: usize = 2 * 1024 * 1024;
pub const MAX_DATAGRAM: usize = 1200;

pub trait Wire: Sized {
    const KIND: u16;
    fn write(&self, w: &mut Writer);
    fn read(r: &mut Reader<'_>) -> Result<Self>;
}
#[derive(Default)]
pub struct Writer(pub Vec<u8>);
impl Writer {
    pub fn u8(&mut self, v: u8) {
        self.0.push(v)
    }
    pub fn u16(&mut self, v: u16) {
        self.0.extend(v.to_le_bytes())
    }
    pub fn u32(&mut self, v: u32) {
        self.0.extend(v.to_le_bytes())
    }
    pub fn u64(&mut self, v: u64) {
        self.0.extend(v.to_le_bytes())
    }
    pub fn i32(&mut self, v: i32) {
        self.0.extend(v.to_le_bytes())
    }
    pub fn f32(&mut self, v: f32) {
        self.u32(v.to_bits())
    }
    pub fn bytes(&mut self, v: &[u8]) {
        self.u32(v.len() as u32);
        self.0.extend(v)
    }
    pub fn string(&mut self, v: &str) {
        self.bytes(v.as_bytes())
    }
}
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}
impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        ensure!(
            n <= self.data.len().saturating_sub(self.pos),
            "truncated message"
        );
        let result = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(result)
    }
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into()?))
    }
    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?))
    }
    pub fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into()?))
    }
    pub fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into()?))
    }
    pub fn f32(&mut self) -> Result<f32> {
        let v = f32::from_bits(self.u32()?);
        ensure!(v.is_finite(), "nonfinite number");
        Ok(v)
    }
    pub fn bytes(&mut self) -> Result<Vec<u8>> {
        let n = self.u32()? as usize;
        ensure!(n <= MAX_FRAME, "field too large");
        Ok(self.take(n)?.to_vec())
    }
    pub fn string(&mut self) -> Result<String> {
        let b = self.bytes()?;
        ensure!(b.len() <= 1024, "string too long");
        Ok(String::from_utf8(b)?)
    }
    pub fn finish(&self) -> Result<()> {
        ensure!(self.pos == self.data.len(), "trailing payload");
        Ok(())
    }
}
#[derive(Debug)]
pub struct Packet {
    pub kind: u16,
    pub sequence: u64,
    pub session: [u8; 16],
    pub body: Vec<u8>,
}
impl Packet {
    pub fn new<T: Wire>(msg: &T, sequence: u64, session: [u8; 16]) -> Self {
        let mut w = Writer::default();
        msg.write(&mut w);
        Self {
            kind: T::KIND,
            sequence,
            session,
            body: w.0,
        }
    }
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer(b"SDOS".to_vec());
        w.u8(MAJOR);
        w.u8(MINOR);
        w.u16(self.kind);
        w.u32(self.body.len() as u32);
        w.u64(self.sequence);
        w.0.extend(self.session);
        w.0.extend(&self.body);
        w.0
    }
    pub fn decode(data: &[u8], limit: usize) -> Result<Self> {
        ensure!(
            data.len() >= HEADER && data.len() <= limit,
            "packet length out of bounds"
        );
        let mut r = Reader::new(data);
        ensure!(r.take(4)? == b"SDOS", "bad magic");
        ensure!(r.u8()? == MAJOR, "unsupported protocol major");
        let _minor = r.u8()?;
        let kind = r.u16()?;
        let n = r.u32()? as usize;
        let sequence = r.u64()?;
        let session = r.take(16)?.try_into()?;
        ensure!(n == data.len() - HEADER, "body length mismatch");
        let body = r.take(n)?.to_vec();
        Ok(Self {
            kind,
            sequence,
            session,
            body,
        })
    }
    pub fn message<T: Wire>(&self) -> Result<T> {
        ensure!(self.kind == T::KIND, "unexpected message kind");
        let mut r = Reader::new(&self.body);
        let v = T::read(&mut r)?;
        r.finish()?;
        Ok(v)
    }
}
pub fn negotiate(min: u8, max: u8, _minor: u8) -> Result<(u8, u8)> {
    if min > MAJOR || max < MAJOR || min > max {
        bail!("no compatible protocol version")
    };
    Ok((MAJOR, MINOR))
}
#[derive(Default)]
pub struct ReplayGuard(u64);
impl ReplayGuard {
    pub fn accept(&mut self, n: u64) -> Result<()> {
        ensure!(n > self.0, "replayed or out-of-order reliable message");
        self.0 = n;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_and_limits() {
        let p = Packet::new(
            &Ping {
                client_timestamp: 42,
            },
            1,
            [7; 16],
        );
        let b = p.encode();
        assert_eq!(
            Packet::decode(&b, MAX_CONTROL)
                .unwrap()
                .message::<Ping>()
                .unwrap()
                .client_timestamp,
            42
        );
        for n in 0..b.len() {
            assert!(Packet::decode(&b[..n], MAX_CONTROL).is_err())
        }
        let mut bad = b;
        bad[8] = 255;
        assert!(Packet::decode(&bad, MAX_CONTROL).is_err());
    }
    #[test]
    fn version_and_replay() {
        assert!(negotiate(2, 3, 0).is_err());
        assert_eq!(negotiate(1, 2, 9).unwrap(), (1, 0));
        let mut g = ReplayGuard::default();
        assert!(g.accept(1).is_ok());
        assert!(g.accept(1).is_err());
        assert!(g.accept(0).is_err());
    }
    #[test]
    fn malicious_fields() {
        let mut r = Reader::new(&[255; 4]);
        assert!(r.bytes().is_err());
        let b = f32::NAN.to_le_bytes();
        assert!(Reader::new(&b).f32().is_err());
    }
}
