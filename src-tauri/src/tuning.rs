//! Gameplay values (a player's speed, an enemy's damage) changed in a game's
//! smali, where Android games are rebuilt from. The analyzer finds each value
//! in the decompiled Java (read-only); here the same field's starting value
//! is found in the class's smali and its number is swapped.
//!
//! A field's starting value is one of:
//! - its declaration: `.field private static final SPEED:F = 5.0f`
//! - a constant stored into it when the object is made:
//!   `const/high16 v0, 0x40a00000` ... `iput v0, p0, Lcom/a/Hero;->speed:F`

use std::io;

fn invalid(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, msg.into())
}

/// Where a field's starting value is written in a smali file.
#[derive(Debug, PartialEq)]
enum Site {
    /// The `.field` line, which ends `= literal`.
    Declared { line: usize },
    /// A `const` line loading the register stored into the field.
    Constant { line: usize, register: String },
}

/// The field's type letter (`F`, `I`, `Z`, `D`, `J`...) and where its value is set.
fn find(text: &str, field: &str) -> Option<(char, Site)> {
    let lines: Vec<&str> = text.lines().collect();
    let target = format!("->{field}:");
    // Declared with a value.
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim();
        if !t.starts_with(".field") {
            continue;
        }
        let Some((decl, _)) = t.split_once(" = ") else { continue };
        let name_type = decl.split_whitespace().last()?;
        if let Some((name, ty)) = name_type.split_once(':') {
            if name == field {
                return Some((ty.chars().next()?, Site::Declared { line: i }));
            }
        }
    }
    // Stored by a constructor (or the class's static setup).
    let mut in_setup = false;
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim();
        if t.starts_with(".method") {
            in_setup = t.contains("<init>") || t.contains("<clinit>");
            continue;
        }
        if !in_setup || !(t.starts_with("iput") || t.starts_with("sput")) || !t.contains(&target) {
            continue;
        }
        let ty = t.rsplit(':').next()?.chars().next()?;
        let register = operands(t).first()?.to_string();
        // The nearest line before it that sets that register must be a constant.
        for j in (0..i).rev() {
            let u = lines[j].trim();
            if u.starts_with(".method") {
                break;
            }
            if u.is_empty() || u.starts_with('.') || u.starts_with('#') || u.starts_with(':') {
                continue;
            }
            let ops = operands(u);
            if ops.first().map(String::as_str) != Some(register.as_str()) || ["iput", "sput", "aput", "if-", "return"].iter().any(|op| u.starts_with(op)) {
                continue;
            }
            return u.starts_with("const").then(|| (ty, Site::Constant { line: j, register: register.clone() }));
        }
        return None;
    }
    None
}

