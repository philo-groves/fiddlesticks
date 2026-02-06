//! Small convenience constructors for common types.

use crate::{ChatSession, ChatTurnRequest, Message, ProviderId, Role, SessionId};

pub fn system_message(content: impl Into<String>) -> Message {
    Message::new(Role::System, content)
}

pub fn user_message(content: impl Into<String>) -> Message {
    Message::new(Role::User, content)
}

pub fn assistant_message(content: impl Into<String>) -> Message {
    Message::new(Role::Assistant, content)
}

pub fn tool_message(content: impl Into<String>) -> Message {
    Message::new(Role::Tool, content)
}

pub fn session(
    id: impl Into<SessionId>,
    provider: ProviderId,
    model: impl Into<String>,
) -> ChatSession {
    ChatSession::new(id, provider, model)
}

pub fn turn(session: ChatSession, user_input: impl Into<String>) -> ChatTurnRequest {
    ChatTurnRequest::new(session, user_input)
}

pub fn streaming_turn(session: ChatSession, user_input: impl Into<String>) -> ChatTurnRequest {
    ChatTurnRequest::new(session, user_input).enable_streaming()
}

pub fn parse_provider_id(value: &str) -> Option<ProviderId> {
    match value.trim().to_ascii_lowercase().as_str() {
        "opencode-zen" | "opencode_zen" | "opencode" | "zen" => Some(ProviderId::OpenCodeZen),
        "openai" => Some(ProviderId::OpenAi),
        "claude" | "anthropic" => Some(ProviderId::Claude),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::{ProviderId, Role};

    use super::{parse_provider_id, streaming_turn, turn, user_message};

    #[test]
    fn parse_provider_id_supports_aliases() {
        assert_eq!(parse_provider_id("openai"), Some(ProviderId::OpenAi));
        assert_eq!(parse_provider_id("Zen"), Some(ProviderId::OpenCodeZen));
        assert_eq!(parse_provider_id("anthropic"), Some(ProviderId::Claude));
        assert_eq!(parse_provider_id("unknown"), None);
    }

    #[test]
    fn message_and_turn_helpers_apply_expected_defaults() {
        let message = user_message("hello");
        assert_eq!(message.role, Role::User);

        let session = crate::session("session-1", ProviderId::OpenAi, "gpt-4o-mini");
        let non_streaming = turn(session.clone(), "hello");
        let streaming = streaming_turn(session, "hello");

        assert!(!non_streaming.stream);
        assert!(streaming.stream);
    }
}
