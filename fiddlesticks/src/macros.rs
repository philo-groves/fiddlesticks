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

#[macro_export]
macro_rules! fs_messages {
    () => {
        Vec::<$crate::Message>::new()
    };
    ($($role:ident => $content:expr),+ $(,)?) => {
        vec![$($crate::fs_msg!($role => $content)),+]
    };
}

#[macro_export]
macro_rules! fs_session {
    ($session_id:expr, openai, $model:expr $(,)?) => {
        $crate::ChatSession::new($session_id, $crate::ProviderId::OpenAi, $model)
    };
    ($session_id:expr, opencode_zen, $model:expr $(,)?) => {
        $crate::ChatSession::new($session_id, $crate::ProviderId::OpenCodeZen, $model)
    };
    ($session_id:expr, claude, $model:expr $(,)?) => {
        $crate::ChatSession::new($session_id, $crate::ProviderId::Claude, $model)
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
    ($session_id:expr, claude, $model:expr, $system_prompt:expr $(,)?) => {
        $crate::ChatSession::new($session_id, $crate::ProviderId::Claude, $model)
            .with_system_prompt($system_prompt)
    };
    ($session_id:expr, $provider:expr, $model:expr, $system_prompt:expr $(,)?) => {
        $crate::ChatSession::new($session_id, $provider, $model).with_system_prompt($system_prompt)
    };
}
