//! Focused unit tests for OpenAI adapter internals.

#![cfg(test)]

use std::sync::Arc;

use crate::{Message, ModelRequest, ProviderError, ProviderFuture, Role, SecureCredentialManager, ToolResult};

use super::provider::OpenAiProvider;
use super::serde_api::parse_finish_reason;
use super::transport::OpenAiTransport;
use super::types::{
    OpenAiAuth, OpenAiFinishReason, OpenAiRequest, OpenAiResponse, OpenAiRole, OpenAiStreamChunk,
};

#[derive(Debug)]
struct NoopTransport;

impl OpenAiTransport for NoopTransport {
    fn complete<'a>(
        &'a self,
        _request: OpenAiRequest,
        _auth: OpenAiAuth,
    ) -> ProviderFuture<'a, Result<OpenAiResponse, ProviderError>> {
        Box::pin(async { Err(ProviderError::other("not used")) })
    }

    fn stream<'a>(
        &'a self,
        _request: OpenAiRequest,
        _auth: OpenAiAuth,
    ) -> ProviderFuture<'a, Result<Vec<OpenAiStreamChunk>, ProviderError>> {
        Box::pin(async { Err(ProviderError::other("not used")) })
    }
}

#[test]
fn build_openai_request_appends_tool_results_as_tool_messages() {
    let provider = OpenAiProvider::new(
        Arc::new(SecureCredentialManager::new()),
        Arc::new(NoopTransport),
    );
    let request = ModelRequest::new("gpt-4o-mini", vec![Message::new(Role::User, "hi")])
        .with_tool_results(vec![ToolResult {
            tool_call_id: "call_1".to_string(),
            output: "{\"ok\":true}".to_string(),
        }]);

    let built = provider.build_openai_request(request, false);
    assert_eq!(built.messages.len(), 2);
    assert_eq!(built.messages[1].role, OpenAiRole::Tool);
    assert_eq!(built.messages[1].tool_call_id.as_deref(), Some("call_1"));
}

#[test]
fn parse_finish_reason_maps_expected_values() {
    assert_eq!(parse_finish_reason(Some("stop")), OpenAiFinishReason::Stop);
    assert_eq!(parse_finish_reason(Some("length")), OpenAiFinishReason::Length);
    assert_eq!(
        parse_finish_reason(Some("tool_calls")),
        OpenAiFinishReason::ToolCalls
    );
    assert_eq!(parse_finish_reason(Some("unknown")), OpenAiFinishReason::Other);
    assert_eq!(parse_finish_reason(None), OpenAiFinishReason::Other);
}
