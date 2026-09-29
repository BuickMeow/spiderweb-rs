//! Math expressions in numeric fields (port of Python `files/mathexpr.py`).
//!
//! - [`calc`]: simple expressions such as `960*4`, `(60+4)*16`; only numbers and
//!   `+ - * / // % ** ( )`, with `x` and `×` as multiplication and `^` as `**` (note the
//!   original first turns `^` into `**`, so a hand-written `**` becomes `****` and errors —
//!   same here); supports Python's int / float / complex semantics.
//! - [`formula`]: formulas such as `x^2`, `sin(x*pi/2)` -> a function of x; names are `x`,
//!   `pi`, `e` and [`FORMULA_FUNCS`], with error messages matching the original.
//! - [`calc_int`], [`fmt`] as in the original.
//!
//! Differences from the original: arbitrary-precision integers only use i128 (falling back to
//! floats beyond that); a fractional power of a negative base returns a complex number, and
//! the last bits of complex powers may differ slightly from CPython.

use std::f64::consts::{E, PI};

/// An expression's value (Python's int / float / complex).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CalcValue {
    Int(i128),
    Float(f64),
    Complex(f64, f64),
}

impl CalcValue {
    /// The real part; complex numbers return None.
    pub fn as_f64(self) -> Option<f64> {
        match self {
            CalcValue::Int(i) => Some(i as f64),
            CalcValue::Float(f) => Some(f),
            CalcValue::Complex(..) => None,
        }
    }

    pub fn is_complex(self) -> bool {
        matches!(self, CalcValue::Complex(..))
    }
}

/// Evaluation error; the messages correspond to the original's ValueError / OverflowError.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum MathError {
    #[error("division by zero")]
    DivisionByZero,
    #[error("whole number needed")]
    WholeNumber,
    #[error("out of range")]
    OutOfRange,
    #[error("{0}")]
    Message(String),
}

/// Constants available in formulas (FORMULA_NAMES).
pub const FORMULA_NAMES: [(&str, f64); 2] = [("pi", PI), ("e", E)];

/// Functions available in formulas (FORMULA_FUNCS).
pub const FORMULA_FUNCS: [&str; 22] = [
    "sin", "cos", "tan", "asin", "acos", "atan", "sinh", "cosh", "tanh", "sqrt", "exp", "log",
    "ln", "log10", "log2", "abs", "min", "max", "floor", "ceil", "round", "pow",
];

// ---------------------------------------------------------------- values

#[derive(Clone, Copy, Debug)]
enum Num {
    Int(i128),
    Float(f64),
    Complex(f64, f64),
}

impl Num {
    fn value(self) -> CalcValue {
        match self {
            Num::Int(i) => CalcValue::Int(i),
            Num::Float(f) => CalcValue::Float(f),
            Num::Complex(a, b) => CalcValue::Complex(a, b),
        }
    }
}

fn cnum(z: Num) -> (f64, f64) {
    match z {
        Num::Int(i) => (i as f64, 0.0),
        Num::Float(f) => (f, 0.0),
        Num::Complex(a, b) => (a, b),
    }
}

fn is_complex(z: Num) -> bool {
    matches!(z, Num::Complex(..))
}

fn abs_num(z: Num) -> f64 {
    match z {
        Num::Int(i) => i.unsigned_abs() as f64,
        Num::Float(f) => f.abs(),
        Num::Complex(a, b) => a.hypot(b),
    }
}

fn add(a: Num, b: Num) -> Num {
    if !is_complex(a)
        && !is_complex(b)
        && let (Num::Int(x), Num::Int(y)) = (a, b)
        && let Some(v) = x.checked_add(y)
    {
        return Num::Int(v);
    }
    let (ar, ai) = cnum(a);
    let (br, bi) = cnum(b);
    if ai == 0.0 && bi == 0.0 {
        Num::Float(ar + br)
    } else {
        Num::Complex(ar + br, ai + bi)
    }
}

