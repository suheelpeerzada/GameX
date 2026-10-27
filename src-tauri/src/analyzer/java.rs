//! Lightweight readers for decompiled Java (jadx) and smali (apktool). They
//! only pull out what classification needs: the class shape, its fields, its
//! method names and literal field values. They never need to understand the
//! full language.

#[derive(Debug, Default, Clone)]
pub struct ClassInfo {
    /// The declared name, e.g. "Hero" (Java only).
    pub name: String,
    /// "class", "interface", "enum" or "abstract class".
    pub kind: String,
    pub extends: Option<String>,
    pub implements: Vec<String>,
    pub fields: Vec<String>,
    /// Fields declared as two-dimensional arrays (`int[][] tiles`).
    pub grid_fields: Vec<String>,
    pub methods: Vec<String>,
    /// Literal values given to fields, with where they are set (Java only).
    pub values: Vec<FieldValue>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldValue {
    pub field: String,
    /// The literal as plain text: `4.5`, `100`, `true`.
    pub value: String,
    /// 1-based line in the Java file.
    pub line: usize,
    /// Set by an assignment in a constructor rather than a field initializer.
    pub in_constructor: bool,
}

/// Reads the top-level type declared in a jadx `.java` file.
pub fn parse_java(text: &str) -> ClassInfo {
    let mut info = ClassInfo::default();
    let mut depth = 0i32;
    let mut in_block_comment = false;
    // The method whose body is being read, if any.
    let mut method: Option<String> = None;
    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim();
        let line_no = index + 1;
        if in_block_comment {
            in_block_comment = !line.contains("*/");
            continue;
        }
        if line.starts_with("/*") && !line.contains("*/") {
            in_block_comment = true;
            continue;
        }
        if line.starts_with("//") || line.starts_with('@') || line.starts_with('*') {
            continue;
        }
        if depth == 0 && info.kind.is_empty() {
            parse_declaration(line, &mut info);
        } else if depth == 1 {
            if let Some(name) = parse_member(line, line_no, &mut info) {
                method = Some(name);
            }
        } else if depth >= 2 && method.as_deref() == Some(info.name.as_str()) {
            parse_constructor_assignment(line, line_no, &mut info);
        }
        depth += brace_delta(line);
        if depth <= 1 {
            method = None;
        }
    }
    info
}

fn parse_declaration(line: &str, info: &mut ClassInfo) {
    let words: Vec<&str> = line
        .split(|c: char| c.is_whitespace() || c == '{' || c == ',')
        .filter(|w| !w.is_empty())
        .collect();
    let Some(pos) = words
        .iter()
        .position(|w| matches!(*w, "class" | "interface" | "enum" | "record"))
    else {
        return;
    };
    let abstract_ = words[..pos].contains(&"abstract");
    info.name = words.get(pos + 1).map(|w| strip_generics(w)).unwrap_or_default();
    info.kind = if abstract_ && words[pos] == "class" {
        "abstract class".into()
    } else {
        words[pos].into()
    };
    let mut mode = "";
    for w in &words[pos + 1..] {
        match *w {
            "extends" | "implements" => mode = w,
            _ if mode == "extends" && info.extends.is_none() && info.kind != "interface" => {
                info.extends = Some(strip_generics(w))
            }
            _ if mode == "implements" || (mode == "extends" && info.kind == "interface") => {
                info.implements.push(strip_generics(w))
            }
            _ => {}
        }
    }
}

