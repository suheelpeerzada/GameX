//! "Keep this change": turning a live field change into a smali edit, so the
//! value is set every time the game makes the object (or loads the class,
//! for a static field). The value is assigned at the end of each
//! constructor, after whatever the game sets there itself, inside a marked
//! block that keeping the same field again replaces.

use std::path::Path;

use serde::Serialize;

use crate::edits::EditStore;
use crate::jdwp::Value;
use crate::live::parse_input;

/// A field to keep a value for, as the live inspector knows it.
pub struct KeepField<'a> {
    /// The object's own class (`com.g.Hero`).
    pub class: &'a str,
    /// The class declaring the field (`com.g.Actor`).
    pub owner: &'a str,
    pub name: &'a str,
    /// JVM signature (`I`, `Ljava/lang/String;`...).
    pub signature: &'a str,
    pub is_static: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeepResult {
    /// The edited smali file, relative to `unpacked/`.
    pub file: String,
    /// The class whose objects now start with the value.
    pub class: String,
    /// Constructors (or static initializers) that set it.
    pub places: usize,
}

const BEGIN: &str = "# GameX keep: ";
const END: &str = "# GameX keep end";

fn descriptor(class: &str) -> String {
    format!("L{};", class.replace('.', "/"))
}

/// `apktool/smali*/com/g/Hero.smali`, relative to `unpacked/`, if it exists.
pub fn smali_file(unpacked: &Path, class: &str) -> Option<String> {
    let rel = class.replace('.', "/");
    let mut dirs: Vec<String> = std::fs::read_dir(unpacked.join("apktool"))
        .ok()?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("smali"))
        .collect();
    dirs.sort();
    dirs.into_iter()
        .map(|d| format!("apktool/{d}/{rel}.smali"))
        .find(|p| unpacked.join(p).is_file())
}

/// The modifiers on a field's `.field` line (`private`, `final`...), if declared here.
pub fn field_modifiers(smali: &str, name: &str, signature: &str) -> Option<Vec<String>> {
    let decl = format!("{name}:{signature}");
    smali.lines().map(str::trim).find_map(|line| {
        let rest = line.strip_prefix(".field ")?;
        let words: Vec<&str> = rest.split_whitespace().collect();
        let at = words.iter().position(|w| *w == decl)?;
        let mut mods: Vec<String> = words[..at].iter().map(|w| w.to_string()).collect();
        if words.get(at + 1) == Some(&"=") {
            mods.push("=".into());
        }
        Some(mods)
    })
}

/// Which class's constructors should set the field: the object's own class,
/// unless only the declaring class may write it (private or final).
pub fn target_class<'a>(field: &KeepField<'a>, owner_mods: Option<&[String]>) -> &'a str {
    let restricted = owner_mods.is_some_and(|m| m.iter().any(|w| w == "private" || w == "final"));
    if field.is_static || restricted { field.owner } else { field.class }
}

fn hex(n: i64) -> String {
    if n < 0 { format!("-0x{:x}", -(n as i128)) } else { format!("0x{n:x}") }
}

fn smali_string(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ' '..='~' => out.push(c),
            c => {
                let mut units = [0u16; 2];
                for u in c.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{u:04x}"));
                }
            }
        }
    }
    out.push('"');
    out
}

/// The instruction putting `text` (typed or shown) into v0 (v0-v1 for wide values).
pub fn load_value(signature: &str, text: &str) -> Result<String, String> {
    if signature == "Ljava/lang/String;" {
        let t = text.trim();
        if t == "null" {
            return Ok("const/4 v0, 0x0".into());
        }
        let t = t.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(text);
        return Ok(format!("const-string v0, {}", smali_string(t)));
    }
    if signature.len() != 1 {
        return Err("Only numbers, true/false, characters and text can be kept.".into());
    }
    Ok(match (signature, parse_input(signature, text)?) {
        ("J", Value::Int(n)) => format!("const-wide v0, {}L", hex(n)),
        (_, Value::Int(n)) => format!("const v0, {}", hex(n)),
        ("F", Value::Float(f)) => format!("const v0, {}    # {f}f", hex((f as f32).to_bits() as i32 as i64)),
        ("D", Value::Float(f)) => format!("const-wide v0, {}L    # {f}", hex(f.to_bits() as i64)),
        (_, Value::Bool(b)) => format!("const/4 v0, {}", if b { "0x1" } else { "0x0" }),
        (_, Value::Char(c)) => format!("const v0, {}", hex(c as i64)),
        _ => return Err("This value can't be kept.".into()),
    })
}

