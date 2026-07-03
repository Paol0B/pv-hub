//! pv-hub — Solarimetro microservice core.

use anyhow::Result;

pub mod catalog;
pub mod config;
pub mod hub;
pub mod model;
pub mod providers;
pub mod scheduler;
pub mod sinks;
pub mod solar;

pub fn init_tracing(level: &str) {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));
    let _ = fmt().with_env_filter(filter).try_init();
}

pub async fn run() -> Result<()> {
    let cfg = config::Config::from_env().map_err(|e| anyhow::anyhow!(e))?;
    init_tracing(&cfg.log_level);
    tracing::info!(
        "pv-hub {} starting — site '{}' at {},{}",
        env!("CARGO_PKG_VERSION"),
        cfg.site_name,
        cfg.latitude,
        cfg.longitude
    );

    let hub = hub::Hub::new();

    // Seed site metrics + provider-down status so Modbus has data at t=0.
    hub.apply(&solar::SolarEngine::site_metrics(&cfg), None, Some(false)).await;

    let mut tasks = Vec::new();
    tasks.push(tokio::spawn(scheduler::solar_loop(cfg.clone(), hub.clone())));
    tasks.push(tokio::spawn(scheduler::weather_loop(cfg.clone(), hub.clone())));
    if cfg.modbus_enable {
        let c = cfg.clone();
        let h = hub.clone();
        tasks.push(tokio::spawn(async move {
            if let Err(e) = sinks::modbus::server::serve(c, h).await {
                tracing::error!("modbus server error: {e}");
            }
        }));
    }
    if cfg.ha_enable {
        let c = cfg.clone();
        let h = hub.clone();
        tasks.push(tokio::spawn(async move {
            if let Err(e) = sinks::homeassistant::serve(c, h).await {
                tracing::error!("home assistant sink error: {e}");
            }
        }));
    }
    {
        let c = cfg.clone();
        let h = hub.clone();
        tasks.push(tokio::spawn(async move {
            if let Err(e) = sinks::http::serve(c, h).await {
                tracing::error!("http server error: {e}");
            }
        }));
    }

    tokio::signal::ctrl_c().await.ok();
    tracing::info!("shutdown signal received; stopping");
    for t in tasks {
        t.abort();
    }
    Ok(())
}