fn sub(a: Num, b: Num) -> Num {
    if !is_complex(a)
        && !is_complex(b)
        && let (Num::Int(x), Num::Int(y)) = (a, b)
        && let Some(v) = x.checked_sub(y)
    {
        return Num::Int(v);
    }
    let (ar, ai) = cnum(a);
    let (br, bi) = cnum(b);
    if ai == 0.0 && bi == 0.0 {
        Num::Float(ar - br)
    } else {
        Num::Complex(ar - br, ai - bi)
    }
}

fn mul(a: Num, b: Num) -> Num {
    if !is_complex(a)
        && !is_complex(b)
        && let (Num::Int(x), Num::Int(y)) = (a, b)
        && let Some(v) = x.checked_mul(y)
    {
        return Num::Int(v);
    }
    let (ar, ai) = cnum(a);
    let (br, bi) = cnum(b);
    if ai == 0.0 && bi == 0.0 {
        Num::Float(ar * br)
    } else {
        Num::Complex(ar * br - ai * bi, ar * bi + ai * br)
    }
}

fn div(a: Num, b: Num) -> Result<Num, MathError> {
    let (ar, ai) = cnum(a);
    let (br, bi) = cnum(b);
    if ai == 0.0 && bi == 0.0 {
        if br == 0.0 {
            return Err(MathError::DivisionByZero);
        }
        return Ok(Num::Float(ar / br));
    }
    if br == 0.0 && bi == 0.0 {
        return Err(MathError::DivisionByZero);
    }
    // Python's complex division: scale by the component with the larger absolute value.
    if br.abs() >= bi.abs() {
        let ratio = bi / br;
        let den = br + bi * ratio;
        Ok(Num::Complex(
            (ar + ai * ratio) / den,
            (ai - ar * ratio) / den,
        ))
    } else {
        let ratio = br / bi;
        let den = br * ratio + bi;
        Ok(Num::Complex(
            (ar * ratio + ai) / den,
            (ai * ratio - ar) / den,
        ))
    }
}

/// CPython `float_divmod` (floatobject.c): fmod first, then fix the sign; the quotient snaps
/// to the nearest integer. A plain `(x / y).floor()` is off by 1 when x/y lands near an
/// integer boundary; this version matches the original.
fn float_divmod(vx: f64, wx: f64) -> Result<(f64, f64), MathError> {
    if wx == 0.0 {
        return Err(MathError::DivisionByZero);
    }
    let mut mod_ = vx % wx;
    let mut div = (vx - mod_) / wx;
    if mod_ != 0.0 {
        if (wx < 0.0) != (mod_ < 0.0) {
            mod_ += wx;
            div -= 1.0;
        }
    } else {
        mod_ = 0.0f64.copysign(wx);
    }
    let floordiv = if div != 0.0 {
        let f = div.floor();
        if div - f > 0.5 { f + 1.0 } else { f }
    } else {
        0.0f64.copysign(vx / wx)
    };
    Ok((floordiv, mod_))
}

fn floordiv(a: Num, b: Num) -> Result<Num, MathError> {
    if is_complex(a) || is_complex(b) {
        return Err(MathError::Message(
            "can't take floor of complex number".into(),
        ));
    }
    if let (Num::Int(x), Num::Int(y)) = (a, b) {
        if y == 0 {
            return Err(MathError::DivisionByZero);
        }
        let q = x / y;
        let extra = x % y != 0 && ((x < 0) != (y < 0));
        return Ok(Num::Int(if extra { q - 1 } else { q }));
    }
    let x = a.value().as_f64().unwrap_or(0.0);
    let y = b.value().as_f64().unwrap_or(0.0);
    Ok(Num::Float(float_divmod(x, y)?.0))
}

fn modulo(a: Num, b: Num) -> Result<Num, MathError> {
    if is_complex(a) || is_complex(b) {
        return Err(MathError::Message("can't mod complex numbers".into()));
    }
    if let (Num::Int(x), Num::Int(y)) = (a, b) {
        if y == 0 {
            return Err(MathError::DivisionByZero);
        }
        let mut r = x % y;
        if r != 0 && (r < 0) != (y < 0) {
            r += y;
        }
        return Ok(Num::Int(r));
    }
    let x = a.value().as_f64().unwrap_or(0.0);
    let y = b.value().as_f64().unwrap_or(0.0);
    Ok(Num::Float(float_divmod(x, y)?.1))
}

