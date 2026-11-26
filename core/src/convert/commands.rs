//! Client commands: Fabric's Brigadier trees (`ClientCommands.literal("a").then(...).executes(...)`)
//! become Horizon usages (`Command::new("a b <n:integer>")`, pushed in `register_client_commands`)
//! and the code of each `executes`, run in the `command` hook when the typed command matches.
//!
//! Only trees built in the registration listener itself are followed; a usage whose argument
//! type has no rule is left out, and its code stays Java.

use super::java::{Translator, snake};
use super::{HookShape, Role, listener_body, text};
use std::collections::BTreeMap;
use tree_sitter::Node;

/// One word of a usage: a literal, or an argument with its Horizon kind.
#[derive(Clone, Debug)]
enum Part {
    Word(String),
    Arg { name: String, kind: &'static str },
}

/// A Brigadier argument type (`IntegerArgumentType.integer()`) as a Horizon argument kind.
fn kind(arg_type: &str) -> Option<&'static str> {
    let call = arg_type.rsplit('.').next()?.split('(').next()?;
    Some(match call {
        "integer" | "longArg" => "integer",
        "word" | "string" => "word",
        "greedyString" => "text",
        "bool" => "boolean",
        "doubleArg" | "floatArg" => "number",
        _ => return None,
    })
}

/// The Rust a typed word becomes, by kind: `None` for kinds kept as `&str`.
fn parse(kind: &str) -> Option<&'static str> {
    match kind {
        "integer" => Some("i64"),
        "boolean" => Some("bool"),
        "number" => Some("f64"),
        _ => None,
    }
}

fn named<'t>(node: Node<'t>) -> Vec<Node<'t>> {
    let mut c = node.walk();
    node.named_children(&mut c).collect()
}

/// The `literal(...)` or `argument(...)` a builder chain starts from.
fn base<'t>(mut node: Node<'t>, src: &str) -> Option<Node<'t>> {
    loop {
        if node.kind() != "method_invocation" {
            return None;
        }
        let name = text(node.child_by_field_name("name")?, src);
        if matches!(name, "literal" | "argument") {
            return Some(node);
        }
        node = node.child_by_field_name("object")?;
    }
}

fn part(base: Node, src: &str) -> Option<Part> {
    let name = text(base.child_by_field_name("name")?, src);
    let args = named(base.child_by_field_name("arguments")?);
    let word = args.first().filter(|a| a.kind() == "string_literal").map(|a| text(*a, src).trim_matches('"').to_string())?;
    if name == "literal" {
        let ok = !word.is_empty() && word.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        return ok.then_some(Part::Word(word));
    }
    Some(Part::Arg { name: word, kind: kind(text(*args.get(1)?, src))? })
}

/// The usage an `executes(...)` call runs for: its words and arguments from the root down. None
/// when the tree isn't built straight into `dispatcher.register(...)`, or a part has no rule.
fn usage(executes: Node, src: &str) -> Option<Vec<Part>> {
    let mut parts = vec![part(base(executes.child_by_field_name("object")?, src)?, src)?];
    let mut node = executes;
    loop {
        let parent = node.parent()?;
        match parent.kind() {
            // `.executes(...).then(...)`: still the same builder.
            "method_invocation" if parent.child_by_field_name("object").is_some_and(|o| o.id() == node.id()) => node = parent,
            "argument_list" => {
                let call = parent.parent()?;
                match text(call.child_by_field_name("name")?, src) {
                    "then" => {
                        parts.push(part(base(call.child_by_field_name("object")?, src)?, src)?);
                        node = call;
                    }
                    "register" => break,
                    _ => return None,
                }
            }
            _ => return None,
        }
    }
    parts.reverse();
    // A text argument takes the rest of the line: only last, and only alone.
    let texts = parts.iter().filter(|p| matches!(p, Part::Arg { kind: "text", .. })).count();
    if texts > 1 || (texts == 1 && !matches!(parts.last(), Some(Part::Arg { kind: "text", .. }))) {
        return None;
    }
    Some(parts)
}

