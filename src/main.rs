#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Container health probe: distroless has no shell/curl, so the binary probes itself.
    if std::env::args().any(|a| a == "--healthcheck") {
        let port = std::env::var("PVHUB_HTTP_PORT").unwrap_or_else(|_| "8080".into());
        let url = format!("http://127.0.0.1:{port}/health");
        let ok = reqwest::get(&url)
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false);
        std::process::exit(if ok { 0 } else { 1 });
    }
    pv_hub::run().await
}
