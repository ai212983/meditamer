#[derive(Clone, Debug, PartialEq)]
enum Token {
    And,
    Or,
    Eq,
    Ne,
    Gte,
    Lte,
    Gt,
    Lt,
    Plus,
    Minus,
    Not,
    LParen,
    RParen,
    Bool(bool),
    Null,
    Number(f64),
    String(String),
    Path(String),
    Ident(String),
}

fn tokenize(raw: &str) -> Result<Vec<Token>> {
    let chars = raw.chars().collect::<Vec<_>>();
    let mut tokens = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        match chars[i] {
            c if c.is_whitespace() => i += 1,
            '&' if chars.get(i + 1) == Some(&'&') => push_token(&mut tokens, Token::And, &mut i, 2),
            '|' if chars.get(i + 1) == Some(&'|') => push_token(&mut tokens, Token::Or, &mut i, 2),
            '=' if chars.get(i + 1) == Some(&'=') => push_token(&mut tokens, Token::Eq, &mut i, 2),
            '!' if chars.get(i + 1) == Some(&'=') => push_token(&mut tokens, Token::Ne, &mut i, 2),
            '>' if chars.get(i + 1) == Some(&'=') => push_token(&mut tokens, Token::Gte, &mut i, 2),
            '<' if chars.get(i + 1) == Some(&'=') => push_token(&mut tokens, Token::Lte, &mut i, 2),
            '>' => push_token(&mut tokens, Token::Gt, &mut i, 1),
            '<' => push_token(&mut tokens, Token::Lt, &mut i, 1),
            '+' => push_token(&mut tokens, Token::Plus, &mut i, 1),
            '-' => push_token(&mut tokens, Token::Minus, &mut i, 1),
            '!' => push_token(&mut tokens, Token::Not, &mut i, 1),
            '(' => push_token(&mut tokens, Token::LParen, &mut i, 1),
            ')' => push_token(&mut tokens, Token::RParen, &mut i, 1),
            '"' => tokenize_string(&chars, raw, &mut tokens, &mut i)?,
            '.' => tokenize_path(&chars, &mut tokens, &mut i),
            c if c.is_ascii_digit() => tokenize_number(&chars, &mut tokens, &mut i)?,
            c if c.is_ascii_alphabetic() || c == '_' => tokenize_ident(&chars, &mut tokens, &mut i),
            other => return Err(anyhow!("unsupported token '{other}' in condition: {raw}")),
        }
    }
    Ok(tokens)
}

fn push_token(tokens: &mut Vec<Token>, token: Token, index: &mut usize, step: usize) {
    tokens.push(token);
    *index += step;
}

fn tokenize_string(chars: &[char], raw: &str, tokens: &mut Vec<Token>, index: &mut usize) -> Result<()> {
    let mut value = String::new();
    *index += 1;
    while *index < chars.len() {
        match chars[*index] {
            '"' => break,
            '\\' if *index + 1 < chars.len() => {
                value.push(chars[*index + 1]);
                *index += 2;
            }
            other => {
                value.push(other);
                *index += 1;
            }
        }
    }
    if *index >= chars.len() || chars[*index] != '"' {
        return Err(anyhow!("unterminated string literal in condition: {raw}"));
    }
    tokens.push(Token::String(value));
    *index += 1;
    Ok(())
}

fn tokenize_path(chars: &[char], tokens: &mut Vec<Token>, index: &mut usize) {
    let start = *index;
    *index += 1;
    while *index < chars.len()
        && (chars[*index].is_ascii_alphanumeric() || chars[*index] == '_' || chars[*index] == '.')
    {
        *index += 1;
    }
    tokens.push(Token::Path(chars[start..*index].iter().collect()));
}

fn tokenize_number(chars: &[char], tokens: &mut Vec<Token>, index: &mut usize) -> Result<()> {
    let start = *index;
    *index += 1;
    while *index < chars.len() && (chars[*index].is_ascii_digit() || chars[*index] == '.') {
        *index += 1;
    }
    let raw_number = chars[start..*index].iter().collect::<String>();
    let number = raw_number
        .parse::<f64>()
        .map_err(|_| anyhow!("invalid numeric literal in condition: {raw_number}"))?;
    tokens.push(Token::Number(number));
    Ok(())
}

fn tokenize_ident(chars: &[char], tokens: &mut Vec<Token>, index: &mut usize) {
    let start = *index;
    *index += 1;
    while *index < chars.len() && (chars[*index].is_ascii_alphanumeric() || chars[*index] == '_') {
        *index += 1;
    }
    let ident = chars[start..*index].iter().collect::<String>();
    tokens.push(match ident.as_str() {
        "true" => Token::Bool(true),
        "false" => Token::Bool(false),
        "null" => Token::Null,
        _ => Token::Ident(ident),
    });
}

struct ExprParser<'a> {
    tokens: &'a [Token],
    index: usize,
    context: &'a Value,
}

