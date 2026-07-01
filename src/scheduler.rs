use crate::config::Config;
use crate::hub::Hub;
use crate::providers::openmeteo::OpenMeteoProvider;
use crate::providers::Provider;
use crate::solar::SolarEngine;
use chrono::Utc;
use std::time::Duration;

/// Periodically fetch weather from the provider and apply to the hub.
pub async fn weather_loop(cfg: Config, hub: Hub) {
    let provider = OpenMeteoProvider::new(&cfg);
    let mut backoff = 1u64;
    loop {
        match provider.poll().await {
            Ok(samples) => {
                hub.apply(&samples, Some(Utc::now()), Some(true)).await;
                tracing::info!("weather updated ({} samples)", samples.len());
                backoff = 1;
                tokio::time::sleep(Duration::from_secs(cfg.poll_interval_s)).await;
            }
            Err(e) => {
                hub.record_poll_error().await;
                tracing::warn!("weather poll failed: {e}; retrying in {backoff}s");
                tokio::time::sleep(Duration::from_secs(backoff)).await;
                backoff = (backoff * 2).min(cfg.poll_interval_s.max(2));
            }
        }
    }
}

/// Periodically recompute solar-derived metrics (no network).
pub async fn solar_loop(cfg: Config, hub: Hub) {
    hub.apply(&SolarEngine::site_metrics(&cfg), None, None).await;
    loop {
        let snap = hub.snapshot().await;
        let samples = SolarEngine::compute(&cfg, Utc::now(), &snap.weather_inputs());
        hub.apply(&samples, None, None).await;
        tokio::time::sleep(Duration::from_secs(cfg.solar_interval_s)).await;
    }
}
