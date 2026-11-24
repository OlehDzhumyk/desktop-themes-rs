// SPDX-License-Identifier: GPL-3.0-or-later
//! The calculator provider (AC-SRCH-03): a recursive-descent parser for arithmetic only. It reads
//! numbers, `+ - * / % ^`, parentheses and unary signs; nothing is ever passed to a shell, an
//! interpreter or another process.
//!
//! ```text
//! expr    := term (('+' | '-') term)*
//! term    := unary (('*' | '/' | '%') unary)*
//! unary   := ('+' | '-') unary | power
//! power   := primary ('^' unary)?          right-associative; -2^2 = -4, 2^-1 = 0.5
//! primary := number | '(' expr ')'
//! ```
//!
//! `×`, `·` and `**` are accepted for `*` and `^`, `÷` for `/`, and a comma works as a decimal
//! separator (the Ukrainian habit).

use std::fmt;

/// Deepest nesting of parentheses and unary signs accepted.
pub const MAX_DEPTH: usize = 64;

/// Why an expression has no value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CalcError {
    /// A character or sequence that is not arithmetic.
    Syntax,
    /// Division or remainder by zero.
    DivisionByZero,
    /// The value is infinite or not a number.
    NotFinite,
    /// More than [`MAX_DEPTH`] nested levels.
    TooDeep,
}

impl fmt::Display for CalcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            CalcError::Syntax => "not an arithmetic expression",
            CalcError::DivisionByZero => "division by zero",
            CalcError::NotFinite => "the result is not a finite number",
            CalcError::TooDeep => "the expression is nested too deeply",
        })
    }
}

impl std::error::Error for CalcError {}

/// A value and how many binary operations produced it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Evaluation {
    pub value: f64,
    /// 0 for a bare number such as `42` or `(42)`: plain text that only looks like a number.
    pub operations: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Token {
    Num(f64),
    Op(char),
    Open,
    Close,
}

fn tokenize(src: &str) -> Result<Vec<Token>, CalcError> {
    let mut out = Vec::new();
    let mut chars = src.chars().peekable();
    while let Some(&c) = chars.peek() {
        match c {
            c if c.is_whitespace() => {
                chars.next();
            }
            '0'..='9' | '.' | ',' => {
                let mut text = String::new();
                while let Some(&d) = chars.peek() {
                    match d {
                        '0'..='9' => text.push(d),
                        '.' | ',' => text.push('.'),
                        _ => break,
                    }
                    chars.next();
                }
                if text == "." || text.matches('.').count() > 1 {
                    return Err(CalcError::Syntax);
                }
                out.push(Token::Num(text.parse().map_err(|_| CalcError::Syntax)?));
            }
            '*' => {
                chars.next();
                if chars.peek() == Some(&'*') {
                    chars.next();
                    out.push(Token::Op('^'));
                } else {
                    out.push(Token::Op('*'));
                }
            }
            '+' | '-' | '/' | '%' | '^' => {
                chars.next();
                out.push(Token::Op(c));
            }
            '×' | '·' => {
                chars.next();
                out.push(Token::Op('*'));
            }
            '÷' => {
                chars.next();
                out.push(Token::Op('/'));
            }
            '−' => {
                chars.next();
                out.push(Token::Op('-'));
            }
            '(' => {
                chars.next();
                out.push(Token::Open);
            }
            ')' => {
                chars.next();
                out.push(Token::Close);
            }
            _ => return Err(CalcError::Syntax),
        }
    }
    Ok(out)
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    depth: usize,
    operations: usize,
}

impl Parser {
    fn peek(&self) -> Option<Token> {
        self.tokens.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<Token> {
        let t = self.peek();
        self.pos += 1;
        t
    }

    fn enter(&mut self) -> Result<(), CalcError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            Err(CalcError::TooDeep)
        } else {
            Ok(())
        }
    }

    fn expr(&mut self) -> Result<f64, CalcError> {
        let mut acc = self.term()?;
        while let Some(Token::Op(op @ ('+' | '-'))) = self.peek() {
            self.pos += 1;
            let rhs = self.term()?;
            self.operations += 1;
            acc = if op == '+' { acc + rhs } else { acc - rhs };
        }
        Ok(acc)
    }

    fn term(&mut self) -> Result<f64, CalcError> {
        let mut acc = self.unary()?;
        while let Some(Token::Op(op @ ('*' | '/' | '%'))) = self.peek() {
            self.pos += 1;
            let rhs = self.unary()?;
            self.operations += 1;
            acc = match op {
                '*' => acc * rhs,
                _ if rhs == 0.0 => return Err(CalcError::DivisionByZero),
                '/' => acc / rhs,
                _ => acc % rhs,
            };
        }
        Ok(acc)
    }

    fn unary(&mut self) -> Result<f64, CalcError> {
        match self.peek() {
            Some(Token::Op(sign @ ('+' | '-'))) => {
                self.pos += 1;
                self.enter()?;
                let v = self.unary()?;
                self.depth -= 1;
                Ok(if sign == '-' { -v } else { v })
            }
            _ => self.power(),
        }
    }

    fn power(&mut self) -> Result<f64, CalcError> {
        let base = self.primary()?;
        if self.peek() == Some(Token::Op('^')) {
            self.pos += 1;
            self.enter()?;
            let exp = self.unary()?;
            self.depth -= 1;
            self.operations += 1;
            return Ok(base.powf(exp));
        }
        Ok(base)
    }

    fn primary(&mut self) -> Result<f64, CalcError> {
        match self.bump() {
            Some(Token::Num(v)) => Ok(v),
            Some(Token::Open) => {
                self.enter()?;
                let v = self.expr()?;
                self.depth -= 1;
                match self.bump() {
                    Some(Token::Close) => Ok(v),
                    _ => Err(CalcError::Syntax),
                }
            }
            _ => Err(CalcError::Syntax),
        }
    }
}

