//! OpenAI Responses API transport over persistent WebSocket mode.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use async_stream::try_stream;
use futures_util::{SinkExt, StreamExt};
use http::{HeaderValue, header};
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message as WsMessage, client::IntoClientRequest},
};

use crate::{ProviderError, ProviderFuture};

use super::transport::{OpenAiChunkStream, OpenAiTransport};
use super::types::{
    OpenAiAssistantMessage, OpenAiAuth, OpenAiFinishReason, OpenAiRequest, OpenAiResponse,
    OpenAiStreamChunk, OpenAiToolCall, OpenAiUsage,
};

const OPENAI_RESPONSES_WS_URL: &str = "wss://api.openai.com/v1/responses";
const CONNECTION_MAX_AGE: Duration = Duration::from_secs(55 * 60);

#[derive(Debug, Default)]
struct WsConnectionState {
    socket: Option<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>>,
    connected_at: Option<Instant>,
    auth_cache_key: Option<String>,
}

#[derive(Debug)]
pub struct OpenAiResponsesWebSocketTransport {
    url: String,
    connection: Mutex<WsConnectionState>,
}

impl OpenAiResponsesWebSocketTransport {
    pub fn new() -> Self {
        Self {
            url: OPENAI_RESPONSES_WS_URL.to_string(),
            connection: Mutex::new(WsConnectionState::default()),
        }
    }

    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = url.into();
        self
    }

    async fn ensure_connection(
        &self,
        state: &mut WsConnectionState,
        auth: &OpenAiAuth,
    ) -> Result<(), ProviderError> {
        let now = Instant::now();
        let auth_cache_key = auth_cache_key(auth);
        let should_reconnect = state
            .connected_at
            .map(|connected_at| now.duration_since(connected_at) >= CONNECTION_MAX_AGE)
            .unwrap_or(false)
            || state.socket.is_none()
            || state.auth_cache_key.as_ref() != Some(&auth_cache_key);

        if !should_reconnect {
            return Ok(());
        }

        state.socket = None;
        state.connected_at = None;
        state.auth_cache_key = None;

        let mut request = self
            .url
            .as_str()
            .into_client_request()
            .map_err(|err| ProviderError::transport(err.to_string()))?;

        match auth {
            OpenAiAuth::ApiKey(key) => {
                let header_value = HeaderValue::from_str(&format!("Bearer {}", key.expose()))
                    .map_err(|err| ProviderError::transport(err.to_string()))?;
                request.headers_mut().insert(header::AUTHORIZATION, header_value);
            }
            OpenAiAuth::BrowserSession(session) => {
                let header_value = HeaderValue::from_str(&format!(
                    "__Secure-next-auth.session-token={}",
                    session.expose()
                ))
                .map_err(|err| ProviderError::transport(err.to_string()))?;
                request.headers_mut().insert(header::COOKIE, header_value);
            }
        }

        let (socket, _) = connect_async(request)
            .await
            .map_err(|err| ProviderError::transport(err.to_string()))?;

        state.socket = Some(socket);
        state.connected_at = Some(now);
        state.auth_cache_key = Some(auth_cache_key);
        Ok(())
    }

    async fn send_create_request(
        socket: &mut WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
        request: OpenAiRequest,
    ) -> Result<(), ProviderError> {
        let payload = build_response_create_payload(request)?;
        socket
            .send(WsMessage::Text(payload.to_string().into()))
            .await
            .map_err(|err| ProviderError::transport(err.to_string()))
    }
}

impl Default for OpenAiResponsesWebSocketTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl OpenAiTransport for OpenAiResponsesWebSocketTransport {
    fn complete<'a>(
        &'a self,
        request: OpenAiRequest,
        auth: OpenAiAuth,
    ) -> ProviderFuture<'a, Result<OpenAiResponse, ProviderError>> {
        Box::pin(async move {
            let mut state = self.connection.lock().await;
            self.ensure_connection(&mut state, &auth).await?;
            let socket = state
                .socket
                .as_mut()
                .ok_or_else(|| ProviderError::transport("OpenAI websocket not connected"))?;

            if let Err(err) = Self::send_create_request(socket, request.clone()).await {
                state.socket = None;
                return Err(err);
            }

            let mut accumulator = ResponsesEventAccumulator::new(request.model);
            loop {
                let next = socket.next().await;
                let Some(message) = next else {
                    state.socket = None;
                    return Err(ProviderError::transport(
                        "OpenAI websocket closed before response completed",
                    ));
                };

                match message {
                    Ok(WsMessage::Text(text)) => {
                        let event = serde_json::from_str::<Value>(&text)
                            .map_err(|err| ProviderError::transport(err.to_string()))?;
                        let chunks = accumulator.handle_event(event)?;
                        for chunk in chunks {
                            if let OpenAiStreamChunk::ResponseComplete(response) = chunk {
                                return Ok(response);
                            }
                        }
                    }
                    Ok(WsMessage::Ping(payload)) => {
                        socket
                            .send(WsMessage::Pong(payload))
                            .await
                            .map_err(|err| ProviderError::transport(err.to_string()))?;
                    }
                    Ok(WsMessage::Close(_)) => {
                        state.socket = None;
                        return Err(ProviderError::transport(
                            "OpenAI websocket closed before response completed",
                        ));
                    }
                    Ok(_) => {}
                    Err(err) => {
                        state.socket = None;
                        return Err(ProviderError::transport(err.to_string()));
                    }
                }
            }
        })
    }

    fn stream<'a>(
        &'a self,
        request: OpenAiRequest,
        auth: OpenAiAuth,
    ) -> ProviderFuture<'a, Result<OpenAiChunkStream<'a>, ProviderError>> {
        Box::pin(async move {
            let stream = try_stream! {
                let mut state = self.connection.lock().await;
                self.ensure_connection(&mut state, &auth).await?;
                let socket = state
                    .socket
                    .as_mut()
                    .ok_or_else(|| ProviderError::transport("OpenAI websocket not connected"))?;

                if let Err(err) = Self::send_create_request(socket, request.clone()).await {
                    Err(err)?;
                }

                let mut accumulator = ResponsesEventAccumulator::new(request.model);
                loop {
                    let next = socket.next().await;
                    let Some(message) = next else {
                        Err(ProviderError::transport(
                            "OpenAI websocket closed before response completed",
                        ))?;
                        continue;
                    };

                    match message {
                        Ok(WsMessage::Text(text)) => {
                            let event = serde_json::from_str::<Value>(&text)
                                .map_err(|err| ProviderError::transport(err.to_string()))?;
                            let chunks = accumulator.handle_event(event)?;

                            let mut complete = false;
                            for chunk in chunks {
                                if matches!(chunk, OpenAiStreamChunk::ResponseComplete(_)) {
                                    complete = true;
                                }
                                yield chunk;
                            }

                            if complete {
                                break;
                            }
                        }
                        Ok(WsMessage::Ping(payload)) => {
                            socket
                                .send(WsMessage::Pong(payload))
                                .await
                                .map_err(|err| ProviderError::transport(err.to_string()))?;
                        }
                        Ok(WsMessage::Close(_)) => {
                            Err(ProviderError::transport(
                                "OpenAI websocket closed before response completed",
                            ))?;
                        }
                        Ok(_) => {}
                        Err(err) => {
                            Err(ProviderError::transport(err.to_string()))?;
                        }
                    }
                }
            };

            Ok(Box::pin(stream) as OpenAiChunkStream<'a>)
        })
    }
}

#[derive(Debug)]
struct ResponsesEventAccumulator {
    fallback_model: String,
    text: String,
    tool_calls: BTreeMap<String, OpenAiToolCall>,
}

impl ResponsesEventAccumulator {
    fn new(fallback_model: String) -> Self {
        Self {
            fallback_model,
            text: String::new(),
            tool_calls: BTreeMap::new(),
        }
    }

