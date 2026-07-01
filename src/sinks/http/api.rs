use crate::catalog::catalog;
use crate::config::Config;
use crate::hub::Hub;
use crate::sinks::http::AppState;
use axum::extract::State;
use axum::Json;
use chrono::Utc;
use serde_json::{json, Value};

/// Build the full state document from the catalog + current snapshot.
pub async fn state_json(hub: &Hub, cfg: &Config) -> Value {
    let now = Utc::now();
    let snap = hub.snapshot().await;
    let mut metrics = serde_json::Map::new();
    for d in catalog() {
        metrics.insert(
            d.id.to_string(),
            json!({
                "value": snap.value(d.metric, now),
                "unit": d.unit,
                "label": d.label,
                "category": d.category,
            }),
        );
    }
    json!({
        "site": {
            "name": cfg.site_name,
            "latitude": cfg.latitude,
            "longitude": cfg.longitude,
            "tilt": cfg.tilt_deg,
            "azimuth": cfg.azimuth_deg,
            "albedo": cfg.albedo,
            "default_theme": cfg.default_theme,
        },
        "provider": { "name": cfg.provider, "ok": snap.provider_ok },
        "timestamp": now.timestamp(),
        "metrics": Value::Object(metrics),
    })
}

pub fn catalog_json() -> Value {
    let items: Vec<Value> = catalog()
        .iter()
        .map(|d| {
            json!({
                "id": d.id, "label": d.label, "unit": d.unit,
                "category": d.category, "register": d.register,
            })
        })
        .collect();
    json!({ "metrics": items })
}

pub async fn state_handler(State(s): State<AppState>) -> Json<Value> {
    Json(state_json(&s.hub, &s.cfg).await)
}

pub async fn catalog_handler() -> Json<Value> {
    Json(catalog_json())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Metric;
    use std::collections::HashMap;

    fn cfg() -> Config {
        let mut m = HashMap::new();
        m.insert("PVHUB_LATITUDE".into(), "45.4642".into());
        m.insert("PVHUB_LONGITUDE".into(), "9.19".into());
        Config::from_map(&m).unwrap()
    }

    #[tokio::test]
    async fn state_json_has_metrics_and_site() {
        let hub = Hub::new();
        hub.apply(&[(Metric::Ghi, 812.0)], Some(Utc::now()), Some(true)).await;
        let v = state_json(&hub, &cfg()).await;
        assert_eq!(v["metrics"]["ghi"]["value"], 812.0);
        assert_eq!(v["metrics"]["ghi"]["unit"], "W/m2");
        assert_eq!(v["site"]["latitude"], 45.4642);
        assert_eq!(v["provider"]["ok"], true);
    }

    #[test]
    fn catalog_json_lists_registers() {
        let v = catalog_json();
        assert!(v["metrics"].as_array().unwrap().len() >= 32);
    }
}
