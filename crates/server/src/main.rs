//! Rustflare server: camera + mount + tracking + capture, controlled from a web page.

mod api;
mod config;
mod engine;
mod files;
mod preview;
mod security;
mod sessionlog;
mod status;
mod storage;
mod sunmodel;

use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// Address to listen on (plain HTTP). Without RUSTFLARE_PASSWORD there is no authentication: trusted networks only.
    #[arg(long, default_value = "0.0.0.0:8080", env = "RUSTFLARE_BIND")]
    bind: String,
    /// Default capture directory. A location chosen in the web UI (saved in --config-file) takes precedence.
    #[arg(long, default_value = "data", env = "RUSTFLARE_DATA_DIR")]
    data_dir: PathBuf,
    /// Where settings chosen in the web UI are saved.
    #[arg(long, default_value = "rustflare.json", env = "RUSTFLARE_CONFIG")]
    config_file: PathBuf,
    /// Password for the web UI and API (HTTP Basic, any user name). Empty: no password.
    #[arg(long, default_value = "", env = "RUSTFLARE_PASSWORD", hide_env_values = true)]
    password: String,
    /// Folder with server.crt, server.key (and ca.crt) made by `rustflare-tls`. When present, HTTPS is served too.
    #[arg(long, default_value = "tls", env = "RUSTFLARE_TLS_DIR")]
    tls_dir: PathBuf,
    /// HTTPS listen address.
    #[arg(long, default_value = "0.0.0.0:8443", env = "RUSTFLARE_TLS_BIND")]
    tls_bind: String,
    /// Built web UI (web/dist).
    #[arg(long, default_value = "web/dist", env = "RUSTFLARE_WEB_DIR")]
    web_dir: PathBuf,
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info,zbus=warn,tracing=warn")).init();
    // Frames are several MB. With glibc's defaults each one is a fresh mmap that is page-faulted in and unmapped again
    // (hundreds of MB/s of kernel work). Keeping big blocks on the heap lets freed frame buffers be reused as they are.
    #[cfg(target_os = "linux")]
    unsafe {
        libc::mallopt(libc::M_MMAP_THRESHOLD, 128 * 1024 * 1024);
        libc::mallopt(libc::M_TRIM_THRESHOLD, 512 * 1024 * 1024);
    }
    let args = Args::parse();
    if !args.web_dir.join("index.html").exists() {
        log::warn!("no web UI at {} (build it with `npm run build` in web/); the API still works", args.web_dir.display());
    }

    let storage = std::sync::Arc::new(storage::Storage::load(args.config_file, args.data_dir));
    log::info!("captures go to {}", storage.dir().display());
    let stretch = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let demand = std::sync::Arc::new(preview::Demand::default());
    let (handle, engine_thread) = engine::spawn(storage.clone(), stretch.clone(), demand.clone());
    let app = api::router(api::AppState { engine: handle, storage, stretch, demand }, args.web_dir);

    let app = if args.password.is_empty() {
        app
    } else {
        log::info!("password protection is on");
        app.layer(axum::middleware::from_fn_with_state(std::sync::Arc::new(args.password.clone()), security::require_password))
    };
    let shutdown = async {
        let _ = tokio::signal::ctrl_c().await;
        log::info!("shutting down");
    };
    match security::tls_files(&args.tls_dir) {
        Some(tls) => {
            let _ = rustls::crypto::ring::default_provider().install_default();
            let cfg = axum_server::tls_rustls::RustlsConfig::from_pem_file(&tls.cert, &tls.key).await?;
            let https_addr: std::net::SocketAddr = args.tls_bind.parse().map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("--tls-bind: {e}")))?;
            log::info!("Rustflare listening on https://{https_addr} (plain http on {} only serves the CA and redirects)", args.bind);
            let handle = axum_server::Handle::new();
            let h2 = handle.clone();
            tokio::spawn(async move {
                shutdown.await;
                h2.graceful_shutdown(Some(std::time::Duration::from_secs(5)));
            });
            let http = tokio::net::TcpListener::bind(&args.bind).await?;
            let redirect = security::http_redirector(tls.ca.clone(), https_addr.port());
            tokio::spawn(async move { let _ = axum::serve(http, redirect).await; });
            axum_server::bind_rustls(https_addr, cfg).handle(handle).serve(app.into_make_service()).await?;
        }
        None => {
            let listener = tokio::net::TcpListener::bind(&args.bind).await?;
            log::info!("Rustflare listening on http://{}", listener.local_addr()?);
            axum::serve(listener, app).with_graceful_shutdown(shutdown).await?;
        }
    }
    // `app` (and the engine handle inside it) is dropped here; the engine stops the mount and exits.
    let _ = tokio::task::spawn_blocking(move || engine_thread.join()).await;
    Ok(())
}