fn op_suffix(signature: &str) -> &'static str {
    match signature {
        "I" | "F" => "",
        "J" | "D" => "-wide",
        "Z" => "-boolean",
        "B" => "-byte",
        "S" => "-short",
        "C" => "-char",
        _ => "-object",
    }
}

/// Registers taken by a method's parameters (`this` included unless static).
fn param_registers(method_line: &str, is_static: bool) -> usize {
    let params = method_line.split_once('(').and_then(|(_, r)| r.split_once(')')).map(|(p, _)| p).unwrap_or("");
    let mut n = usize::from(!is_static);
    let mut chars = params.chars();
    while let Some(c) = chars.next() {
        match c {
            'J' | 'D' => n += 2,
            'L' => {
                for c in chars.by_ref() {
                    if c == ';' {
                        break;
                    }
                }
                n += 1;
            }
            '[' => {
                // The array's element type follows; the array is one register.
                let mut rest = chars.clone();
                while rest.clone().next() == Some('[') {
                    rest.next();
                }
                if rest.next() == Some('L') {
                    for c in rest.by_ref() {
                        if c == ';' {
                            break;
                        }
                    }
                }
                chars = rest;
                n += 1;
            }
            _ => n += 1,
        }
    }
    n
}

/// Takes out earlier kept blocks for `field_ref`.
fn remove_blocks(lines: &mut Vec<String>, field_ref: &str) {
    let begin = format!("{BEGIN}{field_ref}");
    let mut out = Vec::with_capacity(lines.len());
    let mut skipping = false;
    for line in lines.drain(..) {
        let t = line.trim();
        if t == begin {
            skipping = true;
        } else if skipping && t == END {
            skipping = false;
        } else if !skipping {
            out.push(line);
        }
    }
    *lines = out;
}

