use crate::catalog::MetricDef;
use crate::config::Config;
use serde_json::{json, Value};

/// Single JSON state topic all sensors read via `value_template`.
pub fn state_topic(node: &str) -> String {
    format!("pvhub/{node}/state")
}

/// Availability (LWT) topic: `online` / `offline`.
pub fn status_topic(node: &str) -> String {
    format!("pvhub/{node}/status")
}

/// HA MQTT-discovery config topic for one metric.
pub fn config_topic(prefix: &str, node: &str, id: &str) -> String {
    format!("{prefix}/sensor/{node}/{id}/config")
}

/// Map a metric to Home Assistant metadata:
/// `(unit_of_measurement, device_class, state_class)`.
///
/// Catalog units are normalized to HA-accepted units (e.g. `W/m2` -> `W/m²`,
/// `degC` -> `°C`) because HA ignores a `device_class` whose unit is incompatible.
/// A few metrics are special-cased by `id`.
pub fn ha_meta(d: &MetricDef) -> (Option<&'static str>, Option<&'static str>, &'static str) {
    match d.id {
        "rel_humidity" => return (Some("%"), Some("humidity"), "measurement"),
        "poll_errors_total" => return (None, None, "total_increasing"),
        "provider_ok" | "is_daytime" => return (None, None, "measurement"),
        "last_update_epoch" => return (Some("s"), None, "measurement"),
        _ => {}
    }
    match d.unit {
        "W/m2" => (Some("W/m²"), Some("irradiance"), "measurement"),
        "degC" => (Some("°C"), Some("temperature"), "measurement"),
        "m/s" => (Some("m/s"), Some("wind_speed"), "measurement"),
        "hPa" => (Some("hPa"), Some("atmospheric_pressure"), "measurement"),
        "mm" => (Some("mm"), Some("precipitation"), "measurement"),
        "%" => (Some("%"), None, "measurement"),
        "deg" => (Some("°"), None, "measurement"),
        "s" => (Some("s"), Some("duration"), "measurement"),
        _ => (None, None, "measurement"),
    }
}