/// Evaluate an arithmetic expression.
pub fn evaluate(src: &str) -> Result<Evaluation, CalcError> {
    let tokens = tokenize(src)?;
    if tokens.is_empty() {
        return Err(CalcError::Syntax);
    }
    let mut p = Parser {
        tokens,
        pos: 0,
        depth: 0,
        operations: 0,
    };
    let value = p.expr()?;
    if p.pos != p.tokens.len() {
        return Err(CalcError::Syntax);
    }
    if !value.is_finite() {
        return Err(CalcError::NotFinite);
    }
    Ok(Evaluation {
        value,
        operations: p.operations,
    })
}

/// A value as the launcher shows and copies it: integers without a decimal point, other values
/// with at most ten decimals and no trailing zeros, very large or small values in `e` notation.
pub fn format_number(value: f64) -> String {
    let abs = value.abs();
    if abs == 0.0 {
        return "0".to_owned();
    }
    if !(1e-9..1e15).contains(&abs) {
        return format!("{value:e}");
    }
    let text = format!("{value:.10}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" {
        "0".to_owned()
    } else {
        text.to_owned()
    }
}

/// The text of the result when `src` is a calculation with at least one operation (plain text
/// in the all-providers scope, `42` alone is not a calculation).
pub fn implicit(src: &str) -> Option<String> {
    let e = evaluate(src).ok()?;
    (e.operations > 0).then(|| format_number(e.value))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calc(src: &str) -> String {
        format_number(evaluate(src).unwrap().value)
    }

    #[test]
    fn ac_srch_03_twelve_times_three_plus_four_is_84() {
        assert_eq!(calc("12*(3+4)"), "84");
        assert_eq!(calc(" 12 * ( 3 + 4 ) "), "84");
    }

    #[test]
    fn precedence_associativity_and_signs() {
        for (src, want) in [
            ("2+2*3", "8"),
            ("(2+2)*3", "12"),
            ("10-4-3", "3"),
            ("100/10/5", "2"),
            ("2^3^2", "512"),
            ("-2^2", "-4"),
            ("2^-1", "0.5"),
            ("-(3+4)", "-7"),
            ("+5", "5"),
            ("--5", "5"),
            ("7%3", "1"),
            ("-7%3", "-1"),
            ("1/3", "0.3333333333"),
            ("0.1+0.2", "0.3"),
            ("1,5*2", "3"),
            (".5*4", "2"),
            ("3×4÷2", "6"),
            ("2**10", "1024"),
            ("5−8", "-3"),
            ("10^20", "1e20"),
            ("0*-1", "0"),
        ] {
            assert_eq!(calc(src), want, "{src}");
        }
    }

    #[test]
    fn errors_never_panic() {
        for (src, err) in [
            ("", CalcError::Syntax),
            ("1/0", CalcError::DivisionByZero),
            ("5%0", CalcError::DivisionByZero),
            ("10^400", CalcError::NotFinite),
            ("(-8)^0.5", CalcError::NotFinite),
            ("2+", CalcError::Syntax),
            ("(1+2", CalcError::Syntax),
            ("1+2)", CalcError::Syntax),
            ("1..2", CalcError::Syntax),
            ("1.2.3", CalcError::Syntax),
            ("rm -rf /", CalcError::Syntax),
            ("$(reboot)", CalcError::Syntax),
            ("`id`", CalcError::Syntax),
            ("2 3", CalcError::Syntax),
            ("sqrt(4)", CalcError::Syntax),
        ] {
            assert_eq!(evaluate(src).unwrap_err(), err, "{src}");
        }
        let deep = format!("{}1{}", "(".repeat(100), ")".repeat(100));
        assert_eq!(evaluate(&deep).unwrap_err(), CalcError::TooDeep);
        assert_eq!(evaluate(&"-".repeat(200)).unwrap_err(), CalcError::TooDeep);
    }

    #[test]
    fn only_real_calculations_are_implicit() {
        assert_eq!(implicit("2+2").as_deref(), Some("4"));
        assert_eq!(implicit("(3)*3").as_deref(), Some("9"));
        assert_eq!(implicit("42"), None);
        assert_eq!(implicit("(42)"), None);
        assert_eq!(implicit("-5"), None);
        assert_eq!(implicit("firefox"), None);
        assert_eq!(implicit("2+"), None);
    }
}