/// Sets the field to `text` at the end of every constructor of `target`
/// (or its static initializer), in `smali`, the target class's smali file.
/// Returns the new text and how many constructors set it.
pub fn patch(smali: &str, target: &str, field: &KeepField, text: &str) -> Result<(String, usize), String> {
    let field_ref = format!("{}->{}:{}", descriptor(field.owner), field.name, field.signature);
    let load = load_value(field.signature, text)?;
    let wide = matches!(field.signature, "J" | "D");
    let value_regs = if wide { 2 } else { 1 };
    let ctor = if field.is_static { " constructor <clinit>()V" } else { " constructor <init>(" };

    let mut lines: Vec<String> = smali.lines().map(String::from).collect();
    remove_blocks(&mut lines, &field_ref);

    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 8);
    let mut places = 0;
    let mut i = 0;
    while i < lines.len() {
        let line = &lines[i];
        if !(line.trim_start().starts_with(".method ") && line.contains(ctor)) {
            out.push(line.clone());
            i += 1;
            continue;
        }
        // One constructor: from `.method` to `.end method`.
        let end = (i..lines.len()).find(|&j| lines[j].trim() == ".end method").ok_or("Unfinished method in the smali file.")?;
        let mut method: Vec<String> = lines[i..=end].to_vec();
        let params = param_registers(&method[0], field.is_static);
        let size_at = method.iter().position(|l| {
            let t = l.trim_start();
            t.starts_with(".locals ") || t.starts_with(".registers ")
        });
        let Some(size_at) = size_at else {
            out.extend(method);
            i = end + 1;
            continue;
        };
        let (directive, count) = method[size_at].trim().split_once(' ').ok_or("Bad register count.")?;
        let count: usize = count.trim().parse().map_err(|_| "Bad register count.")?;
        let locals = if directive == ".registers" { count.saturating_sub(params) } else { count };

        // v0 (v0-v1) holds the value; `this` must be one of v0-v15 for iput,
        // so a far-off p0 is first copied next to the value.
        let mut needed = value_regs;
        if !field.is_static && needed.max(locals) > 15 {
            needed += 1;
        }
        let new_locals = locals.max(needed);
        if new_locals > locals && new_locals + params > 16 && locals + params <= 16 {
            return Err(format!(
                "A constructor of {} takes too many values to be changed safely here. Edit its smali by hand instead (Open source).",
                target
            ));
        }
        if new_locals > locals {
            let indent: String = method[size_at].chars().take_while(|c| c.is_whitespace()).collect();
            let shown = if directive == ".registers" { new_locals + params } else { new_locals };
            method[size_at] = format!("{indent}{directive} {shown}");
        }
        let object = if field.is_static {
            None
        } else if new_locals > 15 {
            Some(format!("v{value_regs}"))
        } else {
            Some("p0".to_string())
        };
        let mut block = vec![format!("    {BEGIN}{field_ref}"), format!("    {load}")];
        match &object {
            None => block.push(format!("    sput{} v0, {field_ref}", op_suffix(field.signature))),
            Some(reg) => {
                if reg != "p0" {
                    block.push(format!("    move-object/from16 {reg}, p0"));
                }
                block.push(format!("    iput{} v0, {reg}, {field_ref}", op_suffix(field.signature)));
            }
        }
        block.push(format!("    {END}"));

        for l in method {
            if l.trim() == "return-void" {
                out.extend(block.iter().cloned());
                places += 1;
            }
            out.push(l);
        }
        i = end + 1;
    }

    if places == 0 && field.is_static && !lines.iter().any(|l| l.contains(ctor)) {
        // No static initializer yet: add one.
        let mut block = vec![
            String::new(),
            "# direct methods".into(),
            ".method static constructor <clinit>()V".into(),
            format!("    .locals {value_regs}"),
            String::new(),
            format!("    {BEGIN}{field_ref}"),
            format!("    {load}"),
            format!("    sput{} v0, {field_ref}", op_suffix(field.signature)),
            format!("    {END}"),
            "    return-void".into(),
            ".end method".into(),
        ];
        out.append(&mut block);
        places = 1;
    }
    if places == 0 {
        return Err(format!("{} has no constructor GameX can add the value to.", target));
    }
    let mut text = out.join("\n");
    if smali.ends_with('\n') {
        text.push('\n');
    }
    Ok((text, places))
}

/// Keeps `value` for the field in the project at `project_dir`, as an
/// undoable edit of the right class's smali.
pub fn keep(project_dir: &Path, store: &EditStore, field: &KeepField, value: &str) -> Result<KeepResult, String> {
    let unpacked = project_dir.join("unpacked");
    let read = |rel: &str| -> Result<String, String> {
        let path = store.current(rel).map_err(|e| e.to_string())?;
        std::fs::read_to_string(path).map_err(|e| e.to_string())
    };
    let owner_mods = match smali_file(&unpacked, field.owner) {
        Some(rel) => field_modifiers(&read(&rel)?, field.name, field.signature),
        None => None,
    };
    if owner_mods.as_ref().is_some_and(|m| m.iter().any(|w| w == "final") && m.iter().any(|w| w == "=")) {
        return Err(format!(
            "{} is a constant: the game's code has its value copied in wherever it's used, so it can't be kept from here. Search the code for its value instead.",
            field.name
        ));
    }
    let target = target_class(field, owner_mods.as_deref());
    let rel = smali_file(&unpacked, target)
        .ok_or_else(|| format!("{target} isn't one of the game's own classes, so it can't be changed."))?;
    let (text, places) = patch(&read(&rel)?, target, field, value)?;
    let simple = target.rsplit('.').next().unwrap_or(target);
    store
        .save_text_as(&rel, &text, format!("Keep {simple}.{} = {}", field.name, value.trim()))
        .map_err(|e| e.to_string())?;
    Ok(KeepResult { file: rel, class: target.to_string(), places })
}

#[cfg(test)]
mod tests {
    use super::*;