fn usage_text(parts: &[Part]) -> String {
    parts
        .iter()
        .map(|p| match p {
            Part::Word(w) => w.clone(),
            Part::Arg { name, kind } => format!("<{}:{kind}>", snake(name)),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub struct Commands {
    /// `register_client_commands` lines.
    pub register: Vec<String>,
    /// `command` hook lines.
    pub run: Vec<String>,
    pub seen: usize,
    pub done: usize,
}

/// The commands a `ClientCommandRegistrationCallback` listener registers.
pub fn commands(body: Node, root: Node, src: &str, keys: &BTreeMap<String, String>, key_id: &dyn Fn(&str) -> String) -> Commands {
    let mut out = Commands { register: Vec::new(), run: Vec::new(), seen: 0, done: 0 };
    let mut stack = vec![body];
    let mut found = Vec::new();
    while let Some(n) = stack.pop() {
        if n.kind() == "method_invocation" && n.child_by_field_name("name").map(|x| text(x, src)) == Some("executes") {
            found.push(n);
        }
        let mut c = n.walk();
        stack.extend(n.children(&mut c));
    }
    found.sort_by_key(|n| n.start_byte());
    for executes in found {
        out.seen += 1;
        let Some(parts) = usage(executes, src) else { continue };
        let Some(listener) = executes.child_by_field_name("arguments").and_then(|a| named(a).into_iter().next()) else {
            continue;
        };
        let Some((params, code)) = listener_body(listener, root, src) else { continue };
        out.done += 1;
        let line = usage_text(&parts);
        out.register.push(format!("event.commands.push(Command::new({line:?}));"));

        // The code, with its arguments as Rust values bound below.
        let args: BTreeMap<String, String> = parts
            .iter()
            .filter_map(|p| match p {
                Part::Arg { name, .. } => Some((name.clone(), snake(name))),
                Part::Word(_) => None,
            })
            .collect();
        let shape = HookShape { hook: "command", params: &[Role::Command], flow: true, allow: false };
        let roles = params.iter().map(|p| (p.clone(), Role::Command)).collect();
        let mut t = Translator::new(src, roles, keys, key_id, shape).with_args(args);
        let mut code_lines = t.body(code);
        out.seen += t.seen;
        out.done += t.done;
        if !code_lines.last().is_some_and(|l| l.starts_with("return")) {
            code_lines.push("return Flow::Cancel;".into());
        }
        // An argument the converted code doesn't read (its code stayed Java) is bound as `_name`.
        let used = |v: &str| {
            code_lines.iter().filter(|l| !l.trim_start().starts_with("//")).any(|l| {
                l.match_indices(v).any(|(i, _)| {
                    let edge = |c: Option<char>| c.is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_'));
                    edge(l[..i].chars().next_back()) && edge(l[i + v.len()..].chars().next())
                })
            })
        };
        let bound = |name: &str| {
            let v = snake(name);
            if used(&v) { v } else { format!("_{v}") }
        };

        // `if let ["a", "b", n] = <words>[..] { if let Ok(n) = n.parse::<i64>() { ... } }`
        out.run.push(format!("// /{line}"));
        let mut depth = 0;
        let mut open = |line: String, run: &mut Vec<String>| {
            run.push(format!("{}{line}", "    ".repeat(depth)));
            depth += 1;
        };
        let text_arg = match parts.last() {
            Some(Part::Arg { name, kind: "text" }) => Some(bound(name)),
            _ => None,
        };
        let words: Vec<String> = parts
            .iter()
            .filter(|p| !matches!(p, Part::Arg { kind: "text", .. }))
            .map(|p| match p {
                Part::Word(w) => format!("{w:?}"),
                Part::Arg { name, kind } => {
                    // A value parsed below keeps its plain name until then.
                    if parse(kind).is_some() { snake(name) } else { bound(name) }
                }
            })
            .collect();
        match &text_arg {
            // Words, then the rest of the line.
            Some(rest) => {
                let prefix =
                    parts.iter().filter_map(|p| if let Part::Word(w) = p { Some(w.as_str()) } else { None }).collect::<Vec<_>>();
                if prefix.len() + 1 != parts.len() {
                    // Arguments before the text: not matched by a prefix. Keep it Java.
                    out.done -= 1 + t.done;
                    out.run.push(format!("// TODO (Java): the command /{line}"));
                    out.register.pop();
                    continue;
                }
                open(format!("if let Some({rest}) = event.command.strip_prefix(\"{} \") {{", prefix.join(" ")), &mut out.run);
            }
            None => open(
                format!("if let [{}] = event.command.split_whitespace().collect::<Vec<_>>()[..] {{", words.join(", ")),
                &mut out.run,
            ),
        }
        for p in &parts {
            if let Part::Arg { name, kind } = p {
                if let Some(ty) = parse(kind) {
                    let v = snake(name);
                    let b = bound(name);
                    open(format!("if let Ok({b}) = {v}.parse::<{ty}>() {{"), &mut out.run);
                }
            }
        }
        for l in code_lines {
            out.run.push(format!("{}{l}", "    ".repeat(depth)));
        }
        while depth > 0 {
            depth -= 1;
            out.run.push(format!("{}}}", "    ".repeat(depth)));
        }
    }
    out
}
