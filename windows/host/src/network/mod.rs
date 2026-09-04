use crate::protocol::*;
use anyhow::{Context, Result, ensure};
use quinn::{Connection, Endpoint, RecvStream, SendStream};
use std::{sync::Arc, time::Duration};
pub const ALPN: &[u8] = b"sidecardos/1";

pub fn server(port: u16, cert: Vec<u8>, key: Vec<u8>) -> Result<Endpoint> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut tls = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![rustls::pki_types::CertificateDer::from(cert)],
            rustls::pki_types::PrivatePkcs8KeyDer::from(key).into(),
        )?;
    tls.alpn_protocols = vec![ALPN.to_vec()];
    tls.max_early_data_size = 0;
    let crypto = quinn::crypto::rustls::QuicServerConfig::try_from(tls)?;
    let mut config = quinn::ServerConfig::with_crypto(Arc::new(crypto));
    let mut transport = quinn::TransportConfig::default();
    transport
        .max_concurrent_bidi_streams(2u8.into())
        .max_concurrent_uni_streams(2u8.into())
        .max_idle_timeout(Some(Duration::from_secs(6).try_into()?))
        .keep_alive_interval(Some(Duration::from_secs(1)))
        .datagram_receive_buffer_size(Some(64 * 1024))
        .datagram_send_buffer_size(MAX_DATAGRAM * 4)
        .stream_receive_window((MAX_CONTROL as u32 * 2).into())
        .receive_window((MAX_CONTROL as u32 * 4).into());
    config.transport_config(Arc::new(transport));
    Ok(Endpoint::server(config, ([0, 0, 0, 0], port).into())?)
}
pub struct Control {
    pub send: SendStream,
    pub recv: RecvStream,
    sequence: u64,
    guard: ReplayGuard,
    pub session: [u8; 16],
}
impl Control {
    pub async fn accept(c: &Connection) -> Result<Self> {
        let (send, recv) = tokio::time::timeout(Duration::from_secs(10), c.accept_bi()).await??;
        Ok(Self {
            send,
            recv,
            sequence: 0,
            guard: ReplayGuard::default(),
            session: [0; 16],
        })
    }
    pub async fn send<T: Wire>(&mut self, v: &T) -> Result<()> {
        self.sequence += 1;
        let b = Packet::new(v, self.sequence, self.session).encode();
        ensure!(b.len() <= MAX_CONTROL, "control too large");
        tokio::time::timeout(Duration::from_secs(2), self.send.write_all(&b)).await??;
        Ok(())
    }
    pub async fn receive(&mut self) -> Result<Packet> {
        let p = read_packet(&mut self.recv).await?;
        self.guard.accept(p.sequence)?;
        ensure!(p.session == self.session, "session mismatch");
        Ok(p)
    }
}
pub async fn read_packet(stream: &mut RecvStream) -> Result<Packet> {
    let mut header = [0u8; HEADER];
    stream.read_exact(&mut header).await?;
    ensure!(&header[..4] == b"SDOS", "invalid stream framing");
    let len = u32::from_le_bytes(header[8..12].try_into()?) as usize;
    ensure!(len <= MAX_CONTROL - HEADER, "oversized reliable message");
    let mut data = Vec::with_capacity(HEADER + len);
    data.extend(header);
    data.resize(HEADER + len, 0);
    stream.read_exact(&mut data[HEADER..]).await?;
    Packet::decode(&data, MAX_CONTROL)
}
pub trait VideoTransport {
    fn send_frame(
        &self,
        session: [u8; 16],
        frame: &VideoFrame,
    ) -> impl std::future::Future<Output = Result<bool>> + Send;
}
impl VideoTransport for Connection {
    async fn send_frame(&self, session: [u8; 16], f: &VideoFrame) -> Result<bool> {
        ensure!(f.data.len() <= MAX_FRAME, "encoded frame too large");
        let mtu = self
            .max_datagram_size()
            .context("peer must support QUIC Datagram")?
            .min(MAX_DATAGRAM);
        let overhead = HEADER + 4 + 8 * 4 + 1 + 2 + 2 + 4 + 4;
        ensure!(mtu > overhead, "datagram MTU too small");
        let chunk = (mtu - overhead).min(1100);
        let count = f.data.len().div_ceil(chunk);
        ensure!(count > 0 && count <= 4096, "invalid fragment count");
        // Only four datagrams may wait inside QUIC. Abandon a frame after 35 ms;
        // the receiver expires its incomplete frame, and the caller requests IDR.
        if crate::telemetry::now_us().saturating_sub(f.capture_timestamp) > 80_000 {
            return Ok(false);
        }
        let deadline = tokio::time::Instant::now() + Duration::from_millis(35);
        for (index, data) in f.data.chunks(chunk).enumerate() {
            let v = VideoFragment {
                generation: f.generation,
                frame_id: f.frame_id,
                capture_timestamp: f.capture_timestamp,
                encode_timestamp: f.encode_timestamp,
                send_timestamp: crate::telemetry::now_us(),
                keyframe: f.keyframe,
                index: index as u16,
                count: count as u16,
                total_bytes: f.data.len() as u32,
                data: data.to_vec(),
            };
            match tokio::time::timeout_at(
                deadline,
                self.send_datagram_wait(Packet::new(&v, 0, session).encode().into()),
            )
            .await
            {
                Ok(result) => result?,
                Err(_) => return Ok(false),
            }
        }
        Ok(true)
    }
}

pub async fn write_packet(stream: &mut SendStream, packet: &Packet) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(2), stream.write_all(&packet.encode())).await??;
    Ok(())
}