    const HERO: &str = ".class public Lcom/g/Hero;
.super Lcom/g/Actor;

.field private speed:F

# direct methods
.method public constructor <init>()V
    .locals 0

    invoke-direct {p0}, Lcom/g/Actor;-><init>()V

    return-void
.end method

.method public constructor <init>(IJ[[Ljava/lang/String;)V
    .registers 7

    invoke-direct {p0}, Lcom/g/Hero;-><init>()V
    if-eqz p1, :cond_0

    return-void

    :cond_0
    return-void
.end method

.method public run()V
    .locals 1
    return-void
.end method
";

    fn hp<'a>() -> KeepField<'a> {
        KeepField { class: "com.g.Hero", owner: "com.g.Actor", name: "hp", signature: "I", is_static: false }
    }

    #[test]
    fn writes_values_as_smali() {
        assert_eq!(load_value("I", "999").unwrap(), "const v0, 0x3e7");
        assert_eq!(load_value("I", "-5").unwrap(), "const v0, -0x5");
        assert_eq!(load_value("J", "-9223372036854775808").unwrap(), "const-wide v0, -0x8000000000000000L");
        assert_eq!(load_value("F", "1.5").unwrap(), "const v0, 0x3fc00000    # 1.5f");
        assert_eq!(load_value("F", "-1").unwrap(), "const v0, -0x40800000    # -1f");
        assert_eq!(load_value("D", "2.0").unwrap(), "const-wide v0, 0x4000000000000000L    # 2");
        assert_eq!(load_value("Z", "true").unwrap(), "const/4 v0, 0x1");
        assert_eq!(load_value("C", "A").unwrap(), "const v0, 0x41");
        assert_eq!(load_value("Ljava/lang/String;", "\"Hi \"you\"\\ é\"").unwrap(), format!(r#"const-string v0, "Hi \"you\"\\ {}u00e9""#, '\\'));
        assert_eq!(load_value("Ljava/lang/String;", "null").unwrap(), "const/4 v0, 0x0");
        assert!(load_value("I", "lots").is_err());
        assert!(load_value("Lcom/g/Enemy;", "x").is_err());
    }

    #[test]
    fn sets_the_field_at_the_end_of_every_constructor() {
        let (text, places) = patch(HERO, "com.g.Hero", &hp(), "999").unwrap();
        assert_eq!(places, 3, "each return-void of each constructor");
        // .locals 0 grows to make room for the value.
        assert!(text.contains("<init>()V\n    .locals 1\n"));
        // 7 registers = 2 locals + this, int, long (2), array: v0 is free already.
        assert!(text.contains("    .registers 7\n"));
        assert_eq!(text.matches("    iput v0, p0, Lcom/g/Actor;->hp:I\n").count(), 3);
        assert!(text.contains("    :cond_0\n    # GameX keep: Lcom/g/Actor;->hp:I\n    const v0, 0x3e7\n"));
        // Other methods are untouched.
        assert!(text.contains(".method public run()V\n    .locals 1\n    return-void\n"));

        // Keeping again replaces the value instead of adding another.
        let (again, _) = patch(&text, "com.g.Hero", &hp(), "5").unwrap();
        assert!(!again.contains("0x3e7"));
        assert_eq!(again.matches("const v0, 0x5").count(), 3);
        assert_eq!(again.matches(BEGIN).count(), 3);
        assert!(again.contains("<init>()V\n    .locals 1\n"));
    }

    #[test]
    fn copies_a_far_this_next_to_the_value() {
        let smali = ".method public constructor <init>()V\n    .locals 20\n    return-void\n.end method\n";
        let wide = KeepField { signature: "D", ..hp() };
        let (text, _) = patch(smali, "com.g.Hero", &wide, "3").unwrap();
        assert!(text.contains("    move-object/from16 v2, p0\n    iput-wide v0, v2, Lcom/g/Actor;->hp:D\n"));
        assert!(text.contains(".locals 20\n"));
    }

    #[test]
    fn refuses_to_shift_parameters_out_of_reach() {
        let smali = ".method public constructor <init>(IIIIIIIIIIIIIII)V\n    .locals 0\n    return-void\n.end method\n";
        assert!(patch(smali, "com.g.Hero", &hp(), "1").is_err());
    }

    #[test]
    fn static_fields_go_in_the_static_initializer() {
        let level = KeepField { class: "com.g.Game", owner: "com.g.Game", name: "level", signature: "I", is_static: true };
        let smali = ".class public Lcom/g/Game;\n.field public static level:I\n";
        let (text, places) = patch(smali, "com.g.Game", &level, "3").unwrap();
        assert_eq!(places, 1);
        assert!(text.contains(".method static constructor <clinit>()V\n    .locals 1\n"));
        assert!(text.contains("    sput v0, Lcom/g/Game;->level:I\n"));
        // An existing one gets the assignment before it returns.
        let (again, places) = patch(&text, "com.g.Game", &level, "4").unwrap();
        assert_eq!(places, 1);
        assert_eq!(again.matches("<clinit>").count(), 1);
        assert!(again.contains("const v0, 0x4") && !again.contains("const v0, 0x3"));
    }

    #[test]
    fn private_and_final_fields_are_set_by_their_own_class() {
        let owner = ".class Lcom/g/Actor;\n.field private hp:I\n.field protected final max:I\n.field public x:F\n.field public static final N:I = 0x5\n";
        let mods = |n: &str, s: &str| field_modifiers(owner, n, s);
        assert_eq!(mods("N", "I").unwrap(), ["public", "static", "final", "="]);
        assert_eq!(target_class(&hp(), mods("hp", "I").as_deref()), "com.g.Actor");
        assert_eq!(target_class(&KeepField { name: "max", ..hp() }, mods("max", "I").as_deref()), "com.g.Actor");
        assert_eq!(target_class(&KeepField { name: "x", signature: "F", ..hp() }, mods("x", "F").as_deref()), "com.g.Hero");
        assert_eq!(mods("y", "F"), None);
    }

    #[test]
    fn keeps_a_change_as_an_undoable_edit() {
        let dir = std::env::temp_dir().join(format!("gamex-keep-{}", std::process::id()));
        let smali = dir.join("unpacked/apktool/smali_classes2/com/g");
        std::fs::create_dir_all(&smali).unwrap();
        std::fs::create_dir_all(dir.join("unpacked/apktool/smali")).unwrap();
        let actor = ".class Lcom/g/Actor;
.field private hp:I
.field public x:F
.field public static final N:I = 0x5

.method public constructor <init>()V
    .locals 0
    return-void
.end method
";
        std::fs::write(smali.join("Actor.smali"), actor).unwrap();
        std::fs::write(smali.join("Hero.smali"), HERO).unwrap();
        let store = EditStore::new(&dir);

        // Private: set by Actor itself.
        let kept = keep(&dir, &store, &hp(), "999").unwrap();
        assert_eq!((kept.file.as_str(), kept.class.as_str(), kept.places), ("apktool/smali_classes2/com/g/Actor.smali", "com.g.Actor", 1));
        // Public: set by the object's own class.
        let x = KeepField { name: "x", signature: "F", ..hp() };
        assert_eq!(keep(&dir, &store, &x, "12.5").unwrap().file, "apktool/smali_classes2/com/g/Hero.smali");
        let n = KeepField { name: "N", is_static: true, ..hp() };
        assert!(keep(&dir, &store, &n, "6").unwrap_err().contains("constant"));

        // The original is untouched; the edit is in the history and undoable.
        assert_eq!(std::fs::read_to_string(smali.join("Actor.smali")).unwrap(), actor);
        let state = store.state().unwrap();
        assert_eq!(state.edited.len(), 2);
        assert_eq!(state.undo.as_deref(), Some("Keep Hero.x = 12.5"));
        store.undo().unwrap();
        store.undo().unwrap();
        assert!(store.state().unwrap().edited.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn counts_parameter_registers() {
        assert_eq!(param_registers(".method public constructor <init>()V", false), 1);
        assert_eq!(param_registers(".method public constructor <init>(IJ[[Ljava/lang/String;D[I)V", false), 8);
        assert_eq!(param_registers(".method static constructor <clinit>()V", true), 0);
    }
}
