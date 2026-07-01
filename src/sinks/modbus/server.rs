use crate::config::Config;
use crate::hub::Hub;
use crate::sinks::modbus::{build_bank, frame::handle_adu};
use chrono::Utc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Run the Modbus TCP slave until the process ends. One task per connection.
pub async fn serve(cfg: Config, hub: Hub) -> anyhow::Result<()> {
    let addr = format!("{}:{}", cfg.modbus_bind, cfg.modbus_port);
    let listener = TcpListener::bind(&addr).await?;
    tracing::info!("modbus TCP slave listening on {addr}");
    loop {
        let (sock, peer) = listener.accept().await?;
        let cfg = cfg.clone();
        let hub = hub.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_conn(sock, &cfg, &hub).await {
                tracing::debug!("modbus conn {peer} ended: {e}");
            }
        });
    }
}

async fn handle_conn(mut sock: TcpStream, cfg: &Config, hub: &Hub) -> anyhow::Result<()> {
    let mut header = [0u8; 7];
    loop {
        if sock.read_exact(&mut header).await.is_err() {
            return Ok(()); // client closed
        }
        let len = u16::from_be_bytes([header[4], header[5]]) as usize;
        if len == 0 || len > 253 {
            return Ok(());
        }
        // Read the remaining (len - 1) PDU bytes (len counts unit id, already in header[6]).
        let mut pdu = vec![0u8; len - 1];
        sock.read_exact(&mut pdu).await?;

        let mut adu = Vec::with_capacity(7 + pdu.len());
        adu.extend_from_slice(&header);
        adu.extend_from_slice(&pdu);

        let snap = hub.snapshot().await;
        let bank = build_bank(&snap, Utc::now(), cfg.modbus_word_order);
        let resp = handle_adu(&bank, cfg.modbus_holding_mirror, cfg.modbus_unit_id, &adu);
        if !resp.is_empty() {
            sock.write_all(&resp).await?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WordOrder;
    use crate::model::Metric;
    use crate::sinks::modbus::f32_from_words;
    use std::collections::HashMap;

    #[tokio::test]
    async fn end_to_end_read_over_tcp() {
        let mut m = HashMap::new();
        m.insert("PVHUB_LATITUDE".into(), "45.0".into());
        m.insert("PVHUB_LONGITUDE".into(), "9.0".into());
        let mut cfg = Config::from_map(&m).unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        cfg.modbus_port = port;

        let hub = Hub::new();
        hub.apply(&[(Metric::Ghi, 812.0)], Some(Utc::now()), Some(true)).await;

        let cfg2 = cfg.clone();
        let hub2 = hub.clone();
        tokio::spawn(async move {
            let (sock, _) = listener.accept().await.unwrap();
            handle_conn(sock, &cfg2, &hub2).await.unwrap();
        });

        let mut client = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        // Read input registers 0..2 (GHI at register 0).
        let req = [0u8, 1, 0, 0, 0, 6, 1, 0x04, 0, 0, 0, 2];
        client.write_all(&req).await.unwrap();
        let mut resp = [0u8; 13];
        client.read_exact(&mut resp).await.unwrap();
        let ghi = f32_from_words(
            u16::from_be_bytes([resp[9], resp[10]]),
            u16::from_be_bytes([resp[11], resp[12]]),
            WordOrder::Abcd,
        );
        assert!((ghi - 812.0).abs() < 1e-2, "ghi {ghi}");
    }
}
