//! Optional AI assistant: explains a file or selection, shows where it's
//! used, lists values worth tuning, and proposes edits for the user to
//! approve. GameX works fully without it (ARCHITECTURE.md, rule 1).
//!
//! The API key lives in the OS credential store (Windows Credential
//! Manager), never in a settings file. Provider and model are in `ai.json`.
//! Nothing is sent anywhere until the user clicks a button.

pub mod providers;

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::edits::is_editable;
use crate::preview;
use providers::{ModelInfo, Provider, Usage, Want};

const KEYRING_SERVICE: &str = "GameX";
const KEYRING_USER: &str = "ai-api-key";
/// Larger files must be narrowed down with a selection.
const MAX_FILE_CHARS: usize = 400_000;
const MAX_REFERENCES: usize = 30;

// ---------------------------------------------------------------------------
// Configuration

#[derive(Debug, Default, Serialize, Deserialize)]
struct AiConfig {
    provider: Option<Provider>,
    model: Option<String>,
}

fn config_path(data_dir: &Path) -> PathBuf {
    data_dir.join("ai.json")
}

fn load_config(data_dir: &Path) -> AiConfig {
    fs::read_to_string(config_path(data_dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_config(data_dir: &Path, config: &AiConfig) -> Result<(), String> {
    fs::create_dir_all(data_dir).map_err(|e| e.to_string())?;
    fs::write(config_path(data_dir), serde_json::to_string_pretty(config).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

fn key_entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).map_err(|e| format!("Couldn't open the credential store: {e}"))
}

fn load_key() -> Option<String> {
    key_entry().ok()?.get_password().ok()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiStatus {
    pub configured: bool,
    pub provider: Option<Provider>,
    pub provider_label: Option<String>,
    pub model: Option<String>,
    /// e.g. "sk-ant-…x9Qa", so the user can tell which key is stored.
    pub key_hint: Option<String>,
}

pub fn status(data_dir: &Path) -> AiStatus {
    let config = load_config(data_dir);
    let key = load_key();
    let provider = key.as_deref().and_then(Provider::detect);
    AiStatus {
        configured: provider.is_some() && config.model.is_some(),
        provider,
        provider_label: provider.map(|p| p.label().to_string()),
        model: config.model,
        key_hint: key.map(|k| {
            let k = k.trim();
            let head: String = k.chars().take(7).collect();
            let tail: String = k.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
            format!("{head}…{tail}")
        }),
    }
}

/// Checks the key by listing its models, then stores it and picks a model.
pub fn set_key(data_dir: &Path, key: &str) -> Result<(AiStatus, Vec<ModelInfo>), String> {
    let key = key.trim();
    let provider = Provider::detect(key).ok_or(
        "GameX doesn't recognize this key. Anthropic keys start with sk-ant-, OpenAI keys with sk-, OpenRouter keys with sk-or-, Gemini keys with AIza, and Groq keys with gsk_.",
    )?;
    let models = providers::list_models(provider, key)?;
    key_entry()?.set_password(key).map_err(|e| format!("Couldn't save the key: {e}"))?;
    save_config(
        data_dir,
        &AiConfig {
            provider: Some(provider),
            model: models.first().map(|m| m.id.clone()),
        },
    )?;
    Ok((status(data_dir), models))
}

pub fn models() -> Result<Vec<ModelInfo>, String> {
    let (provider, key) = credentials()?;
    providers::list_models(provider, &key)
}

pub fn set_model(data_dir: &Path, model: &str) -> Result<AiStatus, String> {
    let mut config = load_config(data_dir);
    config.model = Some(model.to_string());
    save_config(data_dir, &config)?;
    Ok(status(data_dir))
}

pub fn clear(data_dir: &Path) -> Result<AiStatus, String> {
    if let Ok(entry) = key_entry() {
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(e) => return Err(format!("Couldn't remove the key: {e}")),
        }
    }
    let _ = fs::remove_file(config_path(data_dir));
    Ok(status(data_dir))
}

fn credentials() -> Result<(Provider, String), String> {
    let key = load_key().ok_or("No AI key is set. Add one in Settings.")?;
    let provider = Provider::detect(&key).ok_or("The stored key isn't recognized. Add it again in Settings.")?;
    Ok((provider, key))
}

fn current_model(data_dir: &Path) -> Result<(Provider, String, String), String> {
    let (provider, key) = credentials()?;
    let model = load_config(data_dir).model.ok_or("Pick a model in Settings.")?;
    Ok((provider, key, model))
}

// ---------------------------------------------------------------------------
// Context: the file, its other form, and where it's used

#[derive(Debug, Serialize)]
pub struct Reference {
    /// The referring class, e.g. `com.game.World`.
    pub class: String,
    pub method: Option<String>,
    pub line: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct References {
    pub items: Vec<Reference>,
    /// How many more were found than are listed.
    pub more: usize,
    /// False for files that aren't Java/smali classes.
    pub supported: bool,
}

/// Where other classes use the class in `path` (a jadx Java or smali file).
/// Needs no AI: it searches the smali GameX already has.
pub fn references(project_root: &Path, path: &str) -> References {
    match class_descriptor(path) {
        Some(descriptor) => {
            let (items, more) = find_references(project_root, &descriptor);
            References { items, more, supported: true }
        }
        None => References {
            items: Vec::new(),
            more: 0,
            supported: false,
        },
    }
}

struct FileContext {
    path: String,
    text: String,
    selection: Option<String>,
    /// The same class in its other form: (path, text).
    counterpart: Option<(String, String)>,
    /// The file edits go to: this one if editable, else its smali.
    edit_target: Option<String>,
    references: Vec<Reference>,
    more_references: usize,
}

fn smali_dirs(project_root: &Path) -> Vec<String> {
    let apktool = project_root.join("unpacked/apktool");
    let mut dirs: Vec<String> = fs::read_dir(apktool)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("smali"))
        .collect();
    dirs.sort();
    dirs
}

/// `jadx/sources/a/B.java` -> `apktool/smali*/a/B.smali`, if it exists.
fn java_to_smali(project_root: &Path, path: &str) -> Option<String> {
    let rel = path.strip_prefix("jadx/sources/")?.strip_suffix(".java")?;
    smali_dirs(project_root)
        .into_iter()
        .map(|d| format!("apktool/{d}/{rel}.smali"))
        .find(|p| project_root.join("unpacked").join(p).is_file())
}

/// `apktool/smali_classes2/a/B$1.smali` -> `jadx/sources/a/B.java`, if it exists.
fn smali_to_java(project_root: &Path, path: &str) -> Option<String> {
    let rest = path.strip_prefix("apktool/")?;
    let (dir, rel) = rest.split_once('/')?;
    if !dir.starts_with("smali") {
        return None;
    }
    let rel = rel.strip_suffix(".smali")?;
    let outer = rel.split('$').next()?;
    let java = format!("jadx/sources/{outer}.java");
    project_root.join("unpacked").join(&java).is_file().then_some(java)
}

/// `La/b/C;` for a smali or jadx Java path.
fn class_descriptor(path: &str) -> Option<String> {
    let rel = if let Some(r) = path.strip_prefix("jadx/sources/") {
        r.strip_suffix(".java")?
    } else {
        let (dir, rel) = path.strip_prefix("apktool/")?.split_once('/')?;
        if !dir.starts_with("smali") {
            return None;
        }
        rel.strip_suffix(".smali")?
    };
    Some(format!("L{rel};"))
}

/// Lines in other classes' smali that mention this class.
fn find_references(project_root: &Path, descriptor: &str) -> (Vec<Reference>, usize) {
    let own_prefix = descriptor.trim_end_matches(';');
    let mut found = Vec::new();
    let mut total: usize = 0;
    for dir in smali_dirs(project_root) {
        let base = project_root.join("unpacked/apktool").join(&dir);
        for entry in walkdir::WalkDir::new(&base).into_iter().filter_map(Result::ok) {
            if !entry.file_type().is_file() || entry.path().extension().is_none_or(|e| e != "smali") {
                continue;
            }
            let Ok(rel) = entry.path().strip_prefix(&base) else { continue };
            let class = rel.to_string_lossy().replace('\\', "/");
            let class = class.trim_end_matches(".smali");
            // Skip the class itself and its inner classes.
            if format!("L{class}") == own_prefix || format!("L{class}").starts_with(&format!("{own_prefix}$")) {
                continue;
            }
            let Ok(text) = fs::read_to_string(entry.path()) else { continue };
            if !text.contains(descriptor) {
                continue;
            }
            let mut method = None;
            for line in text.lines() {
                let line = line.trim();
                if let Some(sig) = line.strip_prefix(".method ") {
                    method = sig.rsplit(' ').next().map(|s| s.split('(').next().unwrap_or(s).to_string());
                } else if line.contains(descriptor) && !line.starts_with(".class") && !line.starts_with(".source") {
                    total += 1;
                    if found.len() < MAX_REFERENCES {
                        found.push(Reference {
                            class: class.replace('/', "."),
                            method: method.clone(),
                            line: line.chars().take(200).collect(),
                        });
                    }
                }
            }
        }
    }
    (found, total.saturating_sub(MAX_REFERENCES))
}

fn build_context(project_root: &Path, path: &str, selection: Option<&str>) -> Result<FileContext, String> {
    let text = preview::read_text(project_root, path)
        .map_err(|e| format!("{e} The AI assistant reads text files such as code, smali and XML."))?;
    let selection = selection.map(str::trim).filter(|s| !s.is_empty()).map(String::from);
    if text.chars().count() > MAX_FILE_CHARS && selection.is_none() {
        return Err(format!(
            "This file is too large to send whole ({} KB). Select the part you want to ask about in the preview first.",
            text.len() / 1024
        ));
    }
    let counterpart_path = if path.starts_with("jadx/") {
        java_to_smali(project_root, path)
    } else {
        smali_to_java(project_root, path)
    };
    let counterpart = counterpart_path.and_then(|p| {
        let t = preview::read_text(project_root, &p).ok()?;
        (t.chars().count() <= MAX_FILE_CHARS).then_some((p, t))
    });
    let edit_target = if is_editable(path) {
        Some(path.to_string())
    } else {
        counterpart.as_ref().map(|(p, _)| p.clone()).filter(|p| is_editable(p))
    };
    let (references, more_references) = match class_descriptor(path) {
        Some(d) => find_references(project_root, &d),
        None => (Vec::new(), 0),
    };
    Ok(FileContext {
        path: path.to_string(),
        text,
        selection,
        counterpart,
        edit_target,
        references,
        more_references,
    })
}

fn describe(path: &str) -> &'static str {
    if path.starts_with("jadx/") {
        "decompiled Java from jadx (a read-only view; names may be obfuscated)"
    } else if path.ends_with(".smali") {
        "smali, the Dalvik bytecode apktool produced (this is what gets rebuilt)"
    } else {
        "a game file"
    }
}

/// The shared part of every prompt: the file, optionally its other form,
/// the selection and up to `max_refs` references. The file itself is never
/// trimmed; only these optional extras are left out to fit a size limit.
fn context_prompt(ctx: &FileContext, with_counterpart: bool, max_refs: usize) -> String {
    let mut out = String::new();
    let file_too_big = ctx.text.chars().count() > MAX_FILE_CHARS;
    if !file_too_big {
        out.push_str(&format!(
            "<file path=\"{}\" kind=\"{}\">\n{}\n</file>\n\n",
            ctx.path,
            describe(&ctx.path),
            ctx.text
        ));
    }
    if let Some((p, t)) = ctx.counterpart.as_ref().filter(|_| with_counterpart) {
        out.push_str(&format!(
            "<same_class_other_form path=\"{p}\" kind=\"{}\">\n{t}\n</same_class_other_form>\n\n",
            describe(p)
        ));
    }
    if let Some(sel) = &ctx.selection {
        out.push_str(&format!(
            "The user selected this part of {} and is asking about it specifically:\n<selection>\n{sel}\n</selection>\n\n",
            ctx.path
        ));
    }
    if max_refs == 0 {
        return out;
    }
    if ctx.references.is_empty() {
        if class_descriptor(&ctx.path).is_some() {
            out.push_str("No other class refers to this class directly.\n\n");
        }
    } else {
        out.push_str("Where other classes use this class (smali lines):\n");
        for r in ctx.references.iter().take(max_refs) {
            let method = r.method.as_deref().unwrap_or("?");
            out.push_str(&format!("- {} in {}(): {}\n", r.class, method, r.line));
        }
        let more = ctx.more_references + ctx.references.len().saturating_sub(max_refs);
        if more > 0 {
            out.push_str(&format!("- ...and {more} more\n"));
        }
        out.push('\n');
    }
    out
}

/// Sends a request, and if the provider says it's too large, retries with
/// less optional context: first without the class's other form (unless the
/// edit needs it), then with fewer references, then with none. Returns the
/// reply and, if anything was left out, a note saying what.
fn complete_fitting(
    (provider, key, model): &(Provider, String, String),
    ctx: &FileContext,
    counterpart_required: bool,
    prompt: impl Fn(&str) -> String,
    want: &Want,
    effort: &str,
) -> Result<(providers::Reply, Option<String>), String> {
    // The Java form helps explain smali; the smali form adds little to Java.
    let counterpart_wanted = counterpart_required || (ctx.path.ends_with(".smali") && ctx.counterpart.is_some());
    let mut levels = vec![
        (counterpart_wanted, MAX_REFERENCES),
        (counterpart_required, MAX_REFERENCES),
        (counterpart_required, 5),
        (counterpart_required, 0),
    ];
    levels.dedup();
    for &(with_counterpart, max_refs) in &levels {
        let user = prompt(&context_prompt(ctx, with_counterpart, max_refs));
        match providers::complete(*provider, key, model, BASE_SYSTEM, &user, want, effort) {
            Ok(reply) => {
                let mut left_out = Vec::new();
                if counterpart_wanted && !with_counterpart {
                    left_out.push("the class's other form (Java/smali)");
                }
                if !ctx.references.is_empty() && max_refs < ctx.references.len() {
                    left_out.push(if max_refs == 0 { "the \"where it's used\" references" } else { "most of the \"where it's used\" references" });
                }
                let note = (!left_out.is_empty())
                    .then(|| format!("To fit the provider's size limit, this was sent without {}.", left_out.join(" and ")));
                return Ok((reply, note));
            }
            Err(e) if providers::is_too_large(&e) => continue,
            Err(e) => return Err(e),
        }
    }
    Err(format!(
        "{} Even without the extra context it doesn't fit. Select a smaller part of the file (in the preview or the editor), or pick a model or plan with a higher limit in Settings.",
        providers::TOO_LARGE
    ))
}

const BASE_SYSTEM: &str = "You help people understand and modify offline, single-player games inside GameX, a game editor. \
The person may not be a programmer: explain what code does in the game, in plain language, not just line by line. \
Code is either decompiled Java (from jadx: readable, but read-only, and names may be obfuscated) or smali (Dalvik \
bytecode from apktool: harder to read, but it is what gets rebuilt). When both forms of a class are given they are the \
same code. Say so when something is uncertain, for example because names are obfuscated.";

// ---------------------------------------------------------------------------
// Explain

#[derive(Debug, Serialize, Deserialize)]
pub struct UsageNote {
    #[serde(rename = "where")]
    pub location: String,
    pub how: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TunableValue {
    pub id: String,
    pub name: String,
    pub description: String,
    pub current: String,
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Explanation {
    pub summary: String,
    pub explanation: String,
    pub usage: Vec<UsageNote>,
    pub values: Vec<TunableValue>,
    /// Where proposed changes would be written, if anywhere.
    pub edit_target: Option<String>,
    pub tokens: Usage,
    pub model: String,
    /// What was left out to fit a size limit, if anything.
    pub trimmed: Option<String>,
}

#[derive(Deserialize)]
struct ExplainJson {
    summary: String,
    explanation: String,
    usage: Vec<UsageNote>,
    values: Vec<TunableValue>,
}

fn explain_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["summary", "explanation", "usage", "values"],
        "properties": {
            "summary": {"type": "string"},
            "explanation": {"type": "string"},
            "usage": {"type": "array", "items": {
                "type": "object", "additionalProperties": false,
                "required": ["where", "how"],
                "properties": {"where": {"type": "string"}, "how": {"type": "string"}}
            }},
            "values": {"type": "array", "items": {
                "type": "object", "additionalProperties": false,
                "required": ["id", "name", "description", "current", "type"],
                "properties": {
                    "id": {"type": "string"},
                    "name": {"type": "string"},
                    "description": {"type": "string"},
                    "current": {"type": "string"},
                    "type": {"type": "string", "enum": ["integer", "decimal", "boolean", "text"]}
                }
            }}
        }
    })
}

const EXPLAIN_TASK: &str = "Explain this for someone modding the game.\n\
- summary: one sentence on what it is in the game.\n\
- explanation: Markdown, short paragraphs and lists: what it does, how it behaves while playing, anything notable.\n\
- usage: where and how the game uses it, from the references given (each with a place and what happens there). Empty if nothing uses it.\n\
- values: literal values in the selection (or the whole file if nothing is selected) that a player-facing tweak would change: \
speeds, health, damage, timers, counts, sizes, chances, on/off flags. Up to 15, most interesting first. Skip purely technical \
numbers (indexes, resource IDs, bit masks, register numbers) unless the user selected them. For each: a short stable id \
(snake_case), a plain name, what changing it does in the game, the current value as plain decimal or true/false, and its type.";

pub fn explain(data_dir: &Path, project_root: &Path, path: &str, selection: Option<&str>) -> Result<Explanation, String> {
    let credentials = current_model(data_dir)?;
    let ctx = build_context(project_root, path, selection)?;
    let schema = explain_schema();
    let (reply, trimmed) = complete_fitting(
        &credentials,
        &ctx,
        false,
        |context| format!("{context}{EXPLAIN_TASK}"),
        &Want::Json(&schema),
        "medium",
    )?;
    let parsed: ExplainJson = parse_json(&reply.text)?;
    Ok(Explanation {
        summary: parsed.summary,
        explanation: parsed.explanation,
        usage: parsed.usage,
        values: parsed.values,
        edit_target: ctx.edit_target,
        tokens: reply.usage,
        model: reply.model,
        trimmed,
    })
}

// ---------------------------------------------------------------------------
// Ask

#[derive(Debug, Serialize)]
pub struct Answer {
    pub answer: String,
    pub tokens: Usage,
    pub model: String,
    pub trimmed: Option<String>,
}

pub fn ask(data_dir: &Path, project_root: &Path, path: &str, selection: Option<&str>, question: &str) -> Result<Answer, String> {
    let question = question.trim();
    if question.is_empty() {
        return Err("Type a question first.".into());
    }
    let credentials = current_model(data_dir)?;
    let ctx = build_context(project_root, path, selection)?;
    let (reply, trimmed) = complete_fitting(
        &credentials,
        &ctx,
        false,
        |context| format!("{context}The user's question:\n{question}\n\nAnswer in Markdown."),
        &Want::Text,
        "medium",
    )?;
    Ok(Answer {
        answer: reply.text,
        tokens: reply.usage,
        model: reply.model,
        trimmed,
    })
}

// ---------------------------------------------------------------------------
// Propose a change

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValueChange {
    pub id: String,
    pub name: String,
    pub current: String,
    pub new_value: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TextEdit {
    pub find: String,
    pub replace: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Proposal {
    pub summary: String,
    pub notes: String,
    /// The file the change applies to (relative to `unpacked/`).
    pub target: String,
    /// The target's text the edits were made against, to detect changes
    /// made in between.
    pub original: String,
    pub updated: String,
    pub edits: Vec<TextEdit>,
    pub tokens: Usage,
    pub model: String,
    pub trimmed: Option<String>,
}

#[derive(Deserialize)]
struct ProposeJson {
    summary: String,
    notes: String,
    edits: Vec<TextEdit>,
}

fn propose_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["summary", "notes", "edits"],
        "properties": {
            "summary": {"type": "string"},
            "notes": {"type": "string"},
            "edits": {"type": "array", "items": {
                "type": "object", "additionalProperties": false,
                "required": ["find", "replace"],
                "properties": {"find": {"type": "string"}, "replace": {"type": "string"}}
            }}
        }
    })
}

