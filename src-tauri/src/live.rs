//! Live inspection of a running Java/Kotlin game over JDWP: count and list
//! the live objects of the game's classes, read their fields and change them
//! while the game runs. Nothing here pauses the game.

use std::collections::HashMap;
use std::io;

use serde::Serialize;

use crate::jdwp::{Jdwp, Value};

#[derive(Debug, Clone)]
struct FieldInfo {
    id: u64,
    name: String,
    signature: String,
    is_static: bool,
    declared_in: String,
}

pub struct LiveSession {
    vm: Jdwp,
    pub target: String,
    pub package: String,
    pub pid: u32,
    classes: HashMap<String, Option<u64>>,
    fields: HashMap<u64, Vec<FieldInfo>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassCount {
    pub class: String,
    /// `None` when the class isn't loaded yet (no object was ever made).
    pub count: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveInstance {
    /// The object's ID, as text (JS numbers can't hold 64 bits).
    pub id: String,
    pub summary: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveField {
    pub field_id: String,
    pub name: String,
    pub declared_in: String,
    /// "int", "float", "boolean", "String", "Enemy"...
    pub type_name: String,
    pub value: String,
    pub editable: bool,
    pub is_static: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveObject {
    pub id: String,
    pub class: String,
    pub fields: Vec<LiveField>,
}

/// `com.x.Enemy` <-> `Lcom/x/Enemy;`
fn signature_of(class: &str) -> String {
    format!("L{};", class.replace('.', "/"))
}

fn class_of_signature(sig: &str) -> String {
    sig.trim_start_matches('L').trim_end_matches(';').replace('/', ".")
}

/// A readable name for a field signature: `I` -> int, `Lcom/x/Foo;` -> Foo.
pub fn type_name(sig: &str) -> String {
    match sig {
        "I" => "int".into(),
        "J" => "long".into(),
        "S" => "short".into(),
        "B" => "byte".into(),
        "C" => "char".into(),
        "F" => "float".into(),
        "D" => "double".into(),
        "Z" => "boolean".into(),
        "Ljava/lang/String;" => "String".into(),
        s if s.starts_with('[') => format!("{}[]", type_name(&s[1..])),
        s => class_of_signature(s).rsplit('.').next().unwrap_or(s).to_string(),
    }
}

/// Field names shown in an object's one-line summary.
const SUMMARY_WORDS: &[&str] = &["x", "y", "hp", "health", "life", "lives", "state", "speed", "dir", "name", "energy"];

/// Parses what the user typed for a field of signature `sig`.
pub fn parse_input(sig: &str, text: &str) -> Result<Value, String> {
    let t = text.trim();
    let int = |min: i64, max: i64| -> Result<Value, String> {
        let n: i64 = t.parse().map_err(|_| format!("Enter a whole number for this {} field.", type_name(sig)))?;
        if n < min || n > max {
            return Err(format!("{n} doesn't fit in a {} ({min} to {max}).", type_name(sig)));
        }
        Ok(Value::Int(n))
    };
    match sig {
        "I" => int(i32::MIN as i64, i32::MAX as i64),
        "J" => int(i64::MIN, i64::MAX),
        "S" => int(i16::MIN as i64, i16::MAX as i64),
        "B" => int(i8::MIN as i64, i8::MAX as i64),
        "F" | "D" => t
            .trim_end_matches(['f', 'F', 'd', 'D'])
            .parse::<f64>()
            .ok()
            .filter(|f| f.is_finite())
            .map(Value::Float)
            .ok_or_else(|| "Enter a number, like 2.5.".to_string()),
        "Z" => match t {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            _ => Err("Enter true or false.".into()),
        },
        "C" => {
            let mut chars = t.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => Ok(Value::Char(c)),
                _ => Err("Enter a single character.".into()),
            }
        }
        _ => Err("Only numbers, true/false, characters and text can be changed here.".into()),
    }
}

impl LiveSession {
    pub fn new(vm: Jdwp, target: String, package: String, pid: u32) -> Self {
        LiveSession { vm, target, package, pid, classes: HashMap::new(), fields: HashMap::new() }
    }

    pub fn detach(&mut self) {
        self.vm.dispose();
    }

    fn class_id(&mut self, class: &str) -> io::Result<Option<u64>> {
        if let Some(id) = self.classes.get(class) {
            return Ok(*id);
        }
        let mut w = self.vm.writer();
        w.string(&signature_of(class));
        let data = self.vm.command(1, 2, &w.data)?;
        let mut r = self.vm.reader(&data);
        let id = if r.u32()? > 0 {
            r.u8()?; // type tag
            Some(r.reference_type()?)
        } else {
            None
        };
        // Only remember found classes: a missing one may load later.
        if id.is_some() {
            self.classes.insert(class.to_string(), id);
        }
        Ok(id)
    }

    /// How many live objects each class has right now.
    pub fn counts(&mut self, classes: &[String]) -> io::Result<Vec<ClassCount>> {
        let mut ids = Vec::new();
        for class in classes {
            ids.push(self.class_id(class)?);
        }
        let loaded: Vec<u64> = ids.iter().flatten().copied().collect();
        let mut counts = HashMap::new();
        if !loaded.is_empty() {
            let mut w = self.vm.writer();
            w.u32(loaded.len() as u32);
            for &id in &loaded {
                w.reference_type(id);
            }
            let data = self.vm.command(1, 21, &w.data)?;
            let mut r = self.vm.reader(&data);
            let n = r.u32()? as usize;
            for &id in loaded.iter().take(n) {
                counts.insert(id, r.u64()?);
            }
        }
        Ok(classes
            .iter()
            .zip(ids)
            .map(|(class, id)| ClassCount { class: class.clone(), count: id.and_then(|id| counts.get(&id).copied()) })
            .collect())
    }

    fn signature(&mut self, type_id: u64) -> io::Result<String> {
        let mut w = self.vm.writer();
        w.reference_type(type_id);
        let data = self.vm.command(2, 1, &w.data)?;
        self.vm.reader(&data).string()
    }

    fn superclass(&mut self, type_id: u64) -> io::Result<u64> {
        let mut w = self.vm.writer();
        w.reference_type(type_id);
        let data = self.vm.command(3, 1, &w.data)?;
        self.vm.reader(&data).reference_type()
    }

    /// Fields of a class and its game superclasses (not java.* / android.*).
    fn fields_of(&mut self, type_id: u64) -> io::Result<Vec<FieldInfo>> {
        if let Some(f) = self.fields.get(&type_id) {
            return Ok(f.clone());
        }
        let mut all = Vec::new();
        let mut current = type_id;
        for _ in 0..16 {
            if current == 0 {
                break;
            }
            let class = class_of_signature(&self.signature(current)?);
            if class.starts_with("java.") || class.starts_with("android.") || class.starts_with("kotlin.") {
                break;
            }
            let mut w = self.vm.writer();
            w.reference_type(current);
            let data = self.vm.command(2, 4, &w.data)?;
            let mut r = self.vm.reader(&data);
            let n = r.u32()?;
            let simple = class.rsplit('.').next().unwrap_or(&class).to_string();
            for _ in 0..n {
                let id = r.field()?;
                let name = r.string()?;
                let signature = r.string()?;
                let modifiers = r.u32()?;
                // Skip compiler-made fields (this$0 and the like).
                if modifiers & 0xF000_0000 != 0 || name.contains('$') {
                    continue;
                }
                all.push(FieldInfo { id, name, signature, is_static: modifiers & 0x0008 != 0, declared_in: simple.clone() });
            }
            current = self.superclass(current)?;
        }
        self.fields.insert(type_id, all.clone());
        Ok(all)
    }

    fn object_type(&mut self, object: u64) -> io::Result<u64> {
        let mut w = self.vm.writer();
        w.object(object);
        let data = self.vm.command(9, 1, &w.data)?;
        let mut r = self.vm.reader(&data);
        r.u8()?;
        r.reference_type()
    }

    /// Values of some fields of one object (instance and static).
    fn values(&mut self, object: u64, type_id: u64, fields: &[&FieldInfo]) -> io::Result<Vec<(u8, Value)>> {
        let mut out = vec![(b'V', Value::Void); fields.len()];
        for statics in [false, true] {
            let picked: Vec<(usize, &&FieldInfo)> = fields.iter().enumerate().filter(|(_, f)| f.is_static == statics).collect();
            if picked.is_empty() {
                continue;
            }
            let mut w = self.vm.writer();
            if statics {
                w.reference_type(type_id);
            } else {
                w.object(object);
            }
            w.u32(picked.len() as u32);
            for (_, f) in &picked {
                w.field(f.id);
            }
            let (set, cmd) = if statics { (2, 6) } else { (9, 2) };
            let data = self.vm.command(set, cmd, &w.data)?;
            let mut r = self.vm.reader(&data);
            r.u32()?;
            for (i, _) in &picked {
                out[*i] = r.value()?;
            }
        }
        Ok(out)
    }

    fn display(&mut self, tag: u8, value: &Value) -> io::Result<String> {
        Ok(match value {
            Value::Int(n) => n.to_string(),
            Value::Float(f) => {
                let s = format!("{f}");
                if s.contains('.') || s.contains('e') || s.contains("inf") || s.contains("NaN") { s } else { format!("{s}.0") }
            }
            Value::Bool(b) => b.to_string(),
            Value::Char(c) => c.to_string(),
            Value::Void => String::new(),
            Value::Object(0) => "null".into(),
            Value::Object(id) => match tag {
                b's' => {
                    let mut w = self.vm.writer();
                    w.object(*id);
                    let data = self.vm.command(10, 1, &w.data)?;
                    let text = self.vm.reader(&data).string()?;
                    format!("\"{}\"", text.chars().take(80).collect::<String>())
                }
                b'[' => {
                    let mut w = self.vm.writer();
                    w.object(*id);
                    let data = self.vm.command(13, 1, &w.data)?;
                    format!("array of {}", self.vm.reader(&data).u32()?)
                }
                _ => {
                    let t = self.object_type(*id)?;
                    let class = class_of_signature(&self.signature(t)?);
                    format!("{} #{id}", class.rsplit('.').next().unwrap_or(&class))
                }
            },
        })
    }

    /// Live objects of one class, each with a one-line summary.
    pub fn instances(&mut self, class: &str, max: u32) -> io::Result<Vec<LiveInstance>> {
        let Some(type_id) = self.class_id(class)? else { return Ok(Vec::new()) };
        let mut w = self.vm.writer();
        w.reference_type(type_id).u32(max);
        let data = self.vm.command(2, 16, &w.data)?;
        let mut r = self.vm.reader(&data);
        let n = r.u32()?;
        let mut objects = Vec::new();
        for _ in 0..n {
            r.u8()?;
            objects.push(r.object()?);
        }
        let fields = self.fields_of(type_id)?;
        let summary_fields: Vec<&FieldInfo> = fields
            .iter()
            .filter(|f| !f.is_static && SUMMARY_WORDS.contains(&f.name.to_lowercase().as_str()))
            .take(5)
            .collect();
        let mut out = Vec::new();
        for object in objects {
            let values = self.values(object, type_id, &summary_fields)?;
            let mut parts = Vec::new();
            for (f, (tag, v)) in summary_fields.iter().zip(values) {
                if matches!(v, Value::Int(_) | Value::Float(_) | Value::Bool(_)) {
                    let shown = self.display(tag, &v)?;
                    parts.push(format!("{}={}", f.name, trim_number(&shown)));
                }
            }
            out.push(LiveInstance { id: object.to_string(), summary: parts.join("  ") });
        }
        Ok(out)
    }

    /// Every field of one object with its current value.
    pub fn object(&mut self, object: u64) -> io::Result<LiveObject> {
        let type_id = self.object_type(object)?;
        let class = class_of_signature(&self.signature(type_id)?);
        let fields = self.fields_of(type_id)?;
        let refs: Vec<&FieldInfo> = fields.iter().collect();
        let values = self.values(object, type_id, &refs)?;
        let mut out = Vec::new();
        for (f, (tag, v)) in fields.iter().zip(values) {
            out.push(LiveField {
                field_id: f.id.to_string(),
                name: f.name.clone(),
                declared_in: f.declared_in.clone(),
                type_name: type_name(&f.signature),
                value: self.display(tag, &v)?,
                editable: f.signature.len() == 1 || f.signature == "Ljava/lang/String;",
                is_static: f.is_static,
            });
        }
        Ok(LiveObject { id: object.to_string(), class, fields: out })
    }

    /// Changes one field of a running object. Returns its new display value.
    pub fn set(&mut self, object: u64, field_id: u64, text: &str) -> Result<String, String> {
        let io = |e: io::Error| e.to_string();
        let type_id = self.object_type(object).map_err(io)?;
        let fields = self.fields_of(type_id).map_err(io)?;
        let field = fields.iter().find(|f| f.id == field_id).cloned().ok_or("That field isn't on this object.")?;
        let mut w = self.vm.writer();
        if field.is_static {
            w.reference_type(type_id);
        } else {
            w.object(object);
        }
        w.u32(1).field(field.id);
        if field.signature == "Ljava/lang/String;" {
            // Strings are objects: create one in the game, then point the field at it.
            let mut s = self.vm.writer();
            s.string(text);
            let data = self.vm.command(1, 11, &s.data).map_err(io)?;
            let string_id = self.vm.reader(&data).object().map_err(io)?;
            w.object(string_id);
        } else {
            let tag = field.signature.as_bytes()[0];
            let value = parse_input(&field.signature, text)?;
            w.untagged(tag, &value).map_err(io)?;
        }
        let (set, cmd) = if field.is_static { (3, 2) } else { (9, 3) };
        self.vm.command(set, cmd, &w.data).map_err(io)?;
        let values = self.values(object, type_id, &[&field]).map_err(io)?;
        let (tag, v) = &values[0];
        self.display(*tag, v).map_err(io)
    }
}

fn trim_number(s: &str) -> String {
    match s.split_once('.') {
        Some((whole, frac)) if frac.len() > 2 => format!("{whole}.{}", &frac[..2]),
        _ => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_types_and_parses_input() {
        assert_eq!(type_name("I"), "int");
        assert_eq!(type_name("Lcom/g/Enemy;"), "Enemy");
        assert_eq!(type_name("[[I"), "int[][]");
        assert_eq!(type_name("Ljava/lang/String;"), "String");

        assert_eq!(parse_input("I", " 42 "), Ok(Value::Int(42)));
        assert!(parse_input("B", "200").unwrap_err().contains("doesn't fit"));
        assert_eq!(parse_input("F", "2.5f"), Ok(Value::Float(2.5)));
        assert!(parse_input("D", "abc").is_err());
        assert!(parse_input("D", "inf").is_err());
        assert_eq!(parse_input("Z", "true"), Ok(Value::Bool(true)));
        assert!(parse_input("Z", "yes").is_err());
        assert_eq!(parse_input("C", "x"), Ok(Value::Char('x')));
        assert!(parse_input("Lcom/g/Enemy;", "x").is_err());
        assert_eq!(trim_number("3.14159"), "3.14");
    }
}