/// `(a+bi)^(c+di)` via the usual exp(w·ln z) algorithm (last bits may differ from the original).
fn cpow(a: f64, b: f64, c: f64, d: f64) -> Result<(f64, f64), MathError> {
    if a == 0.0 && b == 0.0 {
        if c == 0.0 && d == 0.0 {
            return Ok((1.0, 0.0));
        }
        if d == 0.0 && c > 0.0 {
            return Ok((0.0, 0.0));
        }
        return Err(MathError::DivisionByZero);
    }
    let r = a.hypot(b);
    let theta = b.atan2(a);
    let (lr, li) = (r.ln(), theta);
    let re = c * lr - d * li;
    let im = c * li + d * lr;
    let mag = re.exp();
    Ok((mag * im.cos(), mag * im.sin()))
}

fn pow(a: Num, b: Num) -> Result<Num, MathError> {
    if abs_num(b) > 64.0 {
        return Err(MathError::Message("exponent too large".into()));
    }
    if !is_complex(a) && !is_complex(b) {
        match (a, b) {
            (Num::Int(x), Num::Int(y)) if y >= 0 => {
                if let Ok(y) = u32::try_from(y)
                    && let Some(v) = x.checked_pow(y)
                {
                    return Ok(Num::Int(v));
                }
                // Integers beyond i128: the original keeps exact integers, this falls back to floats (see the module docs)
                return pow_float(x as f64, y as f64);
            }
            (Num::Int(x), Num::Int(y)) => {
                // Negative exponent -> float; an integer power of a negative base also goes through floats.
                return pow_float(x as f64, y as f64);
            }
            _ => {
                let x = a.value().as_f64().unwrap_or(0.0);
                let y = b.value().as_f64().unwrap_or(0.0);
                return pow_float(x, y);
            }
        }
    }
    let (ar, ai) = cnum(a);
    let (br, bi) = cnum(b);
    let (re, im) = cpow(ar, ai, br, bi)?;
    Ok(Num::Complex(re, im))
}

fn pow_float(base: f64, exp: f64) -> Result<Num, MathError> {
    if base < 0.0 && exp.is_finite() && exp.fract() != 0.0 {
        let (re, im) = cpow(base, 0.0, exp, 0.0)?;
        return Ok(Num::Complex(re, im));
    }
    if base == 0.0 && exp < 0.0 {
        return Err(MathError::DivisionByZero);
    }
    let v = base.powf(exp);
    if v.is_infinite() && base.is_finite() && exp.is_finite() && base != 0.0 {
        return Err(MathError::Message("Result too large".into()));
    }
    Ok(Num::Float(v))
}

fn neg(a: Num) -> Num {
    match a {
        Num::Int(i) => Num::Int(-i),
        Num::Float(f) => Num::Float(-f),
        Num::Complex(x, y) => Num::Complex(-x, -y),
    }
}

fn as_float(a: Num) -> Result<f64, MathError> {
    match a {
        Num::Int(i) => Ok(i as f64),
        Num::Float(f) => Ok(f),
        Num::Complex(..) => Err(MathError::Message("can't convert complex to float".into())),
    }
}