const PROPOSE_RULES: &str = "Make the requested change by returning search-and-replace edits for the EDIT TARGET file only.\n\
- Each `find` must be copied exactly from the edit target, including indentation, and must appear exactly once in it. \
Include a neighbouring line when needed to make it unique.\n\
- Keep edits minimal and change nothing else.\n\
- Smali: const/4 holds -8..7, const/16 holds -32768..32767, const holds any 32-bit value; use const-wide/16, \
const-wide/32 or const-wide for 64-bit registers. If a new value no longer fits the instruction, switch to a wider \
one with the same register. Floats are stored as their IEEE-754 bits (5.0f is 0x40a00000): use const, or const/high16 \
only when the low 16 bits are zero. Put a comment line `# GameX: <name> = <new value>` above each changed line.\n\
- Don't change register numbers, .locals, labels or method signatures unless the change needs it.\n\
- summary: one sentence describing the change. notes: anything the user should know (side effects, uncertainty).\n\
- If the change can't be made safely, return no edits and explain why in notes.";

pub fn propose(
    data_dir: &Path,
    project_root: &Path,
    path: &str,
    selection: Option<&str>,
    changes: &[ValueChange],
    instruction: &str,
) -> Result<Proposal, String> {
    let instruction = instruction.trim();
    if changes.is_empty() && instruction.is_empty() {
        return Err("Enter at least one new value or describe the change you want.".into());
    }
    let credentials = current_model(data_dir)?;
    let ctx = build_context(project_root, path, selection)?;
    let target = ctx.edit_target.clone().ok_or(
        "There is no editable version of this file. For APKs, apktool must have decoded the game during import.",
    )?;
    let original = preview::read_text(project_root, &target).map_err(|e| e.to_string())?;

    let mut request = String::new();
    for c in changes {
        request.push_str(&format!("- {} ({}): change from {} to {}\n", c.name, c.id, c.current, c.new_value));
    }
    if !instruction.is_empty() {
        request.push_str(&format!("- Also: {instruction}\n"));
    }
    let target_block = if target == ctx.path {
        format!("The EDIT TARGET is {target} (the file shown above).\n\n")
    } else {
        format!("The EDIT TARGET is {target} (the same_class_other_form shown above).\n\n")
    };
    let schema = propose_schema();
    let (reply, trimmed) = complete_fitting(
        &credentials,
        &ctx,
        // The edit target must be sent when it's the other form.
        target != ctx.path,
        |context| format!("{context}{target_block}Requested changes:\n{request}\n{PROPOSE_RULES}"),
        &Want::Json(&schema),
        "high",
    )?;
    let parsed: ProposeJson = parse_json(&reply.text)?;
    if parsed.edits.is_empty() {
        return Err(if parsed.notes.trim().is_empty() {
            "The AI didn't propose any edits.".into()
        } else {
            format!("The AI didn't propose any edits: {}", parsed.notes.trim())
        });
    }
    let updated = apply_edits(&original, &parsed.edits)?;
    if target.ends_with(".smali") {
        check_smali_constants(&parsed.edits)?;
    }
    Ok(Proposal {
        summary: parsed.summary,
        notes: parsed.notes,
        target,
        original,
        updated,
        edits: parsed.edits,
        tokens: reply.usage,
        model: reply.model,
        trimmed,
    })
}

