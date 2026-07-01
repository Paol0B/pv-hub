pub mod openmeteo;

use crate::model::Metric;

/// A provider fetches remote data and returns metric samples.
#[async_trait::async_trait]
pub trait Provider: Send + Sync {
    async fn poll(&self) -> anyhow::Result<Vec<(Metric, f64)>>;
    fn name(&self) -> &'static str;
}