fn math_func(name: &str, args: &[Num]) -> Result<Num, MathError> {
    let one = |args: &[Num]| -> Result<f64, MathError> {
        if args.len() != 1 {
            return Err(MathError::Message(format!(
                "{name}() takes exactly one argument ({} given)",
                args.len()
            )));
        }
        as_float(args[0])
    };
    let domain = || Err(MathError::Message("math domain error".into()));
    let range = || Err(MathError::Message("math range error".into()));
    match name {
        "sin" | "cos" | "tan" => {
            let x = one(args)?;
            if !x.is_finite() {
                return domain();
            }
            Ok(Num::Float(match name {
                "sin" => x.sin(),
                "cos" => x.cos(),
                _ => x.tan(),
            }))
        }
        "asin" | "acos" => {
            let x = one(args)?;
            if !(-1.0..=1.0).contains(&x) {
                return domain();
            }
            Ok(Num::Float(if name == "asin" { x.asin() } else { x.acos() }))
        }
        "atan" => Ok(Num::Float(one(args)?.atan())),
        "sinh" | "cosh" | "exp" => {
            let x = one(args)?;
            let v = match name {
                "sinh" => x.sinh(),
                "cosh" => x.cosh(),
                _ => x.exp(),
            };
            // Overflow (finite input, infinite result): the original raises OverflowError("math range error")
            if v.is_infinite() && x.is_finite() {
                return range();
            }
            Ok(Num::Float(v))
        }
        "tanh" => Ok(Num::Float(one(args)?.tanh())),
        "sqrt" => {
            let x = one(args)?;
            if x < 0.0 {
                return domain();
            }
            Ok(Num::Float(x.sqrt()))
        }
        "log" | "ln" | "log10" | "log2" => {
            let x = one(args)?;
            if x <= 0.0 {
                return domain();
            }
            Ok(Num::Float(match name {
                "log" | "ln" => x.ln(),
                "log10" => x.log10(),
                _ => x.log2(),
            }))
        }
        "abs" => {
            if args.len() != 1 {
                return Err(MathError::Message(
                    "abs() takes exactly one argument".into(),
                ));
            }
            Ok(match args[0] {
                Num::Int(i) => Num::Int(i.unsigned_abs() as i128),
                Num::Float(f) => Num::Float(f.abs()),
                Num::Complex(a, b) => Num::Float(a.hypot(b)),
            })
        }
        "min" | "max" => {
            if args.len() < 2 {
                return Err(MathError::Message(format!(
                    "{name} expected at least 2 arguments, got {}",
                    args.len()
                )));
            }
            let mut best = args[0];
            for &a in &args[1..] {
                let less = match (as_float(best), as_float(a)) {
                    (Ok(x), Ok(y)) => x < y,
                    _ => {
                        return Err(MathError::Message(
                            "'<' not supported between instances".into(),
                        ));
                    }
                };
                if (name == "min" && less) || (name == "max" && !less) {
                    best = a;
                }
            }
            Ok(best)
        }
        "floor" | "ceil" => {
            if args.len() != 1 {
                return Err(MathError::Message(format!(
                    "{name}() takes exactly one argument"
                )));
            }
            match args[0] {
                Num::Int(i) => Ok(Num::Int(i)),
                Num::Float(f) => {
                    if f.is_nan() {
                        return Err(MathError::Message(
                            "cannot convert float NaN to integer".into(),
                        ));
                    }
                    if f.is_infinite() {
                        return Err(MathError::Message(
                            "cannot convert float infinity to integer".into(),
                        ));
                    }
                    Ok(Num::Int(if name == "floor" {
                        f.floor() as i128
                    } else {
                        f.ceil() as i128
                    }))
                }
                Num::Complex(..) => Err(MathError::Message(format!(
                    "can't take {name} of complex number"
                ))),
            }
        }
        "round" => {
            if args.len() != 1 {
                return Err(MathError::Message(
                    "round() takes exactly one argument".into(),
                ));
            }
            match args[0] {
                Num::Int(i) => Ok(Num::Int(i)),
                Num::Float(f) => {
                    if f.is_nan() {
                        return Err(MathError::Message(
                            "cannot convert float NaN to integer".into(),
                        ));
                    }
                    if f.is_infinite() {
                        return Err(MathError::Message(
                            "cannot convert float infinity to integer".into(),
                        ));
                    }
                    Ok(Num::Int(spiderweb_core::round_half_even(f) as i128))
                }
                Num::Complex(..) => Err(MathError::Message(
                    "type complex doesn't define __round__ method".into(),
                )),
            }
        }
        "pow" => {
            if args.len() != 2 {
                return Err(MathError::Message("pow() takes exactly 2 arguments".into()));
            }
            pow(args[0], args[1])
        }
        _ => Err(MathError::Message(format!("unknown function \"{name}\""))),
    }
}

// ---------------------------------------------------------------- lexer / parser