/// Build the HA MQTT-discovery config payload for one metric.
pub fn discovery_payload(d: &MetricDef, cfg: &Config) -> Value {
    let node = cfg.ha_node_id.as_str();
    let (unit, device_class, state_class) = ha_meta(d);
    let mut obj = serde_json::Map::new();
    obj.insert("name".into(), json!(d.label));
    obj.insert("unique_id".into(), json!(format!("pvhub_{node}_{}", d.id)));
    obj.insert("object_id".into(), json!(format!("pvhub_{}", d.id)));
    obj.insert("state_topic".into(), json!(state_topic(node)));
    // The state topic carries the full state_json document, which nests metric
    // values under a top-level `metrics` key — hence `value_json.metrics.<id>.value`.
    obj.insert(
        "value_template".into(),
        json!(format!("{{{{ value_json.metrics.{}.value }}}}", d.id)),
    );
    if let Some(u) = unit {
        obj.insert("unit_of_measurement".into(), json!(u));
    }
    if let Some(dc) = device_class {
        obj.insert("device_class".into(), json!(dc));
    }
    obj.insert("state_class".into(), json!(state_class));
    obj.insert("availability_topic".into(), json!(status_topic(node)));
    obj.insert("payload_available".into(), json!("online"));
    obj.insert("payload_not_available".into(), json!("offline"));
    obj.insert(
        "device".into(),
        json!({
            "identifiers": [node],
            "name": cfg.site_name,
            "manufacturer": "pv-hub",
            "model": "Solarimetro",
        }),
    );
    Value::Object(obj)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{catalog, def_for};
    use crate::model::Metric;
    use std::collections::{HashMap, HashSet};

    fn cfg() -> Config {
        let mut m = HashMap::new();
        m.insert("PVHUB_LATITUDE".into(), "45.4642".into());
        m.insert("PVHUB_LONGITUDE".into(), "9.19".into());
        Config::from_map(&m).unwrap()
    }

    #[test]
    fn topics_use_node_and_prefix() {
        assert_eq!(state_topic("pvhub"), "pvhub/pvhub/state");
        assert_eq!(status_topic("roof"), "pvhub/roof/status");
        assert_eq!(
            config_topic("homeassistant", "pvhub", "ghi"),
            "homeassistant/sensor/pvhub/ghi/config"
        );
    }

    #[test]
    fn unit_mapping_normalizes_and_classes() {
        assert_eq!(ha_meta(def_for(Metric::Ghi).unwrap()), (Some("W/m²"), Some("irradiance"), "measurement"));
        assert_eq!(ha_meta(def_for(Metric::AmbientTemp).unwrap()), (Some("°C"), Some("temperature"), "measurement"));
        assert_eq!(ha_meta(def_for(Metric::WindSpeed).unwrap()), (Some("m/s"), Some("wind_speed"), "measurement"));
        assert_eq!(ha_meta(def_for(Metric::SurfacePressure).unwrap()), (Some("hPa"), Some("atmospheric_pressure"), "measurement"));
        assert_eq!(ha_meta(def_for(Metric::Precipitation).unwrap()), (Some("mm"), Some("precipitation"), "measurement"));
        assert_eq!(ha_meta(def_for(Metric::SunAzimuth).unwrap()), (Some("°"), None, "measurement"));
        assert_eq!(ha_meta(def_for(Metric::DataAge).unwrap()), (Some("s"), Some("duration"), "measurement"));
    }

    #[test]
    fn special_cases_by_id() {
        assert_eq!(ha_meta(def_for(Metric::RelHumidity).unwrap()), (Some("%"), Some("humidity"), "measurement"));
        assert_eq!(ha_meta(def_for(Metric::CloudCover).unwrap()), (Some("%"), None, "measurement"));
        assert_eq!(ha_meta(def_for(Metric::PollErrorsTotal).unwrap()), (None, None, "total_increasing"));
        assert_eq!(ha_meta(def_for(Metric::ProviderOk).unwrap()), (None, None, "measurement"));
        assert_eq!(ha_meta(def_for(Metric::IsDaytime).unwrap()), (None, None, "measurement"));
        assert_eq!(ha_meta(def_for(Metric::LastUpdateEpoch).unwrap()), (Some("s"), None, "measurement"));
    }

    #[test]
    fn empty_unit_metric_has_no_unit() {
        // clearsky_index has an empty catalog unit
        assert_eq!(ha_meta(def_for(Metric::ClearskyIndex).unwrap()), (None, None, "measurement"));
        let p = discovery_payload(def_for(Metric::ClearskyIndex).unwrap(), &cfg());
        assert!(p.get("unit_of_measurement").is_none());
        assert!(p.get("device_class").is_none());
    }

    #[test]
    fn one_entry_per_metric_with_unique_ids() {
        let cfg = cfg();
        let mut ids = HashSet::new();
        for d in catalog() {
            let uid = discovery_payload(d, &cfg)["unique_id"].as_str().unwrap().to_string();
            assert!(ids.insert(uid), "duplicate unique_id for {}", d.id);
        }
        assert_eq!(ids.len(), catalog().len());
    }

    #[test]
    fn ghi_payload_shape() {
        let p = discovery_payload(def_for(Metric::Ghi).unwrap(), &cfg());
        assert_eq!(p["state_topic"], "pvhub/pvhub/state");
        assert_eq!(p["value_template"], "{{ value_json.metrics.ghi.value }}");
        assert_eq!(p["unit_of_measurement"], "W/m²");
        assert_eq!(p["device_class"], "irradiance");
        assert_eq!(p["state_class"], "measurement");
        assert_eq!(p["unique_id"], "pvhub_pvhub_ghi");
        assert_eq!(p["object_id"], "pvhub_ghi");
        assert_eq!(p["availability_topic"], "pvhub/pvhub/status");
        assert_eq!(p["payload_not_available"], "offline");
        assert_eq!(p["device"]["identifiers"][0], "pvhub");
        assert_eq!(p["device"]["model"], "Solarimetro");
        assert_eq!(p["device"]["manufacturer"], "pv-hub");
    }
}
