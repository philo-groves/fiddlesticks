/// Creates a single chat [`Message`](crate::Message) from a role shorthand.
///
/// ```rust
/// use fiddlesticks::{Role, fs_msg};
///
/// let message = fs_msg!(assistant => "Done.");
/// assert_eq!(message.role, Role::Assistant);
/// assert_eq!(message.content, "Done.");
/// ```
#[macro_export]
macro_rules! fs_msg {
    (system => $content:expr $(,)?) => {
        $crate::Message::new($crate::Role::System, $content)
    };
    (user => $content:expr $(,)?) => {
        $crate::Message::new($crate::Role::User, $content)
    };
    (assistant => $content:expr $(,)?) => {
        $crate::Message::new($crate::Role::Assistant, $content)
    };
    (tool => $content:expr $(,)?) => {
        $crate::Message::new($crate::Role::Tool, $content)
    };
    ($role:ident => $content:expr $(,)?) => {
        compile_error!("unsupported role: use system, user, assistant, or tool");
    };
}

/// Creates a `Vec<Message>` from role/content pairs.
///
/// ```rust
/// use fiddlesticks::{Role, fs_messages};
///
/// let messages = fs_messages![
///     system => "You are concise.",
///     user => "Summarize this repository.",
/// ];
///
/// assert_eq!(messages.len(), 2);
/// assert_eq!(messages[0].role, Role::System);
/// assert_eq!(messages[1].role, Role::User);
/// ```
#[macro_export]
macro_rules! fs_messages {
    () => {
        Vec::<$crate::Message>::new()
    };
    ($($role:ident => $content:expr),+ $(,)?) => {
        vec![$($crate::fs_msg!($role => $content)),+]
    };
}

/// Creates a [`ChatSession`](crate::ChatSession) with provider shorthand support.
///
/// ```rust
/// use fiddlesticks::{ProviderId, fs_session};
///
/// let session = fs_session!("session-1", openai, "gpt-4o-mini", "Be concise.");
/// assert_eq!(session.provider, ProviderId::OpenAi);
/// assert_eq!(session.system_prompt.as_deref(), Some("Be concise."));
/// ```
#[macro_export]
macro_rules! fs_session {
    ($session_id:expr, openai, $model:expr $(,)?) => {
        $crate::ChatSession::new($session_id, $crate::ProviderId::OpenAi, $model)
    };
    ($session_id:expr, opencode_zen, $model:expr $(,)?) => {
        $crate::ChatSession::new($session_id, $crate::ProviderId::OpenCodeZen, $model)
    };
    ($session_id:expr, anthropic, $model:expr $(,)?) => {
        $crate::ChatSession::new($session_id, $crate::ProviderId::Anthropic, $model)
    };
    ($session_id:expr, ollama, $model:expr $(,)?) => {
        $crate::ChatSession::new($session_id, $crate::ProviderId::Ollama, $model)
    };
    ($session_id:expr, local, $model:expr $(,)?) => {
        $crate::ChatSession::new($session_id, $crate::ProviderId::Ollama, $model)
    };
    ($session_id:expr, claude, $model:expr $(,)?) => {
        $crate::ChatSession::new($session_id, $crate::ProviderId::Anthropic, $model)
    };
    ($session_id:expr, $provider:expr, $model:expr $(,)?) => {
        $crate::ChatSession::new($session_id, $provider, $model)
    };
    ($session_id:expr, openai, $model:expr, $system_prompt:expr $(,)?) => {
        $crate::ChatSession::new($session_id, $crate::ProviderId::OpenAi, $model)
            .with_system_prompt($system_prompt)
    };
    ($session_id:expr, opencode_zen, $model:expr, $system_prompt:expr $(,)?) => {
        $crate::ChatSession::new($session_id, $crate::ProviderId::OpenCodeZen, $model)
            .with_system_prompt($system_prompt)
    };
    ($session_id:expr, anthropic, $model:expr, $system_prompt:expr $(,)?) => {
        $crate::ChatSession::new($session_id, $crate::ProviderId::Anthropic, $model)
            .with_system_prompt($system_prompt)
    };
    ($session_id:expr, ollama, $model:expr, $system_prompt:expr $(,)?) => {
        $crate::ChatSession::new($session_id, $crate::ProviderId::Ollama, $model)
            .with_system_prompt($system_prompt)
    };
    ($session_id:expr, local, $model:expr, $system_prompt:expr $(,)?) => {
        $crate::ChatSession::new($session_id, $crate::ProviderId::Ollama, $model)
            .with_system_prompt($system_prompt)
    };
    ($session_id:expr, claude, $model:expr, $system_prompt:expr $(,)?) => {
        $crate::ChatSession::new($session_id, $crate::ProviderId::Anthropic, $model)
            .with_system_prompt($system_prompt)
    };
    ($session_id:expr, $provider:expr, $model:expr, $system_prompt:expr $(,)?) => {
        $crate::ChatSession::new($session_id, $provider, $model).with_system_prompt($system_prompt)
    };
}
