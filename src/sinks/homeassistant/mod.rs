pub mod discovery;

use crate::catalog::catalog;
use crate::config::Config;
use crate::hub::Hub;
use crate::sinks::http::api::state_json;
use discovery::{config_topic, discovery_payload, state_topic, status_topic};
use rumqttc::{AsyncClient, Event, LastWill, MqttOptions, Packet, QoS};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Request-channel capacity. Publishes use `try_publish` (never block), so this
/// only needs to comfortably hold the connect burst (one discovery message per
/// metric + availability + state); anything beyond it is dropped, not queued.
const REQUEST_CAP: usize = 256;

/// Non-blocking retained publish. Every HA topic is retained and republished on
/// reconnect, so dropping a message when the channel is full is safe — and it
/// keeps the poll loop (the only task that drains the channel) from ever blocking.
fn emit(client: &AsyncClient, topic: String, payload: impl Into<Vec<u8>>, what: &str) {
    if let Err(e) = client.try_publish(topic, QoS::AtLeastOnce, true, payload) {
        tracing::debug!("ha publish dropped ({what}): {e}");
    }
}

/// Publish a retained discovery config for every metric in the catalog.
fn publish_discovery(client: &AsyncClient, cfg: &Config) {
    for d in catalog() {
        let topic = config_topic(&cfg.ha_discovery_prefix, &cfg.ha_node_id, d.id);
        emit(client, topic, discovery_payload(d, cfg).to_string(), "discovery");
    }
}

/// Publish the current state document (retained) to the single state topic.
async fn publish_state(client: &AsyncClient, cfg: &Config, hub: &Hub) {
    let payload = state_json(hub, cfg).await.to_string();
    emit(client, state_topic(&cfg.ha_node_id), payload, "state");
}

pub async fn serve(cfg: Config, hub: Hub) -> anyhow::Result<()> {
    let status = status_topic(&cfg.ha_node_id);

    let mut opts = MqttOptions::new(
        cfg.ha_mqtt_client_id.as_str(),
        cfg.ha_mqtt_host.as_str(),
        cfg.ha_mqtt_port,
    );
    opts.set_keep_alive(Duration::from_secs(30));
    opts.set_last_will(LastWill::new(status.clone(), "offline", QoS::AtLeastOnce, true));
    if let Some(user) = &cfg.ha_mqtt_username {
        opts.set_credentials(user.clone(), cfg.ha_mqtt_password.clone().unwrap_or_default());
    }

    let (client, mut eventloop) = AsyncClient::new(opts, REQUEST_CAP);
    tracing::info!(
        "home assistant sink -> mqtt {}:{} (node '{}')",
        cfg.ha_mqtt_host,
        cfg.ha_mqtt_port,
        cfg.ha_node_id
    );

    // Shared connection flag: the publisher only enqueues while connected, so the
    // request channel cannot accumulate during a broker outage.
    let connected = Arc::new(AtomicBool::new(false));

    // Publisher task: push state on every hub change and at least every
    // `ha_publish_interval_s` (min 1s), but only while connected.
    {
        let client = client.clone();
        let hub = hub.clone();
        let cfg = cfg.clone();
        let connected = connected.clone();
        tokio::spawn(async move {
            let mut rx = hub.subscribe();
            let period = Duration::from_secs(cfg.ha_publish_interval_s.max(1));
            let mut ticker = tokio::time::interval(period);
            loop {
                tokio::select! {
                    _ = rx.recv() => {},
                    _ = ticker.tick() => {},
                }
                if connected.load(Ordering::Relaxed) {
                    publish_state(&client, &cfg, &hub).await;
                }
            }
        });
    }

    // Connection loop: drive the event loop. On each (re)connect republish the
    // retained discovery configs + availability=online + current state. Publishes
    // here use `emit`/try_publish so this — the only task that drains the channel —
    // can never block on it. rumqttc reconnects automatically on subsequent polls.
    let mut backoff = 1u64;
    loop {
        match eventloop.poll().await {
            Ok(Event::Incoming(Packet::ConnAck(_))) => {
                tracing::info!("home assistant mqtt connected");
                connected.store(true, Ordering::Relaxed);
                publish_discovery(&client, &cfg);
                emit(&client, status.clone(), "online", "availability");
                publish_state(&client, &cfg, &hub).await;
                backoff = 1;
            }
            Ok(_) => {}
            Err(e) => {
                connected.store(false, Ordering::Relaxed);
                tracing::warn!("ha mqtt event loop error: {e}; reconnecting in {backoff}s");
                tokio::time::sleep(Duration::from_secs(backoff)).await;
                backoff = (backoff * 2).min(60);
            }
        }
    }
}