#[derive(Clone, Debug)]
enum Tok {
    Num(Num),
    Ident(String),
    Plus,
    Minus,
    Mul,
    Div,
    FloorDiv,
    Mod,
    Pow,
    LParen,
    RParen,
    Comma,
    End,
}

fn number(cs: &[char], start: usize) -> Result<(Num, usize), ()> {
    let mut i = start;
    if cs[i] == '0'
        && let Some(&c2) = cs.get(i + 1)
    {
        let radix = match c2 {
            'x' | 'X' => 16,
            'o' | 'O' => 8,
            'b' | 'B' => 2,
            _ => 0,
        };
        if radix != 0 {
            i += 2;
            let mut digits: Vec<u32> = Vec::new();
            let mut prev_digit = false;
            while let Some(&c) = cs.get(i) {
                if c == '_' {
                    if !prev_digit {
                        return Err(());
                    }
                    prev_digit = false;
                    i += 1;
                    continue;
                }
                if let Some(d) = c.to_digit(radix) {
                    digits.push(d);
                    prev_digit = true;
                    i += 1;
                } else {
                    break;
                }
            }
            if digits.is_empty() || !prev_digit {
                return Err(());
            }
            let mut value: i128 = 0;
            for d in digits {
                value = value.checked_mul(radix as i128).ok_or(())?;
                value = value.checked_add(d as i128).ok_or(())?;
            }
            return Ok((Num::Int(value), i));
        }
    }
    let mut int_digits = String::new();
    let mut frac_digits = String::new();
    let mut exponent: Option<String> = None;
    let mut seen_dot = false;
    let mut prev_digit = false;
    let mut last_under = false;
    while let Some(&c) = cs.get(i) {
        if c.is_ascii_digit() {
            if seen_dot {
                frac_digits.push(c);
            } else {
                int_digits.push(c);
            }
            prev_digit = true;
            last_under = false;
            i += 1;
        } else if c == '_' {
            if !prev_digit {
                return Err(());
            }
            prev_digit = false;
            last_under = true;
            i += 1;
        } else if c == '.' && !seen_dot && exponent.is_none() {
            if last_under {
                return Err(());
            }
            seen_dot = true;
            prev_digit = false;
            i += 1;
        } else if (c == 'e' || c == 'E')
            && exponent.is_none()
            && !last_under
            && !(int_digits.is_empty() && frac_digits.is_empty())
        {
            let mut j = i + 1;
            let mut exp = String::new();
            if let Some(&sign @ ('+' | '-')) = cs.get(j) {
                exp.push(sign);
                j += 1;
            }
            let mut any = false;
            let mut last_digit = false;
            while let Some(&d) = cs.get(j) {
                if d.is_ascii_digit() {
                    exp.push(d);
                    any = true;
                    last_digit = true;
                    j += 1;
                } else if d == '_' {
                    if !last_digit {
                        return Err(());
                    }
                    last_digit = false;
                    j += 1;
                } else {
                    break;
                }
            }
            if !any || !last_digit {
                return Err(());
            }
            exponent = Some(exp);
            i = j;
            break;
        } else {
            break;
        }
    }
    if last_under || (int_digits.is_empty() && frac_digits.is_empty()) {
        return Err(());
    }
    match exponent {
        None if !seen_dot => int_digits.parse::<i128>().map_or_else(
            |_| {
                int_digits
                    .parse::<f64>()
                    .map(|f| (Num::Float(f), i))
                    .map_err(|_| ())
            },
            |v| Ok((Num::Int(v), i)),
        ),
        _ => {
            let mut text = String::new();
            text.push_str(if int_digits.is_empty() {
                "0"
            } else {
                &int_digits
            });
            text.push('.');
            text.push_str(if frac_digits.is_empty() {
                "0"
            } else {
                &frac_digits
            });
            if let Some(e) = &exponent {
                text.push('e');
                text.push_str(e);
            }
            text.parse::<f64>()
                .map(|f| (Num::Float(f), i))
                .map_err(|_| ())
        }
    }
}

