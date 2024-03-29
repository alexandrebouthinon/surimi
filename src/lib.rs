use async_std::net::TcpListener;
use async_std::task;
use async_tungstenite::tungstenite::protocol::Message;
use futures_util::sink::SinkExt;
use futures_util::stream::StreamExt;
use serde_json::Value;
use std::error::Error;

#[derive(Clone)]
pub struct MockServerOptions {
    pub host: String,
    pub port: u16,
}

impl Default for MockServerOptions {
    /// Create MockServerOptions with default values.
    /// Default values are:
    /// - host: "localhost"
    /// - port: 8080
    ///
    /// # Examples
    /// ```
    /// use surimi::MockServerOptions;
    ///
    /// let options = MockServerOptions::default();
    /// ```
    fn default() -> Self {
        Self {
            host: "localhost".into(),
            port: 0,
        }
    }
}

/// MockServer is a mock server that can be used to test your application.
/// It can be used to test WebSocket connections.
///
/// # Examples
/// ```
/// use surimi::MockServer;
/// use async_std::stream::StreamExt;
/// use futures_util::sink::SinkExt;
/// use async_tungstenite::tungstenite::protocol::Message;
/// use serde_json::json;
/// use serde_json::from_str;
/// use serde_json::Value;
///
/// # #[async_std::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let (host, port) = MockServer::default()
///         .responses(vec![
///             json!({"hello": "world"}),
///         ])
///         .start()
///         .await?;
///
///     assert_eq!(host, "localhost");
///     assert_ne!(port, 0); // the port should be pick randomly by the OS
///
///     let endpoint = format!("ws://{}:{}", host, port);
///     let (mut stream, _) = async_tungstenite::async_std::connect_async(endpoint).await?;
///     stream
///         .send(Message::Text("hello".into()))
///         .await?;
///
///     let json_response : Value =
///         serde_json::from_str(&stream.next().await.unwrap()?.into_text()?)?;
///
///     assert_eq!(json_response, json!({"hello": "world"}));
///
///     stream.close(None).await?;
///
/// #   Ok(())
/// # }
/// ```
///
pub struct MockServer {
    pub responses: Vec<Value>,
    pub options: MockServerOptions,
}

impl Default for MockServer {
    fn default() -> Self {
        Self {
            responses: vec![],
            options: MockServerOptions::default(),
        }
    }
}

impl MockServer {
    pub fn host(mut self, host: String) -> Self {
        self.options.host = host;
        self
    }

    pub fn port(mut self, port: u16) -> Self {
        self.options.port = port;
        self
    }

    pub fn responses(mut self, responses: Vec<Value>) -> Self {
        let mut r = responses.clone();
        r.reverse(); // handler use Vec.pop() to get the last response
        self.responses = r;
        self
    }

    pub async fn start(self) -> Result<(String, u16), Box<dyn Error>> {
        let listener =
            TcpListener::bind(format!("{}:{}", &self.options.host, &self.options.port)).await?;

        let port = listener.local_addr()?.port();
        let host = String::from(&self.options.host);

        task::spawn(async move {
            self.ws_handler(&listener).await.unwrap();
        });

        Ok((host, port))
    }

    async fn ws_handler(self, listener: &TcpListener) -> Result<(), Box<dyn Error>> {
        let mut incoming = listener.incoming();
        while let Some(stream) = incoming.next().await {
            let stream = stream?;
            let mut socket = async_tungstenite::accept_async(stream).await?;
            let mut responses = self.responses.clone();

            while let Some(message) = socket.next().await {
                match message? {
                    Message::Text(_) => {
                        if let Some(response) = responses.pop() {
                            socket.send(Message::Text(response.to_string())).await?;
                            continue;
                        }
                        socket
                            .send(Message::Text("No more response".into()))
                            .await?;
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn endpoint(host: &str, port: u16) -> String {
        format!("ws://{}:{}", host, port)
    }

    #[async_std::test]
    #[should_panic]
    async fn should_panic_if_same_port_used_twice() {
        let (_, _) = MockServer::default().port(8080).start().await.unwrap();
        let (_, _) = MockServer::default().port(8080).start().await.unwrap();
    }

    #[async_std::test]
    async fn connect() -> Result<(), Box<dyn Error>> {
        let (host, port) = MockServer::default().start().await?;

        assert_eq!(host, "localhost");
        assert_ne!(port, 0); // the port should be pick randomly by the OS

        let (mut stream, _) =
            async_tungstenite::async_std::connect_async(endpoint(&host, port)).await?;

        stream.close(None).await?;
        Ok(())
    }

    #[async_std::test]
    async fn connect_with_custom_config() -> Result<(), Box<dyn Error>> {
        let (host, port) = MockServer::default()
            .host("127.0.0.1".into())
            .port(8080)
            .start()
            .await?;

        assert_eq!(host, "127.0.0.1");
        assert_eq!(port, 8080);

        let (mut stream, _) =
            async_tungstenite::async_std::connect_async(endpoint(&host, port)).await?;

        stream.close(None).await?;
        Ok(())
    }

    #[async_std::test]
    async fn should_answer_pong() -> Result<(), Box<dyn Error>> {
        let (host, port) = MockServer::default().start().await?;

        let (mut stream, _) =
            async_tungstenite::async_std::connect_async(endpoint(&host, port)).await?;

        stream.send(Message::Ping("Some request".into())).await?;

        let response = stream.next().await.unwrap()?;
        assert_eq!(
            response,
            Message::Pong("Some request".into()),
            "should answer with pong"
        );

        stream.close(None).await?;
        Ok(())
    }

    #[async_std::test]
    async fn should_wait_for_close_message() -> Result<(), Box<dyn Error>> {
        let (host, port) = MockServer::default().start().await?;

        let (mut stream, _) =
            async_tungstenite::async_std::connect_async(endpoint(&host, port)).await?;

        for _ in 0..10 {
            stream.send(Message::Text("Some request".into())).await?;
            let response = stream.next().await.unwrap()?;
            assert_eq!(response, Message::Text("No more response".into()),);
        }

        stream.close(None).await?;
        Ok(())
    }

    #[async_std::test]
    async fn connect_and_answer() -> Result<(), Box<dyn Error>> {
        let mocked_responses = vec![
            json!({"hello": "world"}),
            json!({"hello": "france"}),
            json!({"hello": "montpellier"}),
        ];

        let (host, port) = MockServer::default()
            .responses(mocked_responses.clone())
            .start()
            .await?;

        assert_eq!(host, "localhost");
        assert_ne!(port, 0); // the port should be pick randomly by the OS

        let (mut stream, _) =
            async_tungstenite::async_std::connect_async(endpoint(&host, port)).await?;

        for m_response in mocked_responses {
            stream
                .send(Message::Text(String::from("Trigger mocked server reply")))
                .await?;

            let response: Value =
                serde_json::from_str(&stream.next().await.unwrap()?.into_text()?)?;

            assert_eq!(response, m_response);
            assert_eq!(response["hello"], m_response["hello"]);
        }

        stream.close(None).await?;
        Ok(())
    }
}
