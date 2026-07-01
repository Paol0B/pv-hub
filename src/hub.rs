use crate::model::{Metric, SolarState};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

/// Shared central state. Providers apply samples; sinks read snapshots and
/// subscribe to change notifications.
#[derive(Clone)]
pub struct Hub {
    state: Arc<RwLock<SolarState>>,
    tx: broadcast::Sender<()>,
}

impl Hub {
    pub fn new() -> Hub {
        let (tx, _rx) = broadcast::channel(16);
        Hub { state: Arc::new(RwLock::new(SolarState::default())), tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.tx.subscribe()
    }

    /// Apply metric samples; optionally mark a weather update time and provider status.
    pub async fn apply(
        &self,
        samples: &[(Metric, f64)],
        weather_update: Option<DateTime<Utc>>,
        provider_ok: Option<bool>,
    ) {
        {
            let mut s = self.state.write().await;
            for (m, v) in samples {
                s.set(*m, *v);
            }
            if let Some(t) = weather_update {
                s.last_weather_update = Some(t);
            }
            if let Some(ok) = provider_ok {
                s.provider_ok = ok;
            }
        }
        let _ = self.tx.send(());
    }

    pub async fn record_poll_error(&self) {
        {
            let mut s = self.state.write().await;
            s.poll_errors_total += 1;
            s.provider_ok = false;
        }
        let _ = self.tx.send(());
    }

    /// Read-only snapshot for sinks.
    pub async fn snapshot(&self) -> SolarState {
        self.state.read().await.clone()
    }
}

impl Default for Hub {
    fn default() -> Self {
        Hub::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[tokio::test]
    async fn apply_updates_and_notifies() {
        let hub = Hub::new();
        let mut rx = hub.subscribe();
        let t = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        hub.apply(&[(Metric::Ghi, 812.0)], Some(t), Some(true)).await;
        assert!(rx.try_recv().is_ok());
        let snap = hub.snapshot().await;
        assert_eq!(snap.value(Metric::Ghi, t), Some(812.0));
        assert!(snap.provider_ok);
    }

    #[tokio::test]
    async fn poll_error_increments_and_clears_ok() {
        let hub = Hub::new();
        hub.apply(&[], None, Some(true)).await;
        hub.record_poll_error().await;
        let now = Utc::now();
        let snap = hub.snapshot().await;
        assert_eq!(snap.value(Metric::PollErrorsTotal, now), Some(1.0));
        assert_eq!(snap.value(Metric::ProviderOk, now), Some(0.0));
    }
}
