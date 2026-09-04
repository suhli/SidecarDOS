use anyhow::Result;
use sidecardos_host::{network, protocol::*};
use std::{sync::Arc, time::Duration};
#[tokio::test]
async fn encrypted_stream_and_datagram_share_a_connection() -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let cert = rcgen::generate_simple_self_signed(vec!["sidecardos.local".into()])?;
    let server = network::server(0, cert.cert.der().to_vec(), cert.key_pair.serialize_der())?;
    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert.cert.der().clone())?;
    let mut tls = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    tls.alpn_protocols = vec![network::ALPN.to_vec()];
    let config = quinn::ClientConfig::new(Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(tls)?,
    ));
    let mut client = quinn::Endpoint::client("127.0.0.1:0".parse()?)?;
    client.set_default_client_config(config);
    let connect = client.connect(
        ([127, 0, 0, 1], server.local_addr()?.port()).into(),
        "sidecardos.local",
    )?;
    let (a, b) = tokio::join!(connect, async {
        server
            .accept()
            .await
            .ok_or_else(|| anyhow::anyhow!("endpoint closed"))?
            .await
            .map_err(anyhow::Error::from)
    });
    let a = a?;
    let b: quinn::Connection = b?;
    let (send, mut recv) = a.open_bi().await?;
    let mut send = send;
    let data = Packet::new(
        &Ping {
            client_timestamp: 1234,
        },
        1,
        [0; 16],
    )
    .encode();
    // Exercise a receive operation spanning many partial writes.
    let writer = tokio::spawn(async move {
        for chunk in data.chunks(3) {
            send.write_all(chunk).await?;
        }
        send.finish()?;
        Ok::<_, anyhow::Error>(())
    });
    let (mut host_send, mut host_recv) = b.accept_bi().await?;
    let packet = tokio::time::timeout(Duration::from_secs(2), network::read_packet(&mut host_recv))
        .await??;
    assert_eq!(packet.message::<Ping>()?.client_timestamp, 1234);
    writer.await??;
    network::write_packet(
        &mut host_send,
        &Packet::new(
            &Pong {
                client_timestamp: 1234,
                host_receive: 1235,
                host_send: 1236,
            },
            1,
            [0; 16],
        ),
    )
    .await?;
    assert_eq!(
        network::read_packet(&mut recv)
            .await?
            .message::<Pong>()?
            .host_send,
        1236
    );
    use network::VideoTransport;
    let frame = VideoFrame {
        generation: 1,
        frame_id: 1,
        capture_timestamp: sidecardos_host::telemetry::now_us(),
        encode_timestamp: 2,
        send_timestamp: 3,
        keyframe: 1,
        data: vec![42; 2200],
    };
    assert!(b.send_frame([7; 16], &frame).await?);
    let mut pieces = Vec::new();
    for _ in 0..2 {
        let d = tokio::time::timeout(Duration::from_secs(2), a.read_datagram()).await??;
        assert!(d.len() <= 1200);
        let p = Packet::decode(&d, 1200)?;
        assert_eq!(p.session, [7; 16]);
        pieces.push(p.message::<VideoFragment>()?);
    }
    pieces.sort_by_key(|f| f.index);
    let reassembled = pieces
        .iter()
        .flat_map(|p| p.data.iter().copied())
        .collect::<Vec<_>>();
    assert_eq!(reassembled, frame.data);
    a.close(0u8.into(), b"test complete");
    server.close(0u8.into(), b"test complete");
    client.close(0u8.into(), b"test complete");
    Ok(())
}
