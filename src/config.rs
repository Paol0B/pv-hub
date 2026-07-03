use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranspositionModel {
    HayDavies,
    Perez,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellTempModel {
    Faiman,
    Noct,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WordOrder {
    Abcd,
    Cdab,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub site_name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub elevation_m: Option<f64>,
    pub tilt_deg: f64,
    pub azimuth_deg: f64,
    pub albedo: f64,
    pub transposition: TranspositionModel,
    pub celltemp: CellTempModel,
    pub faiman_u0: f64,
    pub faiman_u1: f64,
    pub noct: f64,
    pub poll_interval_s: u64,
    pub solar_interval_s: u64,
    pub provider: String,
    pub openmeteo_base_url: String,
    pub openmeteo_api_key: Option<String>,
    pub http_bind: String,
    pub http_port: u16,
    pub modbus_enable: bool,
    pub modbus_bind: String,
    pub modbus_port: u16,
    pub modbus_unit_id: u8,
    pub modbus_word_order: WordOrder,
    pub modbus_holding_mirror: bool,
    pub ha_enable: bool,
    pub ha_mqtt_host: String,
    pub ha_mqtt_port: u16,
    pub ha_mqtt_username: Option<String>,
    pub ha_mqtt_password: Option<String>,
    pub ha_mqtt_client_id: String,
    pub ha_discovery_prefix: String,
    pub ha_node_id: String,
    pub ha_publish_interval_s: u64,
    pub default_theme: String,
    pub log_level: String,
}

impl Config {
    pub fn from_env() -> Result<Config, String> {
        let map: HashMap<String, String> = std::env::vars().collect();
        Config::from_map(&map)
    }

    pub fn from_map(env: &HashMap<String, String>) -> Result<Config, String> {
        let req_f64 = |k: &str| -> Result<f64, String> {
            env.get(k)
                .ok_or_else(|| format!("missing required env {k}"))?
                .parse::<f64>()
                .map_err(|e| format!("{k}: {e}"))
        };
        let f64_or = |k: &str, d: f64| -> Result<f64, String> {
            match env.get(k) {
                Some(v) => v.parse::<f64>().map_err(|e| format!("{k}: {e}")),
                None => Ok(d),
            }
        };
        let u64_or = |k: &str, d: u64| -> Result<u64, String> {
            match env.get(k) {
                Some(v) => v.parse::<u64>().map_err(|e| format!("{k}: {e}")),
                None => Ok(d),
            }
        };
        let u16_or = |k: &str, d: u16| -> Result<u16, String> {
            match env.get(k) {
                Some(v) => v.parse::<u16>().map_err(|e| format!("{k}: {e}")),
                None => Ok(d),
            }
        };
        let u8_or = |k: &str, d: u8| -> Result<u8, String> {
            match env.get(k) {
                Some(v) => v.parse::<u8>().map_err(|e| format!("{k}: {e}")),
                None => Ok(d),
            }
        };
        let bool_or = |k: &str, d: bool| -> bool {
            env.get(k)
                .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
                .unwrap_or(d)
        };
        let str_or = |k: &str, d: &str| -> String { env.get(k).cloned().unwrap_or_else(|| d.to_string()) };

        let latitude = req_f64("PVHUB_LATITUDE")?;
        if !(-90.0..=90.0).contains(&latitude) {
            return Err("PVHUB_LATITUDE out of range [-90,90]".into());
        }
        let longitude = req_f64("PVHUB_LONGITUDE")?;
        if !(-180.0..=180.0).contains(&longitude) {
            return Err("PVHUB_LONGITUDE out of range [-180,180]".into());
        }

        let transposition = match str_or("PVHUB_TRANSPOSITION", "hay_davies").as_str() {
            "hay_davies" => TranspositionModel::HayDavies,
            "perez" => TranspositionModel::Perez,
            other => return Err(format!("PVHUB_TRANSPOSITION invalid: {other}")),
        };
        let celltemp = match str_or("PVHUB_CELLTEMP", "faiman").as_str() {
            "faiman" => CellTempModel::Faiman,
            "noct" => CellTempModel::Noct,
            other => return Err(format!("PVHUB_CELLTEMP invalid: {other}")),
        };
        let modbus_word_order = match str_or("PVHUB_MODBUS_WORD_ORDER", "abcd").as_str() {
            "abcd" => WordOrder::Abcd,
            "cdab" => WordOrder::Cdab,
            other => return Err(format!("PVHUB_MODBUS_WORD_ORDER invalid: {other}")),
        };

        Ok(Config {
            site_name: str_or("PVHUB_SITE_NAME", "pv-hub"),
            latitude,
            longitude,
            elevation_m: env.get("PVHUB_ELEVATION_M").map(|v| v.parse().unwrap_or(0.0)),
            tilt_deg: f64_or("PVHUB_TILT_DEG", 30.0)?,
            azimuth_deg: f64_or("PVHUB_AZIMUTH_DEG", 180.0)?,
            albedo: f64_or("PVHUB_ALBEDO", 0.20)?,
            transposition,
            celltemp,
            faiman_u0: f64_or("PVHUB_CELLTEMP_U0", 25.0)?,
            faiman_u1: f64_or("PVHUB_CELLTEMP_U1", 6.84)?,
            noct: f64_or("PVHUB_CELLTEMP_NOCT", 45.0)?,
            poll_interval_s: u64_or("PVHUB_POLL_INTERVAL_S", 600)?,
            solar_interval_s: u64_or("PVHUB_SOLAR_INTERVAL_S", 60)?,
            provider: str_or("PVHUB_PROVIDER", "openmeteo"),
            openmeteo_base_url: str_or("PVHUB_OPENMETEO_BASE_URL", "https://api.open-meteo.com/v1/forecast"),
            openmeteo_api_key: env.get("PVHUB_OPENMETEO_API_KEY").cloned(),
            http_bind: str_or("PVHUB_HTTP_BIND", "0.0.0.0"),
            http_port: u16_or("PVHUB_HTTP_PORT", 8080)?,
            modbus_enable: bool_or("PVHUB_MODBUS_ENABLE", true),
            modbus_bind: str_or("PVHUB_MODBUS_BIND", "0.0.0.0"),
            modbus_port: u16_or("PVHUB_MODBUS_PORT", 502)?,
            modbus_unit_id: u8_or("PVHUB_MODBUS_UNIT_ID", 1)?,
            modbus_word_order,
            modbus_holding_mirror: bool_or("PVHUB_MODBUS_HOLDING_MIRROR", true),
            ha_enable: bool_or("PVHUB_HA_ENABLE", false),
            ha_mqtt_host: str_or("PVHUB_HA_MQTT_HOST", "localhost"),
            ha_mqtt_port: u16_or("PVHUB_HA_MQTT_PORT", 1883)?,
            ha_mqtt_username: env.get("PVHUB_HA_MQTT_USERNAME").cloned(),
            ha_mqtt_password: env.get("PVHUB_HA_MQTT_PASSWORD").cloned(),
            ha_mqtt_client_id: str_or("PVHUB_HA_MQTT_CLIENT_ID", "pvhub"),
            ha_discovery_prefix: str_or("PVHUB_HA_DISCOVERY_PREFIX", "homeassistant"),
            ha_node_id: str_or("PVHUB_HA_NODE_ID", "pvhub"),
            ha_publish_interval_s: u64_or("PVHUB_HA_PUBLISH_INTERVAL_S", 30)?,
            default_theme: str_or("PVHUB_DEFAULT_THEME", "auto"),
            log_level: str_or("PVHUB_LOG_LEVEL", "info"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert("PVHUB_LATITUDE".into(), "45.4642".into());
        m.insert("PVHUB_LONGITUDE".into(), "9.19".into());
        m
    }

    #[test]
    fn parses_required_and_defaults() {
        let c = Config::from_map(&base()).unwrap();
        assert_eq!(c.latitude, 45.4642);
        assert_eq!(c.longitude, 9.19);
        assert_eq!(c.tilt_deg, 30.0);
        assert_eq!(c.azimuth_deg, 180.0);
        assert_eq!(c.albedo, 0.20);
        assert_eq!(c.modbus_port, 502);
        assert_eq!(c.modbus_word_order, WordOrder::Abcd);
        assert!(c.modbus_holding_mirror);
        assert_eq!(c.transposition, TranspositionModel::HayDavies);
        assert_eq!(c.celltemp, CellTempModel::Faiman);
    }

    #[test]
    fn missing_latitude_is_error() {
        let mut m = base();
        m.remove("PVHUB_LATITUDE");
        let err = Config::from_map(&m).unwrap_err();
        assert!(err.contains("PVHUB_LATITUDE"), "got: {err}");
    }

    #[test]
    fn overrides_are_applied() {
        let mut m = base();
        m.insert("PVHUB_TILT_DEG".into(), "15".into());
        m.insert("PVHUB_MODBUS_WORD_ORDER".into(), "cdab".into());
        m.insert("PVHUB_CELLTEMP".into(), "noct".into());
        let c = Config::from_map(&m).unwrap();
        assert_eq!(c.tilt_deg, 15.0);
        assert_eq!(c.modbus_word_order, WordOrder::Cdab);
        assert_eq!(c.celltemp, CellTempModel::Noct);
    }

    #[test]
    fn out_of_range_latitude_is_error() {
        let mut m = base();
        m.insert("PVHUB_LATITUDE".into(), "120".into());
        assert!(Config::from_map(&m).is_err());
    }

    #[test]
    fn ha_defaults_and_overrides() {
        let c = Config::from_map(&base()).unwrap();
        assert!(!c.ha_enable);
        assert_eq!(c.ha_mqtt_host, "localhost");
        assert_eq!(c.ha_mqtt_port, 1883);
        assert_eq!(c.ha_mqtt_client_id, "pvhub");
        assert_eq!(c.ha_discovery_prefix, "homeassistant");
        assert_eq!(c.ha_node_id, "pvhub");
        assert_eq!(c.ha_publish_interval_s, 30);
        assert!(c.ha_mqtt_username.is_none());
        assert!(c.ha_mqtt_password.is_none());

        let mut m = base();
        m.insert("PVHUB_HA_ENABLE".into(), "true".into());
        m.insert("PVHUB_HA_MQTT_HOST".into(), "broker.local".into());
        m.insert("PVHUB_HA_MQTT_PORT".into(), "8883".into());
        m.insert("PVHUB_HA_MQTT_USERNAME".into(), "user".into());
        m.insert("PVHUB_HA_MQTT_PASSWORD".into(), "secret".into());
        m.insert("PVHUB_HA_NODE_ID".into(), "roof".into());
        m.insert("PVHUB_HA_PUBLISH_INTERVAL_S".into(), "15".into());
        let c = Config::from_map(&m).unwrap();
        assert!(c.ha_enable);
        assert_eq!(c.ha_mqtt_host, "broker.local");
        assert_eq!(c.ha_mqtt_port, 8883);
        assert_eq!(c.ha_mqtt_username.as_deref(), Some("user"));
        assert_eq!(c.ha_mqtt_password.as_deref(), Some("secret"));
        assert_eq!(c.ha_node_id, "roof");
        assert_eq!(c.ha_publish_interval_s, 15);
    }
}
