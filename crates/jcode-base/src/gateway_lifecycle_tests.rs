use super::*;

#[tokio::test]
async fn one_shot_reply_drains_before_websocket_close() -> Result<()> {
    tokio::time::timeout(Duration::from_secs(5), async {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let (mut backend, bridge) = crate::transport::stream_pair()?;
        let relay = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
            bridge_websocket(ws, bridge, "lifecycle test".into()).await;
        });
        let (mut client, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws")).await?;
        client.send(Message::Text("{\"type\":\"list_sessions\",\"id\":1}".into())).await?;
        let mut request = String::new();
        BufReader::new(&mut backend).read_line(&mut request).await?;
        assert!(request.contains("list_sessions"));
        // A large valid JSON reply exercises buffering, unlike an empty list.
        let reply = format!("{{\"type\":\"sessions_list\",\"id\":1,\"padding\":\"{}\"}}", "x".repeat(128 * 1024));
        backend.write_all(b"{\"type\":\"ack\",\"id\":1}\n").await?;
        backend.write_all(reply.as_bytes()).await?;
        backend.write_all(b"\n").await?;
        backend.shutdown().await?;
        drop(backend);
        // Mobile readers and their Pong writes can lag behind the backend EOF.
        tokio::time::sleep(Duration::from_millis(50)).await;
        let mut replies = Vec::new();
        let mut closed = false;
        while let Some(frame) = client.next().await {
            match frame? {
                Message::Text(text) => replies.push(text),
                Message::Ping(_) => client.flush().await?,
                Message::Close(frame) => {
                    assert_eq!(frame.map(|f| f.code), Some(tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Normal));
                    client.flush().await?;
                    closed = true;
                    break;
                }
                _ => {}
            }
        }
        assert_eq!(replies.len(), 2);
        assert_eq!(replies[1], reply);
        assert!(closed, "backend EOF must complete a WebSocket close handshake");
        relay.await?;
        Ok::<_, anyhow::Error>(())
    }).await??;
    Ok(())
}
