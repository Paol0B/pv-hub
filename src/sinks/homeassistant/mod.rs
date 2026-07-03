pub mod discovery;

use crate::catalog::catalog;
use crate::config::Config;
use crate::hub::Hub;
use crate::sinks::http::api::state_json;
use discovery::{config_topic, discovery_payload, state_topic, status_topic};
use rumqttc::{AsyncClient, Event, LastWill, MqttOptions, Packet, QoS};
use std::time::Duration;

/// Request-channel capacity. Must exceed the connect burst (one discovery
/// message per metric + availability + state) so publishing on ConnAck never
/// blocks before the event loop can drain it.
const REQUEST_CAP: usize = 256;

/// Publish a retained discovery config for every metric in the catalog.
async fn publish_discovery(client: &AsyncClient, cfg: &Config) {
    for d in catalog() {
        let topic = config_topic(&cfg.ha_discovery_prefix, &cfg.ha_node_id, d.id);
        let payload = discovery_payload(d, cfg).to_string();
        if let Err(e) = client.publish(topic, QoS::AtLeastOnce, true, payload).await {
            tracing::warn!("ha discovery publish failed for {}: {e}", d.id);
        }
    }
}

/// Publish the current state document (retained) to the single state topic.
async fn publish_state(client: &AsyncClient, cfg: &Config, hub: &Hub) {
    let payload = state_json(hub, cfg).await.to_string();
    if let Err(e) = client
        .publish(state_topic(&cfg.ha_node_id), QoS::AtLeastOnce, true, payload)
        .await
    {
        tracing::warn!("ha state publish failed: {e}");
    }
}

pub async fn serve(cfg: Config, hub: Hub) -> anyhow::Result<()> {
    let status = status_topic(&cfg.ha_node_id);

    let mut opts = MqttOptions::new(
        cfg.ha_mqtt_client_id.as_str(),
        cfg.ha_mqtt_host.as_str(),
        cfg.ha_mqtt_port,
    );
    opts.set_keep_alive(Duration::from_secs(30));
    opts.set_last_will(LastWill::new(
        status.clone(),
        "offline",
        QoS::AtLeastOnce,
        true,
    ));
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

    // Publisher task: push state on every hub change and at least every
    // `ha_publish_interval_s`. Enqueues into the shared request channel; the
    // event loop below actually sends them.
    {
        let client = client.clone();
        let hub = hub.clone();
        let cfg = cfg.clone();
        tokio::spawn(async move {
            let mut rx = hub.subscribe();
            let mut ticker = tokio::time::interval(Duration::from_secs(cfg.ha_publish_interval_s));
            loop {
                tokio::select! {
                    _ = rx.recv() => {},
                    _ = ticker.tick() => {},
                }
                publish_state(&client, &cfg, &hub).await;
            }
        });
    }

    // Connection loop: drive the event loop; on each (re)connect republish the
    // retained discovery configs + availability=online + current state. rumqttc
    // reconnects automatically on subsequent polls; back off on hard errors.
    let mut backoff = 1u64;
    loop {
        match eventloop.poll().await {
            Ok(Event::Incoming(Packet::ConnAck(_))) => {
                tracing::info!("home assistant mqtt connected");
                publish_discovery(&client, &cfg).await;
                if let Err(e) = client
                    .publish(status.clone(), QoS::AtLeastOnce, true, "online")
                    .await
                {
                    tracing::warn!("ha availability publish failed: {e}");
                }
                publish_state(&client, &cfg, &hub).await;
                backoff = 1;
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!("ha mqtt event loop error: {e}; reconnecting in {backoff}s");
                tokio::time::sleep(Duration::from_secs(backoff)).await;
                backoff = (backoff * 2).min(60);
            }
        }
    }
}
