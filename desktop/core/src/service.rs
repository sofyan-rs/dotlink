//! Starting and stopping the whole service: control socket, clipboard watch, mDNS, HTTPS.

use crate::events::UiRequest;
use crate::server::{self, App};
use crate::{config, ipc, platform, tls};
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

/// Runs until Ctrl-C, SIGTERM or `dotlink quit`. With `ui`, the service can bring up the
/// window it runs with (a second launch of the app). `ready` gets the app once the control
/// socket is up, or the startup error.
pub async fn run(
    ui: Option<mpsc::UnboundedSender<UiRequest>>,
    ready: Option<oneshot::Sender<Result<Arc<App>, String>>>,
) -> Result<()> {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let (app, identity) = match start(ui).await {
        Ok(started) => {
            if let Some(ready) = ready {
                let _ = ready.send(Ok(started.0.clone()));
            }
            started
        }
        Err(e) => {
            if let Some(ready) = ready {
                let _ = ready.send(Err(format!("{e:#}")));
            }
            return Err(e);
        }
    };
    let result = serve(app, identity).await;
    ipc::cleanup();
    result
}

async fn start(ui: Option<mpsc::UnboundedSender<UiRequest>>) -> Result<(Arc<App>, tls::Identity)> {
    let mut config = config::Config::load()?;
    let mut state = config::State::load()?;
    if !config.clipboard_sync {
        // Clipboard sync used to be one switch for all phones; it's per device now.
        state.clipboard_off_everywhere();
        state.save()?;
        config.clipboard_sync = true;
        config.save()?;
    }
    let identity = tls::load_or_create()?;
    let app = App::new(config, state, identity.fingerprint.clone());
    if let Some(ui) = ui {
        app.attach_ui(ui);
    }

    // Bind the control socket first: it also guards against running twice.
    let ipc_task = tokio::spawn(ipc::serve(app.clone()));
    tokio::task::yield_now().await;
    if ipc_task.is_finished() {
        ipc_task.await??;
    }
    Ok((app, identity))
}

async fn serve(app: Arc<App>, identity: tls::Identity) -> Result<()> {
    // Always watched; each device's clipboard sync is checked per change so it applies live.
    tokio::spawn(platform::clipboard::watch(app.clone()));
    tokio::spawn(crate::usb::watch(app.clone()));
    crate::peer::start_all(&app);
    tokio::spawn(app.clone().watch_battery());
    let _mdns = advertise(&app).map_err(|e| tracing::warn!("mDNS disabled: {e}")).ok();

    let pem = axum_server::tls_rustls::RustlsConfig::from_pem(identity.cert_pem, identity.key_pem).await?;
    // HTTP/1.1 only. Over HTTP/2, the phone's OkHttp had large downloads reset halfway
    // ("stream was reset: PROTOCOL_ERROR"); one file per connection is all we need anyway.
    let mut server = (*pem.get_inner()).clone();
    server.alpn_protocols = vec![b"http/1.1".to_vec()];
    let tls = axum_server::tls_rustls::RustlsConfig::from_config(Arc::new(server));
    let addr = SocketAddr::from(([0, 0, 0, 0], app.port));
    tracing::info!("listening on {addr} (fingerprint {}…)", &identity.fingerprint[..16]);
    let handle = axum_server::Handle::new();
    let shutdown = handle.clone();
    let stop = app.clone();
    tokio::spawn(async move {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate() => {}
            _ = stop.shutdown.notified() => {}
        }
        shutdown.shutdown();
    });
    // TCP_NODELAY: Remote moves, Shell keystrokes and control messages are tiny packets that
    // Nagle's algorithm would otherwise hold back for an ACK.
    let acceptor = axum_server::tls_rustls::RustlsAcceptor::new(tls).acceptor(axum_server::accept::NoDelayAcceptor);
    axum_server::bind(addr)
        .acceptor(acceptor)
        .handle(handle)
        .serve(server::router(app).into_make_service_with_connect_info::<SocketAddr>())
        .await?;
    Ok(())
}

#[cfg(unix)]
async fn terminate() {
    match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
        Ok(mut term) => drop(term.recv().await),
        Err(_) => std::future::pending().await,
    }
}

/// The console window closing, logoff or shutdown (sent to processes with a console; a GUI
/// process gets WM_ENDSESSION instead and simply exits).
#[cfg(windows)]
async fn terminate() {
    use tokio::signal::windows::{ctrl_close, ctrl_logoff, ctrl_shutdown};
    match (ctrl_close(), ctrl_logoff(), ctrl_shutdown()) {
        (Ok(mut close), Ok(mut logoff), Ok(mut shutdown)) => tokio::select! {
            _ = close.recv() => {}
            _ = logoff.recv() => {}
            _ = shutdown.recv() => {}
        },
        _ => std::future::pending().await,
    }
}

/// Announces `_dotlink._tcp` so the phone can find the PC after its IP changes.
fn advertise(app: &Arc<App>) -> Result<mdns_sd::ServiceDaemon> {
    let mdns = mdns_sd::ServiceDaemon::new()?;
    let id = app.server_id();
    let host = format!("dotlink-{id}.local.");
    // `n`: the name a PC joining with a code lists this one under.
    let name = app.name();
    let props = [("id", id.as_str()), ("fp", &app.fingerprint[..16]), ("n", name.as_str())];
    // Two instances on one machine (`DOTLINK_CONFIG_DIR`) need different instance names.
    let instance = format!("{}{}", app.hostname, config::instance_suffix());
    let info = mdns_sd::ServiceInfo::new("_dotlink._tcp.local.", &instance, &host, "", app.port, &props[..])?
        .enable_addr_auto();
    mdns.register(info)?;
    Ok(mdns)
}
