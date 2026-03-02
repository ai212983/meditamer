fn eval_condition(raw_condition: &str, context: &Value) -> Result<bool> {
    let condition = unwrap_expression(raw_condition);

    if condition.eq_ignore_ascii_case("true") {
        return Ok(true);
    }
    if condition.eq_ignore_ascii_case("false") {
        return Ok(false);
    }

    if let Some((left, right)) = condition.split_once("==") {
        let lhs = resolve_operand(context, left.trim())?;
        let rhs = resolve_operand(context, right.trim())?;
        return Ok(lhs == rhs);
    }
    if let Some((left, right)) = condition.split_once("!=") {
        let lhs = resolve_operand(context, left.trim())?;
        let rhs = resolve_operand(context, right.trim())?;
        return Ok(lhs != rhs);
    }
    if let Some((left, right)) = condition.split_once(">=") {
        return compare_numeric(context, left.trim(), right.trim(), |l, r| l >= r);
    }
    if let Some((left, right)) = condition.split_once("<=") {
        return compare_numeric(context, left.trim(), right.trim(), |l, r| l <= r);
    }
    if let Some((left, right)) = condition.split_once('>') {
        return compare_numeric(context, left.trim(), right.trim(), |l, r| l > r);
    }
    if let Some((left, right)) = condition.split_once('<') {
        return compare_numeric(context, left.trim(), right.trim(), |l, r| l < r);
    }

    Err(anyhow!("unsupported condition syntax: {raw_condition}"))
}

fn unwrap_expression(raw_condition: &str) -> &str {
    let condition = raw_condition.trim();
    if condition.starts_with("${") && condition.ends_with('}') && condition.len() >= 4 {
        condition[2..condition.len() - 1].trim()
    } else {
        condition
    }
}

fn extract_path_value(input: &Value, path: &str) -> Result<Value> {
    let trimmed = path.trim();
    let path = trimmed
        .strip_prefix('.')
        .ok_or_else(|| anyhow!("condition path must start with '.': {trimmed}"))?;

    let mut current = input;
    for segment in path.split('.') {
        if segment.is_empty() {
            continue;
        }
        current = current
            .get(segment)
            .ok_or_else(|| anyhow!("missing input field in condition path: {segment}"))?;
    }
    Ok(current.clone())
}

fn parse_literal(raw: &str) -> Value {
    let raw = raw.trim();
    if raw.starts_with('"') && raw.ends_with('"') && raw.len() >= 2 {
        return Value::String(raw[1..raw.len() - 1].to_string());
    }
    if raw.eq_ignore_ascii_case("true") {
        return Value::Bool(true);
    }
    if raw.eq_ignore_ascii_case("false") {
        return Value::Bool(false);
    }
    if let Ok(num) = raw.parse::<i64>() {
        return Value::Number(num.into());
    }
    if let Ok(num) = raw.parse::<f64>() {
        return serde_json::Number::from_f64(num)
            .map(Value::Number)
            .unwrap_or_else(|| Value::String(raw.to_string()));
    }
    Value::String(raw.to_string())
}

fn resolve_operand(context: &Value, raw: &str) -> Result<Value> {
    let trimmed = raw.trim();
    if trimmed.starts_with('.') {
        extract_path_value(context, trimmed)
    } else {
        Ok(parse_literal(trimmed))
    }
}

fn compare_numeric<F>(context: &Value, left: &str, right: &str, cmp: F) -> Result<bool>
where
    F: Fn(f64, f64) -> bool,
{
    let lhs = resolve_operand(context, left)?;
    let rhs = resolve_operand(context, right)?;

    let l = lhs
        .as_f64()
        .ok_or_else(|| anyhow!("left side is not numeric: {left}"))?;
    let r = rhs
        .as_f64()
        .ok_or_else(|| anyhow!("right side is not numeric: {right}"))?;
    Ok(cmp(l, r))
}