    fn handle_event(&mut self, event: Value) -> Result<Vec<OpenAiStreamChunk>, ProviderError> {
        let event_type = event
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();

        match event_type {
            "error" => Err(map_ws_error(event)),
            "response.output_text.delta" => {
                let delta = event
                    .get("delta")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();

                if delta.is_empty() {
                    Ok(Vec::new())
                } else {
                    self.text.push_str(&delta);
                    Ok(vec![OpenAiStreamChunk::TextDelta(delta)])
                }
            }
            "response.function_call_arguments.delta" => {
                let call_id = event
                    .get("call_id")
                    .and_then(Value::as_str)
                    .or_else(|| {
                        event
                            .get("item")
                            .and_then(|item| item.get("call_id"))
                            .and_then(Value::as_str)
                    })
                    .unwrap_or("tool_call_0")
                    .to_string();
                let delta = event
                    .get("delta")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let name = event
                    .get("name")
                    .and_then(Value::as_str)
                    .or_else(|| {
                        event
                            .get("item")
                            .and_then(|item| item.get("name"))
                            .and_then(Value::as_str)
                    })
                    .unwrap_or_default();

                let entry =
                    self.tool_calls
                        .entry(call_id.clone())
                        .or_insert_with(|| OpenAiToolCall {
                            id: call_id.clone(),
                            name: String::new(),
                            arguments: String::new(),
                        });

                if !name.is_empty() {
                    entry.name = name.to_string();
                }
                if !delta.is_empty() {
                    entry.arguments.push_str(delta);
                }

                Ok(vec![OpenAiStreamChunk::ToolCallDelta(entry.clone())])
            }
            "response.output_item.added" | "response.output_item.done" => {
                let Some(item) = event.get("item") else {
                    return Ok(Vec::new());
                };

                if item.get("type").and_then(Value::as_str) == Some("function_call") {
                    let call_id = item
                        .get("call_id")
                        .and_then(Value::as_str)
                        .or_else(|| item.get("id").and_then(Value::as_str))
                        .unwrap_or("tool_call_0")
                        .to_string();

                    let entry =
                        self.tool_calls
                            .entry(call_id.clone())
                            .or_insert_with(|| OpenAiToolCall {
                                id: call_id.clone(),
                                name: String::new(),
                                arguments: String::new(),
                            });

                    if let Some(name) = item.get("name").and_then(Value::as_str) {
                        entry.name = name.to_string();
                    }
                    if let Some(arguments) = item.get("arguments").and_then(Value::as_str) {
                        entry.arguments = arguments.to_string();
                    }

                    return Ok(vec![OpenAiStreamChunk::ToolCallDelta(entry.clone())]);
                }

                if item.get("type").and_then(Value::as_str) == Some("message") {
                    let message_text = parse_message_text(item);
                    if self.text.is_empty() && !message_text.is_empty() {
                        self.text = message_text;
                    }
                }

                Ok(Vec::new())
            }
            "response.completed"
            | "response.failed"
            | "response.incomplete"
            | "response.cancelled" => {
                let response = parse_completed_response(
                    event.get("response").unwrap_or(&Value::Null),
                    &self.fallback_model,
                    &self.text,
                    &self.tool_calls,
                )?;

                Ok(vec![
                    OpenAiStreamChunk::MessageComplete(response.message.clone()),
                    OpenAiStreamChunk::ResponseComplete(response),
                ])
            }
            _ => Ok(Vec::new()),
        }
    }
}

fn build_response_create_payload(request: OpenAiRequest) -> Result<Value, ProviderError> {
    let mut input = Vec::<Value>::new();
    for message in request.messages {
        if matches!(message.role, super::types::OpenAiRole::Tool) {
            let call_id = message.tool_call_id.ok_or_else(|| {
                ProviderError::invalid_request("OpenAI tool message is missing tool_call_id")
            })?;

            input.push(json!({
                "type": "function_call_output",
                "call_id": call_id,
                "output": message.content,
            }));
            continue;
        }

        if message.content.trim().is_empty()
            && !matches!(message.role, super::types::OpenAiRole::Assistant)
        {
            return Err(ProviderError::invalid_request(
                "OpenAI message content must not be empty",
            ));
        }

        input.push(json!({
            "type": "message",
            "role": message.role.as_str(),
            "content": [{
                "type": "input_text",
                "text": message.content,
            }],
        }));
    }

    if input.is_empty() {
        return Err(ProviderError::invalid_request(
            "OpenAI request requires at least one message",
        ));
    }

    let mut tools = Vec::<Value>::new();
    for tool in request.tools {
        let parameters = serde_json::from_str::<Value>(&tool.input_schema)
            .map_err(|_| ProviderError::invalid_request("OpenAI tool schema must be valid JSON"))?;

        tools.push(json!({
            "type": "function",
            "name": tool.name,
            "description": tool.description,
            "parameters": parameters,
        }));
    }

    Ok(json!({
        "type": "response.create",
        "model": request.model,
        "store": false,
        "input": input,
        "tools": tools,
        "temperature": request.temperature,
        "max_output_tokens": request.max_tokens,
    }))
}