/// Reads one member line. Returns the method name when the line opens a
/// method body, so the caller knows which method it is inside.
fn parse_member(line: &str, line_no: usize, info: &mut ClassInfo) -> Option<String> {
    if line.is_empty() || line.starts_with('}') {
        return None;
    }
    let (before_assign, initializer) = match line.split_once('=') {
        Some((before, after)) => (before, Some(after)),
        None => (line, None),
    };
    if let Some(paren) = before_assign.find('(') {
        // A method or constructor: the word right before "(".
        let head = before_assign[..paren].trim_end();
        let name = head.rsplit(|c: char| c.is_whitespace()).next().unwrap_or("");
        let is_control = matches!(name, "if" | "for" | "while" | "switch" | "catch" | "synchronized" | "return");
        if !name.is_empty() && !is_control && (line.ends_with('{') || line.ends_with(';')) {
            info.methods.push(name.to_string());
            if line.ends_with('{') {
                return Some(name.to_string());
            }
        }
    } else if line.ends_with(';') {
        // Fields: `int hp;`, `float x, y;`, `Map<K, V> m = ...;`
        let head = without_generics(before_assign.trim_end_matches(';'));
        let mut parts = head.split(',').peekable();
        let first = parts.next().unwrap_or("").trim();
        if !first.contains(' ') {
            return None;
        }
        let single = parts.peek().is_none();
        let mut words = first.rsplit(char::is_whitespace);
        let name = words.next().unwrap_or("");
        let ty = words.next().unwrap_or("");
        let names = std::iter::once(name).chain(parts.map(str::trim));
        info.fields.extend(names.filter(|n| is_identifier(n)).map(String::from));
        if is_identifier(name) && (ty.contains("[][]") || name.ends_with("[][]")) {
            info.grid_fields.push(name.to_string());
        }
        // `float speed = 4.5f;` (only one name, and only plain literals)
        if let (true, Some(init)) = (single && is_identifier(name), initializer) {
            if let Some(value) = parse_literal(init.trim().trim_end_matches(';')) {
                info.values.push(FieldValue {
                    field: name.to_string(),
                    value,
                    line: line_no,
                    in_constructor: false,
                });
            }
        }
    }
    None
}

/// `this.speed = 4.5f;` or `speed = 4.5f;` inside a constructor.
fn parse_constructor_assignment(line: &str, line_no: usize, info: &mut ClassInfo) {
    let Some((lhs, rhs)) = line.strip_suffix(';').and_then(|l| l.split_once('=')) else { return };
    // Not `==`, `+=`, `<=`...
    if rhs.starts_with('=') || lhs.ends_with(['+', '-', '*', '/', '!', '<', '>', '&', '|', '%', '^']) {
        return;
    }
    let field = lhs.trim().trim_start_matches("this.");
    if !info.fields.iter().any(|f| f == field) {
        return;
    }
    if let Some(value) = parse_literal(rhs.trim()) {
        info.values.push(FieldValue {
            field: field.to_string(),
            value,
            line: line_no,
            in_constructor: true,
        });
    }
}

/// A plain number or boolean literal, without its type suffix or a cast:
/// `4.5f` -> "4.5", `(byte) 3` -> "3", `0x14` -> "20", `true` -> "true".
pub fn parse_literal(text: &str) -> Option<String> {
    let mut t = text.trim();
    if t.starts_with('(') {
        t = t.split_once(')')?.1.trim();
    }
    if t == "true" || t == "false" {
        return Some(t.to_string());
    }
    let (negative, digits) = match t.strip_prefix('-') {
        Some(rest) => (true, rest.trim()),
        None => (false, t),
    };
    let sign = if negative { "-" } else { "" };
    if let Some(hex) = digits.strip_prefix("0x").or_else(|| digits.strip_prefix("0X")) {
        let hex = hex.trim_end_matches(['L', 'l']);
        return i64::from_str_radix(hex, 16).ok().map(|v| format!("{sign}{v}"));
    }
    let number = digits.trim_end_matches(['f', 'F', 'd', 'D', 'L', 'l']);
    let valid = !number.is_empty()
        && number.chars().all(|c| c.is_ascii_digit() || c == '.' || c == 'E' || c == 'e' || c == '-')
        && number.starts_with(|c: char| c.is_ascii_digit())
        && number.parse::<f64>().is_ok();
    valid.then(|| format!("{sign}{number}"))
}

fn without_generics(s: &str) -> String {
    let mut depth = 0;
    s.chars()
        .filter(|&c| {
            match c {
                '<' => depth += 1,
                '>' => depth -= 1,
                _ => return depth == 0,
            }
            false
        })
        .collect()
}

/// Net change in `{`/`}` nesting, ignoring braces inside strings and chars.
fn brace_delta(line: &str) -> i32 {
    let mut delta = 0;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for c in line.chars() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '{' => delta += 1,
            '}' => delta -= 1,
            _ => {}
        }
    }
    delta
}

/// Reads one apktool `.smali` file.
pub fn parse_smali(text: &str) -> ClassInfo {
    let mut info = ClassInfo {
        kind: "class".into(),
        ..Default::default()
    };
    for line in text.lines().map(str::trim) {
        let words: Vec<&str> = line.split_whitespace().collect();
        let Some(&directive) = words.first() else { continue };
        let last = words.last().copied().unwrap_or("");
        match directive {
            ".class" => {
                if words.contains(&"interface") {
                    info.kind = "interface".into();
                } else if words.contains(&"enum") {
                    info.kind = "enum".into();
                } else if words.contains(&"abstract") {
                    info.kind = "abstract class".into();
                }
            }
            ".super" => {
                let name = smali_type(last);
                if name != "java.lang.Object" && name != "java.lang.Enum" {
                    info.extends = Some(name);
                }
            }
            ".implements" => info.implements.push(smali_type(last)),
            ".field" => {
                if let Some((name, _)) = last.split_once(':') {
                    info.fields.push(name.to_string());
                }
            }
            ".method" => {
                if let Some((name, _)) = last.split_once('(') {
                    if !name.starts_with('<') {
                        info.methods.push(name.to_string());
                    }
                }
            }
            _ => {}
        }
    }
    info
}