fn tokenize(text: &str) -> Result<Vec<Tok>, ()> {
    let cs: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c.is_ascii_digit() || (c == '.' && cs.get(i + 1).is_some_and(|d| d.is_ascii_digit())) {
            let (n, ni) = number(&cs, i)?;
            out.push(Tok::Num(n));
            i = ni;
            continue;
        }
        if c == '_' || c.is_alphabetic() {
            let mut j = i + 1;
            while cs.get(j).is_some_and(|&c| c == '_' || c.is_alphanumeric()) {
                j += 1;
            }
            let word: String = cs[i..j].iter().collect();
            match word.as_str() {
                "True" => out.push(Tok::Num(Num::Int(1))),
                "False" => out.push(Tok::Num(Num::Int(0))),
                _ => out.push(Tok::Ident(word)),
            }
            i = j;
            continue;
        }
        match c {
            '(' => out.push(Tok::LParen),
            ')' => out.push(Tok::RParen),
            '+' => out.push(Tok::Plus),
            '-' => out.push(Tok::Minus),
            '*' => {
                if cs.get(i + 1) == Some(&'*') {
                    out.push(Tok::Pow);
                    i += 1;
                } else {
                    out.push(Tok::Mul);
                }
            }
            '/' => {
                if cs.get(i + 1) == Some(&'/') {
                    out.push(Tok::FloorDiv);
                    i += 1;
                } else {
                    out.push(Tok::Div);
                }
            }
            '%' => out.push(Tok::Mod),
            ',' => out.push(Tok::Comma),
            _ => return Err(()),
        }
        i += 1;
    }
    out.push(Tok::End);
    Ok(out)
}

#[derive(Clone, Debug)]
enum Ast {
    Num(Num),
    Name(String),
    Neg(Box<Ast>),
    Pos(Box<Ast>),
    Bin(&'static str, Box<Ast>, Box<Ast>),
    Call(String, Vec<Ast>),
}

struct Parser {
    toks: Vec<Tok>,
    i: usize,
}

impl Parser {
    fn peek(&self) -> &Tok {
        self.toks.get(self.i).unwrap_or(&Tok::End)
    }

    fn expr(&mut self) -> Result<Ast, ()> {
        let mut left = self.term()?;
        loop {
            match self.peek() {
                Tok::Plus => {
                    self.i += 1;
                    left = Ast::Bin("+", Box::new(left), Box::new(self.term()?));
                }
                Tok::Minus => {
                    self.i += 1;
                    left = Ast::Bin("-", Box::new(left), Box::new(self.term()?));
                }
                _ => return Ok(left),
            }
        }
    }

    fn term(&mut self) -> Result<Ast, ()> {
        let mut left = self.unary()?;
        loop {
            let op = match self.peek() {
                Tok::Mul => "*",
                Tok::Div => "/",
                Tok::FloorDiv => "//",
                Tok::Mod => "%",
                _ => return Ok(left),
            };
            self.i += 1;
            left = Ast::Bin(op, Box::new(left), Box::new(self.unary()?));
        }
    }

    fn unary(&mut self) -> Result<Ast, ()> {
        match self.peek() {
            Tok::Minus => {
                self.i += 1;
                Ok(Ast::Neg(Box::new(self.unary()?)))
            }
            Tok::Plus => {
                self.i += 1;
                Ok(Ast::Pos(Box::new(self.unary()?)))
            }
            _ => self.power(),
        }
    }

    fn power(&mut self) -> Result<Ast, ()> {
        let base = self.atom()?;
        if matches!(self.peek(), Tok::Pow) {
            self.i += 1;
            let exp = self.unary()?;
            return Ok(Ast::Bin("**", Box::new(base), Box::new(exp)));
        }
        Ok(base)
    }