fn parse_completed_response(
    response: &Value,
    fallback_model: &str,
    accumulated_text: &str,
    accumulated_tool_calls: &BTreeMap<String, OpenAiToolCall>,
) -> Result<OpenAiResponse, ProviderError> {
    let model = response
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or(fallback_model)
        .to_string();

    let mut message_content = accumulated_text.to_string();
    let mut tool_calls = accumulated_tool_calls.clone();

    if let Some(output) = response.get("output").and_then(Value::as_array) {
        for item in output {
            match item.get("type").and_then(Value::as_str) {
                Some("message") => {
                    if item.get("role").and_then(Value::as_str) == Some("assistant") {
                        let parsed = parse_message_text(item);
                        if !parsed.is_empty() {
                            message_content = parsed;
                        }
                    }
                }
                Some("function_call") => {
                    let call_id = item
                        .get("call_id")
                        .and_then(Value::as_str)
                        .or_else(|| item.get("id").and_then(Value::as_str))
                        .unwrap_or("tool_call_0")
                        .to_string();
                    let name = item
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    let arguments = item
                        .get("arguments")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();

                    tool_calls.insert(
                        call_id.clone(),
                        OpenAiToolCall {
                            id: call_id,
                            name,
                            arguments,
                        },
                    );
                }
                _ => {}
            }
        }
    }

    let finish_reason = parse_response_finish_reason(response);
    let usage = parse_response_usage(response);

    Ok(OpenAiResponse {
        model,
        message: OpenAiAssistantMessage {
            content: message_content,
            tool_calls: tool_calls.into_values().collect(),
        },
        finish_reason,
        usage,
    })
}

fn parse_message_text(item: &Value) -> String {
    let mut output = String::new();
    if let Some(content) = item.get("content").and_then(Value::as_array) {
        for part in content {
            if part.get("type").and_then(Value::as_str) == Some("output_text")
                && let Some(text) = part.get("text").and_then(Value::as_str)
            {
                output.push_str(text);
            }
        }
    }
    output
}

fn parse_response_finish_reason(response: &Value) -> OpenAiFinishReason {
    if let Some(reason) = response
        .get("stop_reason")
        .and_then(Value::as_str)
        .map(|value| value.to_ascii_lowercase())
    {
        return match reason.as_str() {
            "stop" => OpenAiFinishReason::Stop,
            "length" | "max_output_tokens" => OpenAiFinishReason::Length,
            "tool_calls" | "function_call" => OpenAiFinishReason::ToolCalls,
            "cancelled" => OpenAiFinishReason::Cancelled,
            _ => OpenAiFinishReason::Other,
        };
    }

    match response
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "completed" => OpenAiFinishReason::Stop,
        "incomplete" => OpenAiFinishReason::Length,
        "cancelled" => OpenAiFinishReason::Cancelled,
        _ => OpenAiFinishReason::Other,
    }
}

fn parse_response_usage(response: &Value) -> OpenAiUsage {
    let usage = response.get("usage").unwrap_or(&Value::Null);

    OpenAiUsage {
        prompt_tokens: usage
            .get("input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32,
        completion_tokens: usage
            .get("output_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32,
        total_tokens: usage
            .get("total_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32,
    }
}

fn map_ws_error(event: Value) -> ProviderError {
    let status = event
        .get("status")
        .and_then(Value::as_u64)
        .unwrap_or_default() as u16;

    let error = event.get("error").unwrap_or(&Value::Null);
    let code = error
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("OpenAI websocket request failed")
        .to_string();

    match code {
        "previous_response_not_found" | "websocket_connection_limit_reached" => {
            ProviderError::invalid_request(message)
        }
        _ => match status {
            401 | 403 => ProviderError::authentication(message),
            408 | 504 => ProviderError::timeout(message),
            429 => ProviderError::rate_limited(message),
            400 | 422 => ProviderError::invalid_request(message),
            502 | 503 => ProviderError::unavailable(message),
            _ => ProviderError::transport(message),
        },
    }
}

fn auth_cache_key(auth: &OpenAiAuth) -> String {
    match auth {
        OpenAiAuth::ApiKey(value) => format!("api:{}", value.expose()),
        OpenAiAuth::BrowserSession(value) => format!("cookie:{}", value.expose()),
    }
}