/// An instruction's operands: `iput v0, p0, La;->x:F` -> ["v0", "p0", "La;->x:F"].
fn operands(line: &str) -> Vec<String> {
    let rest = line.split_once(char::is_whitespace).map(|(_, r)| r).unwrap_or("");
    let rest = rest.split('#').next().unwrap_or("");
    rest.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

fn wide(ty: char) -> bool {
    matches!(ty, 'D' | 'J')
}

/// A smali literal as the value people see: `0x40a00000` for a float field -> "5".
fn decode(literal: &str, ty: char) -> Option<String> {
    let s = literal.trim().trim_end_matches(['L', 't', 's']);
    // Declarations are written as values already: 5.0f, 2.5, true, 0x10.
    if ty == 'Z' {
        return Some(match s {
            "true" | "0x1" | "1" => "true",
            _ => "false",
        }
        .into());
    }
    if let Some(v) = s.strip_suffix('f').filter(|v| v.contains('.') || v.contains('E')).and_then(|v| v.parse::<f32>().ok()) {
        return Some(trim(v as f64));
    }
    if !s.contains("0x") {
        return s.parse::<f64>().ok().map(trim);
    }
    let negative = s.starts_with('-');
    let bits = i128::from_str_radix(s.trim_start_matches('-').trim_start_matches("0x"), 16).ok()?;
    let bits = if negative { -bits } else { bits };
    Some(match ty {
        'F' => trim(f32::from_bits(bits as u32) as f64),
        'D' => trim(f64::from_bits(bits as u64)),
        _ => bits.to_string(),
    })
}

fn trim(v: f64) -> String {
    let s = format!("{v}");
    s.strip_suffix(".0").map(str::to_string).unwrap_or(s)
}

/// The literal for `value` in a field of type `ty`, as smali writes it in a `const`.
fn encode(value: &str, ty: char) -> io::Result<String> {
    let v = value.trim();
    let bad = || invalid(format!("\"{v}\" isn't a number this value can hold."));
    Ok(match ty {
        'F' => format!("{:#x}", v.parse::<f32>().map_err(|_| bad())?.to_bits()),
        'D' => format!("{:#x}L", v.parse::<f64>().map_err(|_| bad())?.to_bits()),
        'J' => hex(v.parse::<i64>().map_err(|_| bad())? as i128) + "L",
        'Z' => match v {
            "true" | "1" => "0x1".into(),
            "false" | "0" => "0x0".into(),
            _ => return Err(bad()),
        },
        'I' | 'S' | 'B' | 'C' => hex(v.parse::<i32>().map_err(|_| bad())? as i128),
        _ => return Err(invalid("GameX can't change this kind of value yet.")),
    })
}

fn hex(v: i128) -> String {
    if v < 0 {
        format!("-{:#x}", -v)
    } else {
        format!("{v:#x}")
    }
}

/// What a declaration's literal looks like for `value`: 7.5f, 2.5, 3, true.
fn declared(value: &str, ty: char) -> io::Result<String> {
    let v = value.trim();
    let bad = || invalid(format!("\"{v}\" isn't a number this value can hold."));
    Ok(match ty {
        'F' => format!("{:?}f", v.parse::<f32>().map_err(|_| bad())?),
        'D' => format!("{:?}", v.parse::<f64>().map_err(|_| bad())?),
        'Z' => (encode(v, ty)? == "0x1").to_string(),
        'J' => hex(v.parse::<i64>().map_err(|_| bad())? as i128) + "L",
        _ => encode(v, ty)?,
    })
}

/// The field's starting value as written in the smali, if GameX can find it.
pub fn value_of(text: &str, field: &str) -> Option<String> {
    let (ty, site) = find(text, field)?;
    let lines: Vec<&str> = text.lines().collect();
    match site {
        Site::Declared { line } => decode(lines[line].split_once(" = ")?.1, ty),
        Site::Constant { line, .. } => decode(operands(lines[line].trim()).get(1)?, ty),
    }
}

/// The smali with the field's starting value changed to `value`.
pub fn set_value(text: &str, field: &str, value: &str) -> io::Result<String> {
    let (ty, site) = find(text, field).ok_or_else(|| invalid(format!("GameX couldn't find where {field} gets its value in the code.")))?;
    let ending = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    match site {
        Site::Declared { line } => {
            let (decl, _) = lines[line].split_once(" = ").map(|(a, b)| (a.to_string(), b.to_string())).unwrap();
            lines[line] = format!("{decl} = {}", declared(value, ty)?);
        }
        Site::Constant { line, register } => {
            let indent: String = lines[line].chars().take_while(|c| c.is_whitespace()).collect();
            let op = if wide(ty) { "const-wide" } else { "const" };
            lines[line] = format!("{indent}{op} {register}, {}    # {}", encode(value, ty)?, value.trim());
        }
    }
    let mut out = lines.join(ending);
    if text.ends_with('\n') {
        out.push_str(ending);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HERO: &str = ".class public Lcom/a/Hero;
.super Ljava/lang/Object;

.field public static final MAX_HEALTH:I = 0x64
.field private jumpPower:D
.field private speed:F
.field public lives:I
.field public flying:Z

.method public constructor <init>()V
    .locals 2

    invoke-direct {p0}, Ljava/lang/Object;-><init>()V

    const/high16 v0, 0x40a00000    # 5.0f

    iput v0, p0, Lcom/a/Hero;->speed:F

    const/4 v0, 0x3

    iput v0, p0, Lcom/a/Hero;->lives:I

    const-wide/high16 v0, 0x4014000000000000L    # 5.0

    iput-wide v0, p0, Lcom/a/Hero;->jumpPower:D

    invoke-static {}, Lcom/a/Rules;->canFly()Z

    move-result v0

    iput-boolean v0, p0, Lcom/a/Hero;->flying:Z

    return-void
.end method
";

    #[test]
    fn reads_starting_values() {
        assert_eq!(value_of(HERO, "speed").as_deref(), Some("5"));
        assert_eq!(value_of(HERO, "lives").as_deref(), Some("3"));
        assert_eq!(value_of(HERO, "jumpPower").as_deref(), Some("5"));
        assert_eq!(value_of(HERO, "MAX_HEALTH").as_deref(), Some("100"));
        // Worked out while the game runs, not a number in the code.
        assert_eq!(value_of(HERO, "flying"), None);
        assert_eq!(value_of(HERO, "missing"), None);
    }

    #[test]
    fn changes_starting_values() {
        let text = set_value(HERO, "speed", "7.5").unwrap();
        assert!(text.contains("    const v0, 0x40f00000    # 7.5\n"), "{text}");
        assert_eq!(value_of(&text, "speed").as_deref(), Some("7.5"));
        let text = set_value(&text, "lives", "-2").unwrap();
        assert_eq!(value_of(&text, "lives").as_deref(), Some("-2"));
        let text = set_value(&text, "jumpPower", "12.25").unwrap();
        assert!(text.contains("const-wide v0, 0x4028800000000000L"));
        assert_eq!(value_of(&text, "jumpPower").as_deref(), Some("12.25"));
        let text = set_value(&text, "MAX_HEALTH", "250").unwrap();
        assert!(text.contains(".field public static final MAX_HEALTH:I = 0xfa\n"));
        // Everything else is left as it was.
        assert_eq!(text.lines().count(), HERO.lines().count());
        assert!(set_value(HERO, "speed", "fast").is_err());
        assert!(set_value(HERO, "flying", "true").is_err());
    }
}
