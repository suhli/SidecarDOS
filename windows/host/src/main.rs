#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#[cfg(windows)]
fn main() {
    if let Err(e) = run() {
        tracing::error!(error=%e,"SidecarDOS stopped");
        unsafe {
            use windows::{
                Win32::UI::WindowsAndMessaging::*,
                core::{PCWSTR, w},
            };
            let message: Vec<u16> = format!("{e:#}").encode_utf16().chain([0]).collect();
            MessageBoxW(
                None,
                PCWSTR(message.as_ptr()),
                w!("SidecarDOS"),
                MB_OK | MB_ICONERROR,
            );
        }
    }
}
#[cfg(windows)]
fn run() -> anyhow::Result<()> {
    use sidecardos_host::{
        app::{runtime, tray},
        config, pairing,
    };
    use std::sync::atomic::Ordering;
    use windows::{
        Win32::{
            Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError},
            System::Threading::CreateMutexW,
            UI::HiDpi::{
                DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
            },
        },
        core::w,
    };
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let mutex = unsafe { CreateMutexW(None, false, w!("Local\\SidecarDOS.Host"))? };
    struct MutexGuard(windows::Win32::Foundation::HANDLE);
    impl Drop for MutexGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
    let _guard = MutexGuard(mutex);
    anyhow::ensure!(
        unsafe { GetLastError() } != ERROR_ALREADY_EXISTS,
        "SidecarDOS is already running"
    );
    let dir = config::directory()?;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("host.log"))?;
    let env =
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());
    tracing_subscriber::fmt()
        .with_env_filter(env)
        .with_ansi(false)
        .with_writer(std::sync::Mutex::new(log))
        .init();
    let path = dir.join("config.toml");
    let config = config::Config::load(&path)?;
    if !path.exists() {
        config.save(&path)?;
    }
    let mut identity = pairing::Identity::load(&dir.join("identity.dpapi"))?;
    if std::env::args().any(|a| a == "--forget-devices") {
        identity.forget_all()?;
        return Ok(());
    }
    let ui = tray::Ui::default();
    let background_ui = ui.clone();
    let (tx, rx) = tokio::sync::mpsc::channel(32);
    let background_path = path.clone();
    let background = std::thread::spawn(move || {
        let result = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(anyhow::Error::from)
            .and_then(|rt| {
                rt.block_on(runtime::run(
                    config,
                    identity,
                    background_ui.clone(),
                    rx,
                    background_path,
                ))
            });
        if let Err(e) = result {
            tracing::error!(error=%e,"host background loop failed");
            background_ui.exit.store(true, Ordering::Relaxed);
        }
    });
    let result = tray::run(ui.clone(), tx.clone(), &path);
    ui.exit.store(true, Ordering::Relaxed);
    let _ = tx.blocking_send(tray::Action::Exit);
    let _ = background.join();
    result
}
#[cfg(not(windows))]
fn main() {
    eprintln!("SidecarDOS Host requires Windows 11 x64.");
}
