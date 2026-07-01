use crate::config::Config;
use crate::model::Metric;
use crate::providers::Provider;
use serde::Deserialize;

pub struct OpenMeteoProvider {
    base_url: String,
    api_key: Option<String>,
    lat: f64,
    lon: f64,
    tilt: f64,
    azimuth: f64,
    client: reqwest::Client,
}

#[derive(Deserialize)]
struct OmResponse {
    current: OmCurrent,
}

#[derive(Deserialize)]
struct OmCurrent {
    temperature_2m: Option<f64>,
    relative_humidity_2m: Option<f64>,
    wind_speed_10m: Option<f64>,
    wind_direction_10m: Option<f64>,
    cloud_cover: Option<f64>,
    precipitation: Option<f64>,
    surface_pressure: Option<f64>,
    shortwave_radiation: Option<f64>,
    direct_normal_irradiance: Option<f64>,
    diffuse_radiation: Option<f64>,
    global_tilted_irradiance: Option<f64>,
}

impl OpenMeteoProvider {
    pub fn new(cfg: &Config) -> OpenMeteoProvider {
        OpenMeteoProvider {
            base_url: cfg.openmeteo_base_url.clone(),
            api_key: cfg.openmeteo_api_key.clone(),
            lat: cfg.latitude,
            lon: cfg.longitude,
            tilt: cfg.tilt_deg,
            azimuth: cfg.azimuth_deg,
            client: reqwest::Client::new(),
        }
    }

    /// Build the request URL. Open-Meteo azimuth is 0=S, +E; we convert from
    /// our North-referenced azimuth (180=S) via (az - 180).
    pub fn url(&self) -> String {
        let current = "temperature_2m,relative_humidity_2m,wind_speed_10m,wind_direction_10m,\
cloud_cover,precipitation,surface_pressure,shortwave_radiation,direct_normal_irradiance,\
diffuse_radiation,global_tilted_irradiance";
        let om_azimuth = self.azimuth - 180.0;
        let mut url = format!(
            "{}?latitude={}&longitude={}&current={}&tilt={}&azimuth={}&wind_speed_unit=ms&timezone=UTC",
            self.base_url, self.lat, self.lon, current, self.tilt, om_azimuth
        );
        if let Some(k) = &self.api_key {
            url.push_str(&format!("&apikey={k}"));
        }
        url
    }

    /// Parse a raw response body into samples.
    pub fn parse(body: &str) -> anyhow::Result<Vec<(Metric, f64)>> {
        let r: OmResponse = serde_json::from_str(body)?;
        let c = r.current;
        let mut out = Vec::new();
        let mut push = |m: Metric, v: Option<f64>| {
            if let Some(v) = v {
                out.push((m, v));
            }
        };
        push(Metric::AmbientTemp, c.temperature_2m);
        push(Metric::RelHumidity, c.relative_humidity_2m);
        push(Metric::WindSpeed, c.wind_speed_10m);
        push(Metric::WindDirection, c.wind_direction_10m);
        push(Metric::CloudCover, c.cloud_cover);
        push(Metric::Precipitation, c.precipitation);
        push(Metric::SurfacePressure, c.surface_pressure);
        push(Metric::Ghi, c.shortwave_radiation);
        push(Metric::Dni, c.direct_normal_irradiance);
        push(Metric::Dhi, c.diffuse_radiation);
        push(Metric::PoaProvider, c.global_tilted_irradiance);
        Ok(out)
    }
}

#[async_trait::async_trait]
impl Provider for OpenMeteoProvider {
    async fn poll(&self) -> anyhow::Result<Vec<(Metric, f64)>> {
        let body = self
            .client
            .get(self.url())
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        OpenMeteoProvider::parse(&body)
    }
    fn name(&self) -> &'static str {
        "openmeteo"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn cfg() -> Config {
        let mut m = HashMap::new();
        m.insert("PVHUB_LATITUDE".into(), "45.4642".into());
        m.insert("PVHUB_LONGITUDE".into(), "9.19".into());
        Config::from_map(&m).unwrap()
    }

    #[test]
    fn url_contains_key_params() {
        let p = OpenMeteoProvider::new(&cfg());
        let u = p.url();
        assert!(u.contains("latitude=45.4642"));
        assert!(u.contains("shortwave_radiation"));
        assert!(u.contains("global_tilted_irradiance"));
        assert!(u.contains("tilt=30"));
        assert!(u.contains("azimuth=0"));
    }

    #[test]
    fn parses_fixture() {
        let body = include_str!("../../tests/fixtures/openmeteo.json");
        let map: HashMap<_, _> = OpenMeteoProvider::parse(body).unwrap().into_iter().collect();
        assert_eq!(map.get(&Metric::Ghi), Some(&812.0));
        assert_eq!(map.get(&Metric::PoaProvider), Some(&918.0));
        assert_eq!(map.get(&Metric::AmbientTemp), Some(&27.4));
    }

    #[tokio::test]
    async fn poll_against_mock_server() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        let body = include_str!("../../tests/fixtures/openmeteo.json");
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;

        let mut m = HashMap::new();
        m.insert("PVHUB_LATITUDE".into(), "45.4642".into());
        m.insert("PVHUB_LONGITUDE".into(), "9.19".into());
        m.insert("PVHUB_OPENMETEO_BASE_URL".into(), server.uri());
        let c = Config::from_map(&m).unwrap();
        let p = OpenMeteoProvider::new(&c);
        let samples = p.poll().await.unwrap();
        assert!(samples.iter().any(|(mtc, _)| *mtc == Metric::Ghi));
    }
}
