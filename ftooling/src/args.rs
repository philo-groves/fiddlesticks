//! JSON argument parsing helpers for function and trait-based tools.
//!
//! ```rust
//! use ftooling::{parse_json_object, required_string};
//!
//! let args = parse_json_object(r#"{"query":"rust"}"#).expect("object should parse");
//! let query = required_string(&args, "query").expect("query should be present");
//! assert_eq!(query, "rust");
//! ```

use serde_json::{Map, Value};

use crate::ToolError;

pub fn parse_json_value(args_json: &str) -> Result<Value, ToolError> {
    serde_json::from_str(args_json)
        .map_err(|err| ToolError::invalid_arguments(format!("invalid JSON arguments: {err}")))
}

pub fn parse_json_object(args_json: &str) -> Result<Map<String, Value>, ToolError> {
    let value = parse_json_value(args_json)?;
    value
        .as_object()
        .cloned()
        .ok_or_else(|| ToolError::invalid_arguments("expected JSON object arguments"))
}

pub fn required_string(args: &Map<String, Value>, key: &str) -> Result<String, ToolError> {
    args.get(key)
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| ToolError::invalid_arguments(format!("missing required string: '{key}'")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_object_and_extract_required_string() {
        let args = parse_json_object("{\"query\":\"rust\"}").expect("args should parse");
        let query = required_string(&args, "query").expect("query should exist");
        assert_eq!(query, "rust");
    }

    #[test]
    fn parse_invalid_json_returns_invalid_arguments() {
        let error = parse_json_value("{").expect_err("json should fail");
        assert_eq!(error.kind, crate::ToolErrorKind::InvalidArguments);
    }
}
