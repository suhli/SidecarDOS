use super::{
    SessionState,
    tray::{Action, Ui},
};
use crate::{
    config::Config,
    display,
    encoder::{EncoderSettings, MfEncoder, VideoEncoder},
    input::Injector,
    network::{self, Control, VideoTransport},
    pairing::{self, Identity},
    protocol::*,
    telemetry, topology,
};
use anyhow::{Context, Result, ensure};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc as stdmpsc,
    },
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, watch};

enum Command {
    Connect {
        device: [u8; 16],
        modes: Vec<DisplayMode>,
    },
    Disconnect {
        immediate: bool,
    },
    Input(InputEvent),
    Mode(DisplayModeChange),
    Placement(crate::config::Position),
    Shutdown,
}
enum Event {
    Ready(VideoConfig),
    Failed(String),
}
struct Pipeline {
    commands: stdmpsc::SyncSender<Command>,
    frames: watch::Receiver<Option<Arc<VideoFrame>>>,
    events: mpsc::Receiver<Event>,
    keyframe: Arc<AtomicBool>,
    bitrate: Arc<AtomicU32>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Pipeline {
    fn spawn(config: Config, display_changed: Arc<AtomicBool>) -> Self {
        let (tx, rx) = stdmpsc::sync_channel(256);
        let (frame_tx, frames) = watch::channel(None);
        let (event_tx, events) = mpsc::channel(4);
        let keyframe = Arc::new(AtomicBool::new(true));
        let bitrate = Arc::new(AtomicU32::new(config.video.bitrate));
        let key = keyframe.clone();
        let rate = bitrate.clone();
        let thread = std::thread::spawn(move || {
            let mut driver = None;
            let mut encoder: Option<MfEncoder> = None;
            let mut input: Option<Injector> = None;
            let mut state = SessionState::Idle;
            let mut cfg = config;
            let mut seen = [0; 3];
            let mut mode_request = None;
            let mut reconcile = topology::Reconciler::default();
            let mut generation = 0;
            let mut serial = 0u64;
            let mut encode_rate = 0;
            let mut last_probe = Instant::now();
            let mut last_bounds = None;
            let mut startup = None;
            let mut previous_modes: Vec<DisplayMode> = vec![];
            loop {
                let active = matches!(state, SessionState::Streaming { .. });
                let cmd = rx.recv_timeout(if active {
                    Duration::from_millis(2)
                } else {
                    Duration::from_millis(100)
                });
                let operation = (|| -> Result<bool> {
                    match cmd {
                        Ok(Command::Shutdown) | Err(stdmpsc::RecvTimeoutError::Disconnected) => {
                            return Ok(false);
                        }
                        Ok(Command::Connect { device, modes }) => {
                            ensure!(state.accepts(device), "display reserved for another device");
                            if driver.is_none() {
                                driver = Some(display::Driver::open()?);
                            }
                            let replace_monitor =
                                matches!(state, SessionState::Idle) || modes != previous_modes;
                            if replace_monitor {
                                driver
                                    .as_ref()
                                    .context("driver missing")?
                                    .start(device, &modes)?;
                            }
                            let m = modes.first().context("no display modes")?;
                            previous_modes = modes.clone();
                            mode_request = if replace_monitor {
                                Some((m.width, m.height, m.fps))
                            } else {
                                None
                            };
                            state = SessionState::Streaming { device };
                            encoder = None;
                            input = None;
                            seen = [0; 3];
                            generation = 0;
                            reconcile = topology::Reconciler::default();
                            startup = Some(Instant::now());
                            frame_tx.send_replace(None);
                        }
                        Ok(Command::Disconnect { immediate }) => {
                            encoder = None;
                            input = None;
                            frame_tx.send_replace(None);
                            if immediate {
                                if let Some(d) = &driver {
                                    d.stop()?;
                                }
                                state = SessionState::Idle;
                            } else {
                                state.disconnect(
                                    Instant::now(),
                                    Duration::from_millis(cfg.display.reconnect_grace_ms),
                                );
                            }
                        }
                        Ok(Command::Input(event)) => {
                            if let Some(i) = &mut input {
                                i.event(&event)?;
                            }
                        }
                        Ok(Command::Mode(m)) => {
                            ensure!(
                                (320..=4094).contains(&m.width)
                                    && (320..=4094).contains(&m.height)
                                    && m.width % 2 == 0
                                    && m.height % 2 == 0
                                    && (30..=60).contains(&m.fps),
                                "invalid mode request"
                            );
                            mode_request = Some((m.width, m.height, m.fps));
                            reconcile.changed(Instant::now());
                            key.store(true, Ordering::Relaxed);
                        }
                        Ok(Command::Placement(p)) => {
                            cfg.display.position = p;
                            reconcile.changed(Instant::now());
                        }
                        Err(stdmpsc::RecvTimeoutError::Timeout) => {}
                    }
                    if state.expired(Instant::now()) {
                        if let Some(d) = &driver {
                            d.stop()?;
                        }
                        state = SessionState::Idle;
                    }
                    if !matches!(state, SessionState::Streaming { .. }) {
                        return Ok(true);
                    }
                    if display_changed.swap(false, Ordering::Relaxed) {
                        reconcile.changed(Instant::now());
                    }
                    // This also catches topology changes when a shell suppresses WM_DISPLAYCHANGE.
                    if last_probe.elapsed() > Duration::from_secs(1) {
                        last_probe = Instant::now();
                        let b = topology::current_bounds().ok();
                        if b != last_bounds {
                            last_bounds = b;
                            reconcile.changed(Instant::now());
                        }
                    }
                    if reconcile.ready(Instant::now()) {
                        match topology::reconcile(
                            cfg.display.position,
                            cfg.display.primary,
                            mode_request,
                        ) {
                            Ok((b, matched)) => {
                                if let Some(i) = &mut input {
                                    if i.bounds != b {
                                        i.release_all();
                                        i.bounds = b;
                                    }
                                } else {
                                    input = Some(Injector::new(b)?);
                                }
                                last_bounds = Some(b);
                                mode_request = None;
                                reconcile.result(Instant::now(), matched);
                            }
                            Err(e) => {
                                tracing::warn!(target:"topology",error=%e,"reconcile attempt failed");
                                reconcile.result(Instant::now(), false);
                            }
                        }
                    }
                    let d = driver.as_ref().context("driver missing")?;
                    let status = d.status()?;
                    if status.width == 0 || input.is_none() {
                        ensure!(
                            !startup.is_some_and(|t| t.elapsed() > Duration::from_secs(8)),
                            "virtual display did not become active"
                        );
                        return Ok(true);
                    }
                    if encoder.is_none() || generation != status.generation {
                        encoder = None;
                        seen = [0; 3];
                        generation = status.generation;
                        let settings = EncoderSettings {
                            width: status.width,
                            height: status.height,
                            fps: status.fps.clamp(30, 60),
                            bitrate: rate.load(Ordering::Relaxed),
                        };
                        let (e, names) = MfEncoder::open(status, settings)?;
                        d.surfaces(status.generation, &names)?;
                        encoder = Some(e);
                        encode_rate = settings.bitrate;
                        event_tx
                            .try_send(Event::Ready(VideoConfig {
                                generation,
                                codec: 1,
                                width: status.width,
                                height: status.height,
                                fps: settings.fps,
                                bit_depth: 8,
                                color_space: 1,
                                parameter_sets: vec![],
                            }))
                            .context("pipeline control queue full")?;
                        key.store(true, Ordering::Relaxed);
                        startup = None;
                    }
                    let e = encoder.as_mut().context("encoder missing")?;
                    if key.swap(false, Ordering::Relaxed) {
                        e.request_keyframe()?;
                    }
                    let desired = rate.load(Ordering::Relaxed);
                    if desired != encode_rate {
                        e.reconfigure(EncoderSettings {
                            width: status.width,
                            height: status.height,
                            fps: status.fps.clamp(30, 60),
                            bitrate: desired,
                        })?;
                        encode_rate = desired;
                    }
                    // Drain encoded output before offering fresh input. At most three samples exist inside MFT.
                    for _ in 0..4 {
                        if let Some(mut f) = e.poll()? {
                            serial += 1;
                            f.frame_id = serial;
                            frame_tx.send_replace(Some(Arc::new(f)));
                        } else {
                            break;
                        }
                    }
                    let newest = status
                        .slots
                        .iter()
                        .enumerate()
                        .filter(|(i, s)| s.frame > seen[*i])
                        .max_by_key(|(_, s)| s.frame)
                        .map(|(i, _)| i);
                    // Release stale slots without feeding the encoder (frame 0 selects discard in the bridge).
                    for (i, s) in status.slots.iter().enumerate() {
                        if s.frame > seen[i] {
                            let mut metadata = *s;
                            if Some(i) != newest {
                                metadata.frame = 0;
                            }
                            if e.encode(i, metadata)? {
                                seen[i] = s.frame;
                            }
                        }
                    }
                    Ok(true)
                })();
                match operation {
                    Ok(true) => {}
                    Ok(false) => break,
                    Err(e) => {
                        tracing::error!(target:"display",error=%e,"pipeline stopped");
                        let _ = event_tx.try_send(Event::Failed(e.to_string()));
                        encoder = None;
                        input = None;
                        if let Some(d) = &driver {
                            let _ = d.stop();
                        }
                        driver = None;
                        state = SessionState::Idle;
                        frame_tx.send_replace(None);
                    }
                }
            }
            drop(encoder);
            drop(input);
            if let Some(d) = driver {
                let _ = d.stop();
            }
        });
        Self {
            commands: tx,
            frames,
            events,
            keyframe,
            bitrate,
            thread: Some(thread),
        }
    }
    fn command(&self, cmd: Command) -> Result<()> {
        self.commands
            .try_send(cmd)
            .context("pipeline input queue full or stopped")
    }
}
impl Drop for Pipeline {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

async fn authenticate(
    connection: &quinn::Connection,
    identity: &mut Identity,
    ui: &Ui,
) -> Result<(Control, [u8; 16], String)> {
    let mut control = Control::accept(connection).await?;
    let hello = control.receive().await?.message::<Handshake>()?;
    ensure!(
        hello.client_id.len() == 16 && hello.name.len() <= 128,
        "invalid client identity"
    );
    let id: [u8; 16] = hello.client_id.as_slice().try_into()?;
    let (major, minor) = negotiate(hello.min_major, hello.max_major, hello.minor)?;
    control.send(&ProtocolVersion { major, minor }).await?;
    control
        .send(&HostInfo {
            host_id: identity.host_id.to_vec(),
            name: std::env::var("COMPUTERNAME").unwrap_or_else(|_| "Windows PC".into()),
        })
        .await?;
    ensure!(hello.trusted <= 1, "invalid pairing state");
    let trusted = if hello.trusted == 1 {
        identity.trusted(&id)?
    } else {
        None
    };
    let is_new = trusted.is_none();
    let key = trusted.unwrap_or_else(|| pairing::random::<16>().to_vec());
    let nonce = pairing::random::<32>();
    if is_new {
        ui.pairing(&hello.name, &hex::encode(&key));
    }
    control
        .send(&Pairing {
            phase: if is_new { 0 } else { 1 },
            nonce: nonce.to_vec(),
            proof: vec![],
            secret: vec![],
        })
        .await?;
    let answer = control.receive().await?.message::<Pairing>()?;
    ensure!(
        answer.phase == 2 && answer.nonce == nonce && answer.secret.is_empty(),
        "unexpected pairing response"
    );
    pairing::verify(&key, &identity.cert, &id, &nonce, 1, &answer.proof)?;
    let secret = if is_new {
        pairing::random::<32>().to_vec()
    } else {
        key.clone()
    };
    if is_new {
        identity.trust(&id, &secret)?;
    }
    control
        .send(&Pairing {
            phase: 3,
            nonce: nonce.to_vec(),
            proof: pairing::proof(&key, &identity.cert, &id, &nonce, 2)?,
            secret: if is_new { secret } else { vec![] },
        })
        .await?;
    ui.clear_pairing();
    Ok((control, id, hello.name))
}
struct SessionPreferences<'a> {
    config: &'a mut Config,
    path: &'a std::path::Path,
}
async fn session(
    endpoint: &quinn::Endpoint,
    c: &quinn::Connection,
    mut control: Control,
    id: [u8; 16],
    pipeline: &mut Pipeline,
    actions: &mut mpsc::Receiver<Action>,
    preferences: SessionPreferences<'_>,
) -> Result<bool> {
    let SessionPreferences {
        config,
        path: config_path,
    } = preferences;
    let capabilities = tokio::time::timeout(Duration::from_secs(5), control.receive())
        .await??
        .message::<DisplayCapabilities>()?;
    let mut modes = display::modes(&capabilities, config.video.fps)?;
    let preferred = display::preferred(&modes)
        .context("no supported display mode")?
        .clone();
    modes.sort_by_key(|m| {
        if m.width == preferred.width && m.height == preferred.height {
            0
        } else {
            1
        }
    });
    ensure!(
        c.max_datagram_size().is_some(),
        "iPad did not negotiate QUIC Datagram"
    );
    pipeline.command(Command::Connect { device: id, modes })?;
    let token = pairing::random::<16>();
    control.session = token;
    control
        .send(&SessionStart {
            width: preferred.width,
            height: preferred.height,
            fps: preferred.fps,
        })
        .await?;
    control
        .send(&EncoderCapabilities {
            codecs: 1,
            hardware: 1,
            max_width: 4094,
            max_height: 4094,
        })
        .await?;
    let mut input: Option<quinn::RecvStream> = None;
    let mut abr = telemetry::AdaptiveBitrate::new(
        config.video.bitrate,
        config.video.min_bitrate,
        config.video.max_bitrate,
    );
    let mut last_id = 0;
    let mut last_key = Instant::now() - Duration::from_secs(1);
    // Keep one read future alive: cancelling read_exact in select would lose partial framing.
    let (packet_tx, mut packets) = mpsc::channel::<Result<Packet>>(32);
    let mut recv = control.recv;
    let reader = tokio::spawn(async move {
        loop {
            let p = network::read_packet(&mut recv).await;
            let failed = p.is_err();
            if packet_tx.send(p).await.is_err() || failed {
                break;
            }
        }
    });
    struct Abort(tokio::task::JoinHandle<()>);
    impl Drop for Abort {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let _reader = Abort(reader);
    let mut guard = ReplayGuard::default();
    let mut sequence = 10u64;
    let mut input_task = None;
    let outcome=async{
 loop{tokio::select!{
  p=packets.recv()=>{
   let p=p.context("control stream ended")??;ensure!(p.session==token,"wrong session");guard.accept(p.sequence)?;
   match p.kind{
    SessionStop::KIND=>return Ok(p.message::<SessionStop>()?.reason==0),
    KeyframeRequest::KIND=>{p.message::<KeyframeRequest>()?;if last_key.elapsed()>Duration::from_millis(200){pipeline.keyframe.store(true,Ordering::Relaxed);last_key=Instant::now();}}
    DisplayModeChange::KIND=>pipeline.command(Command::Mode(p.message()?))?,
    Statistics::KIND=>{
     let s=p.message::<Statistics>()?;ensure!(s.loss_ppm<=1_000_000&&s.fps>=0.&&s.fps<=240.,"invalid statistics");
     tracing::debug!(target:"network",fps=s.fps,rtt_us=s.rtt_us,loss_ppm=s.loss_ppm,decode_us=s.decode_us,render_us=s.render_us,latency_us=s.estimated_latency_us,"client statistics");
     if config.video.adaptive && let Some(rate)=abr.update(&s,Instant::now()){pipeline.bitrate.store(rate,Ordering::Relaxed);}
    }
    Ping::KIND=>{let ping=p.message::<Ping>()?;sequence+=1;let pong=Pong{client_timestamp:ping.client_timestamp,host_receive:telemetry::now_us(),host_send:telemetry::now_us()};network::write_packet(&mut control.send, &Packet::new(&pong,sequence,token)).await?;}
    _=>anyhow::bail!("unexpected session control message"),
   }
  }
  stream=c.accept_uni(),if input.is_none()&&input_task.is_none()=>{
   input=Some(stream?);
   let mut stream=input.take().context("input stream absent")?;let commands=pipeline.commands.clone();let input_connection=c.clone();
   input_task=Some(Abort(tokio::spawn(async move{
    let mut input_guard=ReplayGuard::default();let result=async{loop{let p=network::read_packet(&mut stream).await?;ensure!(p.session==token,"input session mismatch");input_guard.accept(p.sequence)?;commands.try_send(Command::Input(p.message()?)).context("input queue overflow")?;}#[allow(unreachable_code)]Ok::<(),anyhow::Error>(())}.await;
    if let Err(e)=result{tracing::warn!(target:"input",error=%e,"input stream stopped");input_connection.close(3u8.into(),b"input stream stopped");let _=commands.try_send(Command::Disconnect{immediate:false});}
   })));
  }
  changed=pipeline.frames.changed()=>{
   changed?;let frame=pipeline.frames.borrow_and_update().clone();
   if let Some(f)=frame{
    if f.frame_id>last_id+1{pipeline.keyframe.store(true,Ordering::Relaxed);}
    last_id=f.frame_id;
    if !c.send_frame(token,&f).await?{pipeline.keyframe.store(true,Ordering::Relaxed);}
   }
  }
  e=pipeline.events.recv()=>match e.context("capture worker stopped")?{
   Event::Ready(v)=>{sequence+=1;network::write_packet(&mut control.send, &Packet::new(&v,sequence,token)).await?;}
   Event::Failed(e)=>anyhow::bail!("display pipeline: {e}"),
  },
  action=actions.recv()=>match action{
   Some(Action::Disconnect)=>return Ok(true),
   Some(Action::Exit)|None=>anyhow::bail!("host exit requested"),
   Some(Action::Position(p))=>{config.display.position=p;config.save(config_path)?;pipeline.command(Command::Placement(p))?;}
   Some(Action::Quality(p))=>{config.video.adaptive=p==0;let rate=match p{1=>6_000_000,2=>20_000_000,_=>12_000_000};config.video.bitrate=rate;config.save(config_path)?;pipeline.bitrate.store(rate,Ordering::Relaxed);}
  },
  incoming=endpoint.accept()=>{if let Some(i)=incoming{i.refuse();}},
  reason=c.closed()=>anyhow::bail!("QUIC closed: {reason}"),
 }}
 }.await;
    drop(input_task);
    outcome
}
pub async fn run(
    mut config: Config,
    mut identity: Identity,
    ui: Ui,
    mut actions: mpsc::Receiver<Action>,
    config_path: std::path::PathBuf,
) -> Result<()> {
    let endpoint = network::server(config.port, identity.cert.clone(), identity.key.clone())?;
    let name = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "Windows PC".into());
    let _advertisement =
        crate::discovery::Advertisement::start(&identity.host_id, &name, config.port)?;
    let mut pipeline = Pipeline::spawn(config.clone(), ui.display_changed.clone());
    let mut reserve: Option<([u8; 16], Instant)> = None;
    tracing::info!(target:"network",port=config.port,"SidecarDOS listening");
    while !ui.exit.load(Ordering::Relaxed) {
        tokio::select! {
         incoming=endpoint.accept()=>{
          let Some(incoming)=incoming else{break};
          let connect=tokio::time::timeout(Duration::from_secs(10),incoming).await;
          let c=match connect{Ok(Ok(c))=>c,_=>continue};
          let auth=tokio::select! { auth=tokio::time::timeout(Duration::from_secs(90),authenticate(&c,&mut identity,&ui))=>auth, _=wait_exit(&ui)=>{c.close(0u8.into(),b"shutdown");break;} };
          ui.clear_pairing();
          match auth{
           Ok(Ok((control,id,name)))=>{
            if reserve.is_some_and(|(device,until)|device!=id&&Instant::now()<until){c.close(1u8.into(),b"display reserved during reconnect");continue}
            ui.status(&format!("Connected: {name}"));
            // Discard prior-session status and frames before attaching the new receiver.
            while pipeline.events.try_recv().is_ok(){}pipeline.frames.borrow_and_update();
            let result=session(&endpoint,&c,control,id,&mut pipeline,&mut actions,SessionPreferences { config:&mut config, path:&config_path }).await;
            let immediate=matches!(result,Ok(true))||ui.exit.load(Ordering::Relaxed);
            if let Err(e)=&result{tracing::warn!(target:"network",error=%e,"session ended");}
            pipeline.command(Command::Disconnect{immediate})?;
            reserve=if immediate{None}else{Some((id,Instant::now()+Duration::from_millis(config.display.reconnect_grace_ms)))};
            ui.status("SidecarDOS — Available");c.close(0u8.into(),b"session ended");
           }
           _=>{tracing::warn!(target:"pairing","authentication rejected or timed out");c.close(2u8.into(),b"authentication failed");tokio::time::sleep(Duration::from_secs(1)).await;}
          }
         }
         action=actions.recv()=>match action{
          Some(Action::Exit)|None=>break,
          Some(Action::Position(p))=>{config.display.position=p;config.save(&config_path)?;pipeline.command(Command::Placement(p))?;}
          Some(Action::Quality(p))=>{config.video.adaptive=p==0;config.video.bitrate=match p{1=>6_000_000,2=>20_000_000,_=>12_000_000};config.save(&config_path)?;pipeline.bitrate.store(config.video.bitrate,Ordering::Relaxed);}
          Some(Action::Disconnect)=>{reserve=None;pipeline.command(Command::Disconnect{immediate:true})?;}
         }
        }
    }
    endpoint.close(0u8.into(), b"host shutdown");
    drop(pipeline);
    Ok(())
}

async fn wait_exit(ui: &Ui) {
    while !ui.exit.load(Ordering::Relaxed) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
