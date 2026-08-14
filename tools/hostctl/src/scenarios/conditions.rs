//! Condition and expression evaluation for switch cases and retry guards.
//!
//! [`parse`] holds the tokenizer and recursive-descent parser; this file is the
//! evaluation entry point.

use crate::scenarios::conditions::parse::coerce_bool;
use crate::scenarios::conditions::parse::tokenize;
use crate::scenarios::conditions::parse::ExprParser;
use anyhow::anyhow;
use anyhow::Result;
use serde_json::Value;

pub(crate) mod parse;

pub(crate) fn eval_condition(raw_condition: &str, context: &Value) -> Result<bool> {
    coerce_bool(&eval_expression_value(raw_condition, context)?)
}

pub(crate) fn eval_expression_value(raw: &str, context: &Value) -> Result<Value> {
    let tokens = tokenize(unwrap_expression(raw))?;
    let mut parser = ExprParser {
        tokens: &tokens,
        index: 0,
        context,
    };
    let value = parser.parse_or()?;
    if parser.index != tokens.len() {
        return Err(anyhow!("unsupported condition syntax: {raw}"));
    }
    Ok(value)
}

fn unwrap_expression(raw: &str) -> &str {
    let trimmed = raw.trim();
    if trimmed.starts_with("${") && trimmed.ends_with('}') && trimmed.len() >= 4 {
        trimmed[2..trimmed.len() - 1].trim()
    } else {
        trimmed
    }
}

pub(crate) fn extract_path_value(input: &Value, path: &str) -> Result<Value> {
    lookup_path_value(input, path)
        .cloned()
        .ok_or_else(|| anyhow!("missing input field in condition path: {path}"))
}

pub(crate) fn lookup_path_value<'a>(input: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = input;
    for segment in path.trim().trim_start_matches('.').split('.') {
        if segment.is_empty() {
            continue;
        }
        current = current.get(segment)?;
    }
    Some(current)
}
