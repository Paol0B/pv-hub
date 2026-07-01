#[tokio::main]
async fn main() -> anyhow::Result<()> {
    pv_hub::run().await
}
