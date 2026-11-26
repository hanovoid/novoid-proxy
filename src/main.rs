use clap::Parser;
use novoid_proxy::router::ModelRouter;
use novoid_proxy::server::{build_router, ProxyState};
use std::time::Duration;

#[derive(Parser)]
#[command(
    name = "novoid-proxy",
    version,
    about = "Novoid Proxy - 2-tier smart model router for AI coding CLIs"
)]
struct Cli {
    /// Port to listen on
    #[arg(long, env = "PORT", default_value_t = 8080)]
    port: u16,

    /// Host address to bind
    #[arg(long, env = "HOST", default_value = "0.0.0.0")]
    host: String,

    /// Upstream Novoid Core base URL
    #[arg(long, env = "UPSTREAM_URL", default_value = "http://127.0.0.1:8081")]
    upstream: String,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let router = ModelRouter::new(cli.upstream.clone());

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    // Warm up model groups before/right after bind
    let router_init = router.clone();
    tokio::spawn(async move {
        router_init.refresh_models(false).await;
        let snapshot = router_init.get_groups_snapshot().await;
        println!(
            "[NovoidProxy] Phan nhom model hien tai: {}",
            serde_json::to_string(&snapshot).unwrap_or_default()
        );
    });

    let addr = format!("{}:{}", cli.host, cli.port);
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(err) => {
            eprintln!("[NovoidProxy] Failed to bind {addr}: {err}");
            std::process::exit(1);
        }
    };

    println!(
        "[NovoidProxy] Router v{} dang chay tai http://{addr} -> upstream {}",
        env!("CARGO_PKG_VERSION"),
        cli.upstream
    );

    let app = build_router(ProxyState {
        router,
        shutdown_rx: shutdown_rx.clone(),
    });

    let server = axum::serve(listener, app).with_graceful_shutdown(async move {
        shutdown_signal().await;
        let _ = shutdown_tx.send(true);
    });

    let mut watch = shutdown_rx;
    tokio::select! {
        res = server => {
            if let Err(err) = res {
                eprintln!("[NovoidProxy] Server error: {err}");
            }
        }
        _ = async {
            while !*watch.borrow() {
                if watch.changed().await.is_err() {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(1000)).await;
        } => {}
    }

    println!("[NovoidProxy] Stopped cleanly");
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut sig) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            sig.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    println!("[NovoidProxy] Shutting down...");
}
