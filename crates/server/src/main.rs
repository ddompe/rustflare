//! Rustflare server: camera + mount + tracking + capture, controlled from a web page.

mod api;
mod config;
mod engine;
mod files;
mod preview;
mod status;
mod storage;

use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// Address to listen on. There is no authentication: only expose this on a trusted network.
    #[arg(long, default_value = "0.0.0.0:8080", env = "RUSTFLARE_BIND")]
    bind: String,
    /// Default capture directory. A location chosen in the web UI (saved in --config-file) takes precedence.
    #[arg(long, default_value = "data", env = "RUSTFLARE_DATA_DIR")]
    data_dir: PathBuf,
    /// Where settings chosen in the web UI are saved.
    #[arg(long, default_value = "rustflare.json", env = "RUSTFLARE_CONFIG")]
    config_file: PathBuf,
    /// Built web UI (web/dist).
    #[arg(long, default_value = "web/dist", env = "RUSTFLARE_WEB_DIR")]
    web_dir: PathBuf,
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let args = Args::parse();
    if !args.web_dir.join("index.html").exists() {
        log::warn!("no web UI at {} (build it with `npm run build` in web/); the API still works", args.web_dir.display());
    }

    let storage = std::sync::Arc::new(storage::Storage::load(args.config_file, args.data_dir));
    log::info!("captures go to {}", storage.dir().display());
    let stretch = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (handle, engine_thread) = engine::spawn(storage.clone(), stretch.clone());
    let app = api::router(api::AppState { engine: handle, storage, stretch }, args.web_dir);

    let listener = tokio::net::TcpListener::bind(&args.bind).await?;
    log::info!("Rustflare listening on http://{}", listener.local_addr()?);
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
            log::info!("shutting down");
        })
        .await?;
    // `app` (and the engine handle inside it) is dropped here; the engine stops the mount and exits.
    let _ = tokio::task::spawn_blocking(move || engine_thread.join()).await;
    Ok(())
}