/// Applies search-and-replace edits; each `find` must match exactly once.
fn apply_edits(original: &str, edits: &[TextEdit]) -> Result<String, String> {
    // Files may use CRLF while the model answers with LF.
    let crlf = original.contains("\r\n");
    let mut text = original.replace("\r\n", "\n");
    for (i, edit) in edits.iter().enumerate() {
        let find = edit.find.replace("\r\n", "\n");
        if find.is_empty() {
            return Err(format!("Proposed edit {} is empty. Try again.", i + 1));
        }
        match text.matches(&find).count() {
            1 => text = text.replacen(&find, &edit.replace.replace("\r\n", "\n"), 1),
            0 => {
                return Err(format!(
                    "Proposed edit {} doesn't match the file exactly, so it wasn't applied. Try again.",
                    i + 1
                ))
            }
            n => {
                return Err(format!(
                    "Proposed edit {} matches {n} places in the file, so it's ambiguous. Try again, or select the exact part to change.",
                    i + 1
                ))
            }
        }
    }
    Ok(if crlf { text.replace('\n', "\r\n") } else { text })
}

fn parse_number(token: &str) -> Option<i128> {
    let t = token.trim().trim_end_matches(['L', 'l', 't', 's']);
    let (neg, t) = match t.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, t),
    };
    let value = match t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        Some(hex) => i128::from_str_radix(hex, 16).ok()?,
        None => t.parse().ok()?,
    };
    Some(if neg { -value } else { value })
}