/// `Lcom/x/Foo;` -> `com.x.Foo`
fn smali_type(t: &str) -> String {
    t.trim_start_matches('L').trim_end_matches(';').replace('/', ".")
}

fn strip_generics(t: &str) -> String {
    t.split('<').next().unwrap_or(t).to_string()
}

fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

/// Last segment of a dotted name: `com.x.Foo` -> `Foo`.
pub fn simple_name(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_jadx_output() {
        let src = r#"package com.example.game;

import android.graphics.Canvas;

/* loaded from: classes.dex */
public class Hero extends Actor implements Runnable, Drawable<Canvas> {
    private static final String TAG = "a { b";
    public int hp;
    float x, y;
    private final List<Item> bag = new ArrayList();

    public Hero(int hp) {
        this.hp = hp;
        if (hp > 0) {
            move(1);
        }
    }

    public void move(float dx) {
        this.x += dx;
    }

    abstract void jump();
}
"#;
        let info = parse_java(src);
        assert_eq!(info.kind, "class");
        assert_eq!(info.extends.as_deref(), Some("Actor"));
        assert_eq!(info.implements, ["Runnable", "Drawable"]);
        assert_eq!(info.fields, ["TAG", "hp", "x", "y", "bag"]);
        assert_eq!(info.methods, ["Hero", "move", "jump"]);
        assert_eq!(info.name, "Hero");
    }

    #[test]
    fn reads_literal_values_with_lines() {
        let src = "public class Hero extends Actor {\n\
                   private static final int MAX_HP = 100;\n\
                   float speed = 4.5f;\n\
                   int[][] tiles = new int[8][8];\n\
                   boolean flying = false;\n\
                   int a = 1, b = 2;\n\
                   String name = \"x\";\n\
                   double jump;\n\
                   public Hero() {\n\
                   this.jump = -2.5d;\n\
                   speed += 1;\n\
                   if (speed == 3) {\n\
                   x = 5;\n\
                   }\n\
                   }\n\
                   void other() {\n\
                   this.jump = 9;\n\
                   }\n\
                   }\n";
        let info = parse_java(src);
        let values: Vec<_> = info.values.iter().map(|v| (v.field.as_str(), v.value.as_str(), v.line, v.in_constructor)).collect();
        assert_eq!(
            values,
            [
                ("MAX_HP", "100", 2, false),
                ("speed", "4.5", 3, false),
                ("flying", "false", 5, false),
                ("jump", "-2.5", 10, true),
            ]
        );
        assert_eq!(info.grid_fields, ["tiles"]);
    }

    #[test]
    fn parses_literals() {
        assert_eq!(parse_literal("4.5f").as_deref(), Some("4.5"));
        assert_eq!(parse_literal("(byte) 3").as_deref(), Some("3"));
        assert_eq!(parse_literal("0x14").as_deref(), Some("20"));
        assert_eq!(parse_literal("-7").as_deref(), Some("-7"));
        assert_eq!(parse_literal("1.5e3").as_deref(), Some("1.5e3"));
        assert_eq!(parse_literal("speed * 2"), None);
        assert_eq!(parse_literal("new int[3]"), None);
        assert_eq!(parse_literal("\"x\""), None);
    }

    #[test]
    fn reads_smali() {
        let src = ".class public abstract Lcom/x/Enemy;\n.super Lcom/x/Actor;\n.implements Ljava/lang/Runnable;\n\
                   .field public health:I\n.method public constructor <init>()V\n.end method\n\
                   .method public update(F)V\n.end method\n";
        let info = parse_smali(src);
        assert_eq!(info.kind, "abstract class");
        assert_eq!(info.extends.as_deref(), Some("com.x.Actor"));
        assert_eq!(info.implements, ["java.lang.Runnable"]);
        assert_eq!(info.fields, ["health"]);
        assert_eq!(info.methods, ["update"]);
    }
}
