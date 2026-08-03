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
    Lexer::new(raw).tokenize()
}

struct Lexer<'a> {
    raw: &'a str,
    chars: Vec<char>,
    tokens: Vec<Token>,
    index: usize,
}

impl<'a> Lexer<'a> {
    fn new(raw: &'a str) -> Self {
        Self {
            raw,
            chars: raw.chars().collect(),
            tokens: Vec::new(),
            index: 0,
        }
    }

    fn tokenize(mut self) -> Result<Vec<Token>> {
        while self.index < self.chars.len() {
            self.tokenize_next()?;
        }
        Ok(self.tokens)
    }

    fn tokenize_next(&mut self) -> Result<()> {
        let current = self.chars[self.index];
        if current.is_whitespace() {
            self.index += 1;
            return Ok(());
        }
        if let Some((token, width)) = operator_token(current, self.chars.get(self.index + 1)) {
            self.push(token, width);
            return Ok(());
        }
        match current {
            '"' => self.tokenize_string(),
            '.' => {
                self.tokenize_path();
                Ok(())
            }
            c if c.is_ascii_digit() => self.tokenize_number(),
            c if c.is_ascii_alphabetic() || c == '_' => {
                self.tokenize_ident();
                Ok(())
            }
            other => Err(anyhow!(
                "unsupported token '{other}' in condition: {}",
                self.raw
            )),
        }
    }

    fn push(&mut self, token: Token, width: usize) {
        self.tokens.push(token);
        self.index += width;
    }

    fn tokenize_string(&mut self) -> Result<()> {
        let mut value = String::new();
        self.index += 1;
        while self.index < self.chars.len() {
            match self.chars[self.index] {
                '"' => break,
                '\\' if self.index + 1 < self.chars.len() => {
                    value.push(self.chars[self.index + 1]);
                    self.index += 2;
                }
                other => {
                    value.push(other);
                    self.index += 1;
                }
            }
        }
        if self.chars.get(self.index) != Some(&'"') {
            return Err(anyhow!(
                "unterminated string literal in condition: {}",
                self.raw
            ));
        }
        self.push(Token::String(value), 1);
        Ok(())
    }

    fn tokenize_path(&mut self) {
        let start = self.index;
        self.index += 1;
        while self
            .chars
            .get(self.index)
            .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
        {
            self.index += 1;
        }
        self.tokens
            .push(Token::Path(self.chars[start..self.index].iter().collect()));
    }

    fn tokenize_number(&mut self) -> Result<()> {
        let start = self.index;
        self.index += 1;
        while self
            .chars
            .get(self.index)
            .is_some_and(|c| c.is_ascii_digit() || *c == '.')
        {
            self.index += 1;
        }
        let raw_number = self.chars[start..self.index].iter().collect::<String>();
        let number = raw_number
            .parse::<f64>()
            .map_err(|_| anyhow!("invalid numeric literal in condition: {raw_number}"))?;
        self.tokens.push(Token::Number(number));
        Ok(())
    }

    fn tokenize_ident(&mut self) {
        let start = self.index;
        self.index += 1;
        while self
            .chars
            .get(self.index)
            .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_')
        {
            self.index += 1;
        }
        let ident = self.chars[start..self.index].iter().collect::<String>();
        self.tokens.push(match ident.as_str() {
            "true" => Token::Bool(true),
            "false" => Token::Bool(false),
            "null" => Token::Null,
            _ => Token::Ident(ident),
        });
    }
}

fn operator_token(current: char, next: Option<&char>) -> Option<(Token, usize)> {
    two_char_operator(current, next)
        .map(|token| (token, 2))
        .or_else(|| one_char_operator(current).map(|token| (token, 1)))
}

fn two_char_operator(current: char, next: Option<&char>) -> Option<Token> {
    match (current, next.copied()) {
        ('&', Some('&')) => Some(Token::And),
        ('|', Some('|')) => Some(Token::Or),
        ('=', Some('=')) => Some(Token::Eq),
        ('!', Some('=')) => Some(Token::Ne),
        ('>', Some('=')) => Some(Token::Gte),
        ('<', Some('=')) => Some(Token::Lte),
        _ => None,
    }
}

fn one_char_operator(current: char) -> Option<Token> {
    match current {
        '>' => Some(Token::Gt),
        '<' => Some(Token::Lt),
        '+' => Some(Token::Plus),
        '-' => Some(Token::Minus),
        '!' => Some(Token::Not),
        '(' => Some(Token::LParen),
        ')' => Some(Token::RParen),
        _ => None,
    }
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
            other => Err(anyhow!(
                "unexpected token in condition expression: {other:?}"
            )),
        }
    }

    fn parse_function_call(&mut self, ident: &str) -> Result<Value> {
        self.expect(Token::LParen)?;
        let path = match self.tokens.get(self.index) {
            Some(Token::Path(path)) => path.clone(),
            _ => {
                return Err(anyhow!(
                    "function '{ident}' expects a context path argument"
                ))
            }
        };
        self.index += 1;
        self.expect(Token::RParen)?;
        let value = lookup_path_value(self.context, &path);
        match ident {
            "exists" => Ok(Value::Bool(value.is_some())),
            "present" => Ok(Value::Bool(value.is_some_and(|item| !item.is_null()))),
            _ => Err(anyhow!(
                "unsupported function in condition expression: {ident}"
            )),
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
