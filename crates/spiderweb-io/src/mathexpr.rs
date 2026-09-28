//! 数字框里的数学表达式（Python `files/mathexpr.py` 的移植）。
//!
//! - [`calc`]：`960*4`、`(60+4)*16` 这类简单算式；只有数字与 `+ - * / // % ** ( )`，
//!   `x` 和 `×` 当乘号、`^` 当 `**`（注意原版先把 `^` 换成 `**`，所以手写 `**` 会被换成
//!   `****` 而报错——这里照做）；支持 Python 的整数 / 浮点 / 复数语义。
//! - [`formula`]：`x^2`、`sin(x*pi/2)` 这类公式 -> x 的函数；名字有 `x`、`pi`、`e`
//!   和 [`FORMULA_FUNCS`]，报错消息与原版一致。
//! - [`calc_int`]、[`fmt`] 同原版。
//!
//! 与原版的差异：整数的任意精度只用 i128（超出后退化成浮点）；负底数的分数次幂返回复数，
//! 复数幂的末位与 CPython 可能有微小差别。

use std::f64::consts::{E, PI};

/// 表达式的值（Python 的 int / float / complex）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CalcValue {
    Int(i128),
    Float(f64),
    Complex(f64, f64),
}

impl CalcValue {
    /// 实数部分；复数返回 None。
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

/// 求值错误；消息与原版的 ValueError / OverflowError 对应。
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

/// 公式里的常量（FORMULA_NAMES）。
pub const FORMULA_NAMES: [(&str, f64); 2] = [("pi", PI), ("e", E)];

/// 公式里能用的函数（FORMULA_FUNCS）。
pub const FORMULA_FUNCS: [&str; 22] = [
    "sin", "cos", "tan", "asin", "acos", "atan", "sinh", "cosh", "tanh", "sqrt", "exp", "log",
    "ln", "log10", "log2", "abs", "min", "max", "floor", "ceil", "round", "pow",
];

// ---------------------------------------------------------------- 值

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
    // Python 的复数除法：按绝对值大的分量缩放。
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

/// CPython `float_divmod`（floatobject.c）：先 fmod 再修正符号，商向最近的整数吸附。
/// 直接 `(x / y).floor()` 在 x/y 落在整数边界附近时会差 1，这里的写法与原版一致。
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

/// `(a+bi)^(c+di)`，用 exp(w·ln z) 的常用算法（末位与原版可能有差）。
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
                // 超出 i128 的整数：原版保持精确整数，这里退化成浮点（见模块说明）
                return pow_float(x as f64, y as f64);
            }
            (Num::Int(x), Num::Int(y)) => {
                // 负指数 -> 浮点；负底数的整数次幂也走浮点。
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
            // 溢出（输入有限而结果无穷）原版抛 OverflowError("math range error")
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

// ---------------------------------------------------------------- 词法 / 语法

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

/// Python `ast.parse(mode="eval")` 对首行缩进的 IndentationError；`calc` 不 strip 输入。
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

// ---------------------------------------------------------------- 求值

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

/// `calc` 的求值：没有常量 / 函数调用（原版 ev 只认数字、二元与一元运算）。
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

/// 求值简单算式（`mathexpr.calc`）。
pub fn calc(text: &str) -> Result<CalcValue, MathError> {
    let text = text.replace(['x', '×'], "*").replace('^', "**");
    let ast = parse(&text).map_err(|_| unreadable(&text))?;
    match eval_calc(&ast) {
        Ok(v) => Ok(v.value()),
        Err(MathError::DivisionByZero) => Err(MathError::DivisionByZero),
        Err(_) => Err(unreadable(&text)),
    }
}

/// [`calc`] 只要实数（复数当错误）。
pub fn calc_f64(text: &str) -> Result<f64, MathError> {
    calc(text)?
        .as_f64()
        .ok_or_else(|| MathError::Message(format!("can't read \"{text}\"")))
}

/// 求值必须是整数（`mathexpr.calc_int`）。
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

// ---------------------------------------------------------------- 公式

/// 编译好的公式（`mathexpr.formula` 返回的函数）。
#[derive(Clone, Debug)]
pub struct Formula {
    ast: Ast,
}

impl Formula {
    /// 代入 x 求值；复数结果报 "not a real number"。
    pub fn eval(&self, x: f64) -> Result<f64, MathError> {
        let v = eval(&self.ast, Some(x))?;
        if is_complex(v) {
            return Err(MathError::Message("not a real number".into()));
        }
        Ok(v.value().as_f64().unwrap_or(f64::NAN))
    }
}

/// 原版 `check` 对不支持的写法给的通用消息。
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

/// 编译 x 的公式（`mathexpr.formula`）。
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

/// 数字 -> 短文本（`mathexpr.fmt`）。
pub fn fmt(x: f64) -> String {
    if (x - spiderweb_core::round_half_even(x)).abs() < 1e-6 {
        return (spiderweb_core::round_half_even(x) as i64).to_string();
    }
    format!("{x:.3}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}