    fn atom(&mut self) -> Result<Ast, ()> {
        match self.peek().clone() {
            Tok::Num(n) => {
                self.i += 1;
                Ok(Ast::Num(n))
            }
            Tok::Ident(name) => {
                self.i += 1;
                if matches!(self.peek(), Tok::LParen) {
                    self.i += 1;
                    let mut args = Vec::new();
                    if !matches!(self.peek(), Tok::RParen) {
                        loop {
                            args.push(self.expr()?);
                            match self.peek() {
                                Tok::Comma => {
                                    self.i += 1;
                                }
                                Tok::RParen => break,
                                _ => return Err(()),
                            }
                        }
                    }
                    self.i += 1; // ')'
                    Ok(Ast::Call(name, args))
                } else {
                    Ok(Ast::Name(name))
                }
            }
            Tok::LParen => {
                self.i += 1;
                let inner = self.expr()?;
                if !matches!(self.peek(), Tok::RParen) {
                    return Err(());
                }
                self.i += 1;
                Ok(inner)
            }
            _ => Err(()),
        }
    }
}

/// Python `ast.parse(mode="eval")`'s IndentationError for a first-line indent; `calc` does not strip its input.
fn has_indent(text: &str) -> bool {
    for line in text.split('\n') {
        if line.trim().is_empty() {
            continue;
        }
        return line.starts_with([' ', '\t', '\u{0c}']);
    }
    false
}

fn parse(text: &str) -> Result<Ast, ()> {
    if has_indent(text) {
        return Err(());
    }
    let toks = tokenize(text)?;
    let mut p = Parser { toks, i: 0 };
    let ast = p.expr()?;
    if !matches!(p.peek(), Tok::End) {
        return Err(());
    }
    Ok(ast)
}

// ---------------------------------------------------------------- evaluation

fn eval(ast: &Ast, x: Option<f64>) -> Result<Num, MathError> {
    match ast {
        Ast::Num(n) => Ok(*n),
        Ast::Name(name) => match name.as_str() {
            "x" => x
                .map(Num::Float)
                .ok_or_else(|| MathError::Message("unsupported".into())),
            "True" => Ok(Num::Int(1)),
            "False" => Ok(Num::Int(0)),
            n => FORMULA_NAMES
                .iter()
                .find(|(k, _)| *k == n)
                .map(|(_, v)| Num::Float(*v))
                .ok_or_else(|| MathError::Message("unsupported".into())),
        },
        Ast::Neg(e) => Ok(neg(eval(e, x)?)),
        Ast::Pos(e) => eval(e, x),
        Ast::Bin(op, a, b) => {
            let left = eval(a, x)?;
            let right = eval(b, x)?;
            match *op {
                "+" => Ok(add(left, right)),
                "-" => Ok(sub(left, right)),
                "*" => Ok(mul(left, right)),
                "/" => div(left, right),
                "//" => floordiv(left, right),
                "%" => modulo(left, right),
                _ => pow(left, right),
            }
        }
        Ast::Call(name, args) => {
            let mut values = Vec::with_capacity(args.len());
            for a in args {
                values.push(eval(a, x)?);
            }
            math_func(name, &values)
        }
    }
}

fn unreadable(text: &str) -> MathError {
    MathError::Message(format!("can't read \"{text}\""))
}

/// `calc`'s evaluation: no constants / function calls (the original's ev only knows numbers, binary and unary operations).
fn eval_calc(ast: &Ast) -> Result<Num, MathError> {
    let unsupported = || Err(MathError::Message("unsupported".into()));
    match ast {
        Ast::Num(n) => Ok(*n),
        Ast::Name(name) => match name.as_str() {
            "True" => Ok(Num::Int(1)),
            "False" => Ok(Num::Int(0)),
            _ => unsupported(),
        },
        Ast::Neg(e) => Ok(neg(eval_calc(e)?)),
        Ast::Pos(e) => eval_calc(e),
        Ast::Bin(op, a, b) => {
            let left = eval_calc(a)?;
            let right = eval_calc(b)?;
            match *op {
                "+" => Ok(add(left, right)),
                "-" => Ok(sub(left, right)),
                "*" => Ok(mul(left, right)),
                "/" => div(left, right),
                "//" => floordiv(left, right),
                "%" => modulo(left, right),
                _ => pow(left, right),
            }
        }
        Ast::Call(..) => unsupported(),
    }
}

/// Evaluate a simple expression (`mathexpr.calc`).
pub fn calc(text: &str) -> Result<CalcValue, MathError> {
    let text = text.replace(['x', '×'], "*").replace('^', "**");
    let ast = parse(&text).map_err(|_| unreadable(&text))?;
    match eval_calc(&ast) {
        Ok(v) => Ok(v.value()),
        Err(MathError::DivisionByZero) => Err(MathError::DivisionByZero),
        Err(_) => Err(unreadable(&text)),
    }
}

/// [`calc`] but real-valued only (complex numbers are an error).
pub fn calc_f64(text: &str) -> Result<f64, MathError> {
    calc(text)?
        .as_f64()
        .ok_or_else(|| MathError::Message(format!("can't read \"{text}\"")))
}

/// Evaluate; the result must be an integer (`mathexpr.calc_int`).
pub fn calc_int(text: &str, lo: Option<i64>, hi: Option<i64>) -> Result<i64, MathError> {
    let value = calc(text)?;
    let value = match value {
        CalcValue::Int(i) => i,
        CalcValue::Float(f) => {
            if f.is_nan() {
                return Err(MathError::Message(
                    "cannot convert float NaN to integer".into(),
                ));
            }
            if f.is_infinite() {
                return Err(MathError::Message(
                    "cannot convert float infinity to integer".into(),
                ));
            }
            if f != f.trunc() {
                return Err(MathError::WholeNumber);
            }
            f as i128
        }
        CalcValue::Complex(..) => {
            return Err(MathError::Message("can't convert complex to int".into()));
        }
    };
    if lo.is_some_and(|l| value < l as i128) || hi.is_some_and(|h| value > h as i128) {
        return Err(MathError::OutOfRange);
    }
    i64::try_from(value).map_err(|_| MathError::OutOfRange)
}

// ---------------------------------------------------------------- formulas

/// A compiled formula (the function `mathexpr.formula` returns).
#[derive(Clone, Debug)]
pub struct Formula {
    ast: Ast,
}

impl Formula {
    /// Evaluate at x; a complex result reports "not a real number".
    pub fn eval(&self, x: f64) -> Result<f64, MathError> {
        let v = eval(&self.ast, Some(x))?;
        if is_complex(v) {
            return Err(MathError::Message("not a real number".into()));
        }
        Ok(v.value().as_f64().unwrap_or(f64::NAN))
    }
}

/// The generic message the original `check` gives for unsupported syntax.
const UNSUPPORTED_MSG: &str = "only numbers, x, + - * / ^ and functions like sin( ) work";

fn check(ast: &Ast) -> Result<(), MathError> {
    match ast {
        Ast::Num(_) => Ok(()),
        Ast::Name(name) => {
            if name == "x" || FORMULA_NAMES.iter().any(|(k, _)| k == name) {
                Ok(())
            } else if name == "None" {
                Err(MathError::Message(UNSUPPORTED_MSG.into()))
            } else {
                Err(MathError::Message(format!(
                    "unknown name \"{name}\" (use x for the position)"
                )))
            }
        }
        Ast::Neg(e) | Ast::Pos(e) => check(e),
        Ast::Bin(_, a, b) => {
            check(a)?;
            check(b)
        }
        Ast::Call(name, args) => {
            if !FORMULA_FUNCS.contains(&name.as_str()) {
                return Err(MathError::Message(format!("unknown function \"{name}\"")));
            }
            for a in args {
                check(a)?;
            }
            Ok(())
        }
    }
}

/// Compile a formula in x (`mathexpr.formula`).
pub fn formula(text: &str) -> Result<Formula, MathError> {
    let text = text.trim().replace('×', "*").replace('^', "**");
    if text.is_empty() {
        return Err(MathError::Message("type a formula".into()));
    }
    let ast = parse(&text)
        .map_err(|_| MathError::Message("can't read it (check the brackets and signs)".into()))?;
    check(&ast)?;
    Ok(Formula { ast })
}

/// Number -> short text (`mathexpr.fmt`).
pub fn fmt(x: f64) -> String {
    if (x - spiderweb_core::round_half_even(x)).abs() < 1e-6 {
        return (spiderweb_core::round_half_even(x) as i64).to_string();
    }
    format!("{x:.3}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}
