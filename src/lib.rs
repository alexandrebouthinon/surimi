use async_std::net::TcpListener;
use async_std::task;
use async_tungstenite::tungstenite::protocol::Message;
use futures_util::{sink::SinkExt, stream::StreamExt};
use serde_json::{json, Value};
use std::error::Error;

pub struct MockServer {
    responses: Vec<Value>,
}

impl MockServer {
    pub fn new() -> Self {
        MockServer { responses: vec![] }
    }

    pub fn respond_with(mut self, responses: Vec<Value>) -> Self {
        for response in responses {
            self.responses.push(json!(response));
        }
        self
    }

    pub async fn then_start(self) -> Result<(&'static str, u16), Box<dyn Error>> {
        let socket = TcpListener::bind("localhost:0").await?;
        let port = socket.local_addr()?.port();
        task::spawn(async move {
            let (stream, _) = socket
                .accept()
                .await
                .expect("Failed to accept inbound TCP connection");
            let mut ws_stream = async_tungstenite::accept_async(&stream)
                .await
                .expect("Failed to open WebSocket stream");
            let mut messages = self.responses.clone().into_iter();
            while let Some(Ok(incomming_msg)) = ws_stream.next().await {
                if !incomming_msg.is_close() {
                    if let Some(message) = messages.next() {
                        let msg: String = serde_json::to_string(&message).unwrap();
                        ws_stream.send(Message::Text(msg)).await.unwrap();
                    } else {
                        return;
                    }
                }
            }
        });
        Ok(("localhost", port))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[async_std::test]
    async fn connect() -> Result<(), Box<dyn Error>> {
        let (host, port) = MockServer::new().then_start().await?;
        let (_, _) =
            async_tungstenite::async_std::connect_async(format!("ws://{}:{}", host, port)).await?;
            
        Ok(())
    }

    #[async_std::test]
    async fn with_json_responses() -> Result<(), Box<dyn Error>> {
        let responses = vec![
            json!({"Response": 1}),
            json!({"Response": 2}),
            json!({"Response": 3}),
        ];
        let (host, port) = MockServer::new()
            .respond_with(responses.clone())
            .then_start()
            .await?;
        let (mut client, _) =
            async_tungstenite::async_std::connect_async(format!("ws://{}:{}", host, port)).await?;

        for mocked_response in responses.clone() {
            client
                .send(Message::Text(String::from("Trigger mocked server reply")))
                .await?;

            let response: Value =
                serde_json::from_str(&client.next().await.unwrap()?.into_text()?)?;

            assert_eq!(
                serde_json::from_value::<u8>(mocked_response["Response"].clone())?,
                serde_json::from_value::<u8>(response["Response"].clone())?
            );
        }

        Ok(())
    }
}
