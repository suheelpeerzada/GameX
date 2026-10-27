//! Lightweight readers for decompiled Java (jadx) and smali (apktool). They
//! only pull out what classification needs: the class shape, its fields and
//! its method names. They never need to understand the full language.

#[derive(Debug, Default, Clone)]
pub struct ClassInfo {
    /// "class", "interface", "enum" or "abstract class".
    pub kind: String,
    pub extends: Option<String>,
    pub implements: Vec<String>,
    pub fields: Vec<String>,
    pub methods: Vec<String>,
}

/// Reads the top-level type declared in a jadx `.java` file.
pub fn parse_java(text: &str) -> ClassInfo {
    let mut info = ClassInfo::default();
    let mut depth = 0i32;
    let mut in_block_comment = false;
    for raw in text.lines() {
        let line = raw.trim();
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
            parse_member(line, &mut info);
        }
        depth += brace_delta(line);
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

fn parse_member(line: &str, info: &mut ClassInfo) {
    if line.is_empty() || line.starts_with('}') {
        return;
    }
    let before_assign = line.split('=').next().unwrap_or(line);
    if let Some(paren) = before_assign.find('(') {
        // A method or constructor: the word right before "(".
        let head = before_assign[..paren].trim_end();
        let name = head.rsplit(|c: char| c.is_whitespace()).next().unwrap_or("");
        let is_control = matches!(name, "if" | "for" | "while" | "switch" | "catch" | "synchronized" | "return");
        if !name.is_empty() && !is_control && (line.ends_with('{') || line.ends_with(';')) {
            info.methods.push(name.to_string());
        }
    } else if line.ends_with(';') {
        // Fields: `int hp;`, `float x, y;`, `Map<K, V> m = ...;`
        let head = without_generics(before_assign.trim_end_matches(';'));
        let mut parts = head.split(',');
        let first = parts.next().unwrap_or("").trim();
        if !first.contains(' ') {
            return;
        }
        let names = first.rsplit(char::is_whitespace).next().into_iter().chain(parts.map(str::trim));
        info.fields.extend(names.filter(|n| is_identifier(n)).map(String::from));
    }
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