/// Catches the most common smali mistake: a constant that doesn't fit its
/// instruction, which would fail the build.
fn check_smali_constants(edits: &[TextEdit]) -> Result<(), String> {
    for line in edits.iter().flat_map(|e| e.replace.lines()) {
        let line = line.trim();
        let Some((op, rest)) = line.split_once(char::is_whitespace) else { continue };
        let Some(value) = rest.rsplit(',').next().and_then(parse_number) else { continue };
        let ok = match op {
            "const/4" => (-8..=7).contains(&value),
            "const/16" | "const-wide/16" => (-32768..=32767).contains(&value),
            "const" => (i32::MIN as i128..=u32::MAX as i128).contains(&value),
            "const-wide/32" => (i32::MIN as i128..=i32::MAX as i128).contains(&value),
            "const/high16" => (value & 0xffff) == 0 && (i32::MIN as i128..=u32::MAX as i128).contains(&value),
            _ => true,
        };
        if !ok {
            return Err(format!(
                "The proposed smali has a value that doesn't fit its instruction (`{line}`), which would break the build. Try again."
            ));
        }
    }
    Ok(())
}

/// Accepts a bare JSON object, or one wrapped in a code fence or prose.
fn parse_json<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, String> {
    let trimmed = text.trim();
    let candidate = match (trimmed.find('{'), trimmed.rfind('}')) {
        (Some(start), Some(end)) if end > start => &trimmed[start..=end],
        _ => trimmed,
    };
    serde_json::from_str(candidate).map_err(|e| format!("The AI's answer wasn't in the expected format ({e}). Try again."))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(root: &Path, rel: &str, text: &str) {
        let p = root.join("unpacked").join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }

    #[test]
    fn pairs_java_with_smali_and_finds_references() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(root, "jadx/sources/com/g/Hero.java", "class Hero { int speed = 3; }");
        touch(root, "apktool/smali_classes2/com/g/Hero.smali", ".class public Lcom/g/Hero;\n");
        touch(root, "apktool/smali_classes2/com/g/Hero$1.smali", ".class Lcom/g/Hero$1;\n    iget v0, p0, Lcom/g/Hero;->speed:I\n");
        touch(
            root,
            "apktool/smali/com/g/World.smali",
            ".class public Lcom/g/World;\n.method public update()V\n    new-instance v0, Lcom/g/Hero;\n.end method\n",
        );

        assert_eq!(java_to_smali(root, "jadx/sources/com/g/Hero.java").as_deref(), Some("apktool/smali_classes2/com/g/Hero.smali"));
        assert_eq!(smali_to_java(root, "apktool/smali_classes2/com/g/Hero$1.smali").as_deref(), Some("jadx/sources/com/g/Hero.java"));
        assert_eq!(class_descriptor("jadx/sources/com/g/Hero.java").as_deref(), Some("Lcom/g/Hero;"));

        let ctx = build_context(root, "jadx/sources/com/g/Hero.java", None).unwrap();
        assert_eq!(ctx.edit_target.as_deref(), Some("apktool/smali_classes2/com/g/Hero.smali"));
        // Hero$1 is part of Hero, so only World counts.
        assert_eq!(ctx.references.len(), 1);
        assert_eq!(ctx.references[0].class, "com.g.World");
        assert_eq!(ctx.references[0].method.as_deref(), Some("update"));
        assert!(context_prompt(&ctx, true, MAX_REFERENCES).contains("com.g.World in update()"));
        assert!(context_prompt(&ctx, true, MAX_REFERENCES).contains("<same_class_other_form"));
        let lean = context_prompt(&ctx, false, 0);
        assert!(!lean.contains("same_class_other_form") && !lean.contains("World"));
        assert!(lean.contains("class Hero"));
    }

    #[test]
    fn applies_unique_edits_only() {
        let original = "a\r\nconst/4 v0, 0x3\r\nb\r\nb\r\n";
        let edit = |f: &str, r: &str| TextEdit { find: f.into(), replace: r.into() };
        let out = apply_edits(original, &[edit("const/4 v0, 0x3", "const/16 v0, 0x14")]).unwrap();
        assert_eq!(out, "a\r\nconst/16 v0, 0x14\r\nb\r\nb\r\n");
        assert!(apply_edits(original, &[edit("b", "c")]).unwrap_err().contains("2 places"));
        assert!(apply_edits(original, &[edit("zzz", "c")]).is_err());
    }

    #[test]
    fn rejects_constants_that_do_not_fit() {
        let edit = |r: &str| vec![TextEdit { find: String::new(), replace: r.into() }];
        assert!(check_smali_constants(&edit("    const/4 v0, 0x7")).is_ok());
        assert!(check_smali_constants(&edit("    const/4 v0, -0x8")).is_ok());
        assert!(check_smali_constants(&edit("    const/4 v0, 0x14")).is_err());
        assert!(check_smali_constants(&edit("    const/16 v1, 0x7fff")).is_ok());
        assert!(check_smali_constants(&edit("    const/16 v1, 0x10000")).is_err());
        assert!(check_smali_constants(&edit("    const/high16 v2, 0x40a00000")).is_ok());
        assert!(check_smali_constants(&edit("    const/high16 v2, 0x40a00001")).is_err());
        assert!(check_smali_constants(&edit("    const v2, 0x40a00001")).is_ok());
    }

    #[test]
    fn parses_json_with_or_without_fences() {
        #[derive(Deserialize)]
        struct T {
            a: i32,
        }
        assert_eq!(parse_json::<T>("{\"a\": 1}").unwrap().a, 1);
        assert_eq!(parse_json::<T>("```json\n{\"a\": 2}\n```").unwrap().a, 2);
        assert!(parse_json::<T>("nope").is_err());
    }

    #[test]
    fn large_files_need_a_selection() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "files/big.txt", &"x".repeat(MAX_FILE_CHARS + 1));
        assert!(build_context(dir.path(), "files/big.txt", None).is_err());
        let ctx = build_context(dir.path(), "files/big.txt", Some("xxx")).unwrap();
        // Only the selection is sent for a file that large.
        assert!(!context_prompt(&ctx, true, MAX_REFERENCES).contains("<file"));
    }
}