impl ExprParser<'_> {
    fn parse_or(&mut self) -> Result<Value> {
        let mut value = self.parse_and()?;
        while self.consume(&Token::Or) {
            let rhs = self.parse_and()?;
            value = Value::Bool(coerce_bool(&value)? || coerce_bool(&rhs)?);
        }
        Ok(value)
    }

    fn parse_and(&mut self) -> Result<Value> {
        let mut value = self.parse_equality()?;
        while self.consume(&Token::And) {
            let rhs = self.parse_equality()?;
            value = Value::Bool(coerce_bool(&value)? && coerce_bool(&rhs)?);
        }
        Ok(value)
    }

    fn parse_equality(&mut self) -> Result<Value> {
        let mut value = self.parse_comparison()?;
        loop {
            if self.consume(&Token::Eq) {
                value = Value::Bool(value == self.parse_comparison()?);
            } else if self.consume(&Token::Ne) {
                value = Value::Bool(value != self.parse_comparison()?);
            } else {
                return Ok(value);
            }
        }
    }

    fn parse_comparison(&mut self) -> Result<Value> {
        let mut value = self.parse_additive()?;
        loop {
            let op = if self.consume(&Token::Gte) {
                Some(Token::Gte)
            } else if self.consume(&Token::Lte) {
                Some(Token::Lte)
            } else if self.consume(&Token::Gt) {
                Some(Token::Gt)
            } else if self.consume(&Token::Lt) {
                Some(Token::Lt)
            } else {
                None
            };
            let Some(op) = op else {
                return Ok(value);
            };
            let lhs = as_f64(&value)?;
            let rhs = as_f64(&self.parse_additive()?)?;
            value = Value::Bool(match op {
                Token::Gte => lhs >= rhs,
                Token::Lte => lhs <= rhs,
                Token::Gt => lhs > rhs,
                Token::Lt => lhs < rhs,
                _ => unreachable!("comparison operator already filtered"),
            });
        }
    }

    fn parse_additive(&mut self) -> Result<Value> {
        let mut value = self.parse_unary()?;
        loop {
            if self.consume(&Token::Plus) {
                value = number_value(as_f64(&value)? + as_f64(&self.parse_unary()?)?)?;
            } else if self.consume(&Token::Minus) {
                value = number_value(as_f64(&value)? - as_f64(&self.parse_unary()?)?)?;
            } else {
                return Ok(value);
            }
        }
    }

    fn parse_unary(&mut self) -> Result<Value> {
        if self.consume(&Token::Not) {
            return Ok(Value::Bool(!coerce_bool(&self.parse_unary()?)?));
        }
        if self.consume(&Token::Minus) {
            return number_value(-as_f64(&self.parse_unary()?)?);
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Value> {
        let Some(token) = self.tokens.get(self.index) else {
            return Err(anyhow!("unexpected end of condition expression"));
        };
        self.index += 1;
        match token {
            Token::Bool(value) => Ok(Value::Bool(*value)),
            Token::Null => Ok(Value::Null),
            Token::Number(value) => number_value(*value),
            Token::String(value) => Ok(Value::String(value.clone())),
            Token::Path(path) => extract_path_value(self.context, path),
            Token::Ident(ident) => self.parse_function_call(ident),
            Token::LParen => {
                let value = self.parse_or()?;
                self.expect(Token::RParen)?;
                Ok(value)
            }
            other => Err(anyhow!("unexpected token in condition expression: {other:?}")),
        }
    }

    fn parse_function_call(&mut self, ident: &str) -> Result<Value> {
        self.expect(Token::LParen)?;
        let path = match self.tokens.get(self.index) {
            Some(Token::Path(path)) => path.clone(),
            _ => return Err(anyhow!("function '{ident}' expects a context path argument")),
        };
        self.index += 1;
        self.expect(Token::RParen)?;
        let value = lookup_path_value(self.context, &path);
        match ident {
            "exists" => Ok(Value::Bool(value.is_some())),
            "present" => Ok(Value::Bool(value.is_some_and(|item| !item.is_null()))),
            _ => Err(anyhow!("unsupported function in condition expression: {ident}")),
        }
    }

    fn consume(&mut self, token: &Token) -> bool {
        if self.tokens.get(self.index) == Some(token) {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, token: Token) -> Result<()> {
        if self.consume(&token) {
            Ok(())
        } else {
            Err(anyhow!("expected token {token:?} in condition expression"))
        }
    }
}

fn coerce_bool(value: &Value) -> Result<bool> {
    match value {
        Value::Bool(flag) => Ok(*flag),
        other => Err(anyhow!("expression is not boolean: {other}")),
    }
}

fn as_f64(value: &Value) -> Result<f64> {
    value
        .as_f64()
        .ok_or_else(|| anyhow!("expression is not numeric: {value}"))
}

fn number_value(value: f64) -> Result<Value> {
    if value.fract() == 0.0 && value.is_finite() {
        return Ok(Value::Number((value as i64).into()));
    }
    serde_json::Number::from_f64(value)
        .map(Value::Number)
        .ok_or_else(|| anyhow!("invalid numeric result in condition expression"))
}
