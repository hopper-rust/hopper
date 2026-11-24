//! Java statements and expressions to Rust, by rules. Every rule either produces Rust it is sure
//! of or gives up (None): the statement around it is then kept as Java in a TODO comment.

use super::{HookShape, Role, text};
use std::collections::{BTreeMap, BTreeSet};
use tree_sitter::Node;

pub struct Translator<'a> {
    src: &'a str,
    roles: BTreeMap<String, Role>,
    /// Variables holding a key mapping → its lang key.
    keys: &'a BTreeMap<String, String>,
    key_id: &'a dyn Fn(&str) -> String,
    shape: HookShape,
    /// Locals declared so far (Java name → Rust name).
    locals: BTreeMap<String, String>,
    /// Locals assigned after their declaration, which need `let mut`.
    reassigned: BTreeSet<String>,
    /// Each converted `let`: its Rust name and the Java it came from, checked once the body is done.
    lets: Vec<(String, String)>,
    /// Statements seen, and converted.
    pub seen: usize,
    pub done: usize,
}

/// `camelCase` → `snake_case`.
fn snake(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

fn named<'t>(node: Node<'t>) -> Vec<Node<'t>> {
    let mut c = node.walk();
    node.named_children(&mut c).filter(|n| n.kind() != "line_comment" && n.kind() != "block_comment").collect()
}

impl<'a> Translator<'a> {
    pub fn new(
        src: &'a str,
        roles: BTreeMap<String, Role>,
        keys: &'a BTreeMap<String, String>,
        key_id: &'a dyn Fn(&str) -> String,
        shape: HookShape,
    ) -> Self {
        Self {
            src,
            roles,
            keys,
            key_id,
            shape,
            locals: BTreeMap::new(),
            reassigned: BTreeSet::new(),
            lets: Vec::new(),
            seen: 0,
            done: 0,
        }
    }

    fn t(&self, node: Node) -> &'a str {
        text(node, self.src)
    }

    /// The listener's body: a block, or a lambda's single expression.
    pub fn body(&mut self, body: Node) -> Vec<String> {
        self.find_reassigned(body);
        if body.kind() == "block" {
            let lines = self.block(body, 0);
            self.tidy_lets(lines)
        } else {
            self.seen += 1;
            match self.statement_expr(body) {
                Some(s) => {
                    self.done += 1;
                    vec![s]
                }
                None => self.keep_java(body, 0),
            }
        }
    }

    fn find_reassigned(&mut self, node: Node) {
        let mut stack = vec![node];
        while let Some(n) = stack.pop() {
            match n.kind() {
                "assignment_expression" => {
                    if let Some(l) = n.child_by_field_name("left").filter(|l| l.kind() == "identifier") {
                        self.reassigned.insert(self.t(l).to_string());
                    }
                }
                "update_expression" => {
                    if let Some(l) = named(n).into_iter().find(|c| c.kind() == "identifier") {
                        self.reassigned.insert(self.t(l).to_string());
                    }
                }
                _ => {}
            }
            let mut c = n.walk();
            stack.extend(n.children(&mut c));
        }
    }

    /// A converted variable that no converted line uses goes back to Java (it would only be an
    /// unused variable), and one no converted line changes loses its `mut`.
    fn tidy_lets(&mut self, mut lines: Vec<String>) -> Vec<String> {
        let code = |lines: &[String], skip: usize| -> Vec<String> {
            lines
                .iter()
                .enumerate()
                .filter(|(i, l)| *i != skip && !l.trim_start().starts_with("//"))
                .map(|(_, l)| l.clone())
                .collect()
        };
        let word = |line: &str, name: &str| {
            line.match_indices(name).any(|(i, _)| {
                let before = line[..i].chars().next_back();
                let after = line[i + name.len()..].chars().next();
                let edge = |c: Option<char>| c.is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_'));
                edge(before) && edge(after)
            })
        };
        for (name, java) in std::mem::take(&mut self.lets) {
            let Some(at) = lines.iter().position(|l| {
                let t = l.trim_start();
                t.starts_with(&format!("let {name} = ")) || t.starts_with(&format!("let mut {name} = "))
            }) else {
                continue;
            };
            let rest = code(&lines, at);
            let pad: String = lines[at].chars().take_while(|c| *c == ' ').collect();
            if !rest.iter().any(|l| word(l, &name)) {
                let back: Vec<String> = java.lines().map(str::trim).filter(|l| !l.is_empty()).map(|l| format!("{pad}// TODO (Java): {l}")).collect();
                lines.splice(at..=at, back);
                self.done -= 1;
                continue;
            }
            let changed = rest.iter().any(|l| {
                let t = l.trim_start();
                ["=", "+=", "-=", "*=", "/="].iter().any(|op| t.starts_with(&format!("{name} {op} ")))
            });
            if !changed {
                lines[at] = lines[at].replacen("let mut ", "let ", 1);
            }
        }
        lines
    }

    /// Keeps `node`'s Java as TODO comments, one per line.
    fn keep_java(&self, node: Node, depth: usize) -> Vec<String> {
        let pad = "    ".repeat(depth);
        self.t(node)
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(|l| format!("{pad}// TODO (Java): {l}"))
            .collect()
    }

    fn block(&mut self, block: Node, depth: usize) -> Vec<String> {
        let mut out = Vec::new();
        for s in named(block) {
            out.extend(self.statement(s, depth));
        }
        out
    }

    /// A statement or a block in a statement's place.
    fn nested(&mut self, node: Node, depth: usize) -> Vec<String> {
        if node.kind() == "block" { self.block(node, depth) } else { self.statement(node, depth) }
    }

    fn statement(&mut self, s: Node, depth: usize) -> Vec<String> {
        let pad = "    ".repeat(depth);
        match s.kind() {
            "if_statement" => {
                self.seen += 1;
                let Some(cond) = s.child_by_field_name("condition").and_then(|c| self.condition(c)) else {
                    return self.keep_java(s, depth);
                };
                self.done += 1;
                let mut out = vec![format!("{pad}if {cond} {{")];
                if let Some(then) = s.child_by_field_name("consequence") {
                    out.extend(self.nested(then, depth + 1));
                }
                if let Some(other) = s.child_by_field_name("alternative") {
                    if other.kind() == "if_statement" {
                        let mut rest = self.statement(other, depth);
                        if let Some(first) = rest.first_mut() {
                            *first = format!("{pad}}} else {}", first.trim_start());
                        }
                        out.extend(rest);
                        return out;
                    }
                    out.push(format!("{pad}}} else {{"));
                    out.extend(self.nested(other, depth + 1));
                }
                out.push(format!("{pad}}}"));
                out
            }
            "while_statement" => {
                self.seen += 1;
                let Some(cond) = s.child_by_field_name("condition").and_then(|c| self.condition(c)) else {
                    return self.keep_java(s, depth);
                };
                self.done += 1;
                let mut out = vec![format!("{pad}while {cond} {{")];
                if let Some(body) = s.child_by_field_name("body") {
                    out.extend(self.nested(body, depth + 1));
                }
                out.push(format!("{pad}}}"));
                out
            }
            "block" => {
                let mut out = vec![format!("{pad}{{")];
                out.extend(self.block(s, depth + 1));
                out.push(format!("{pad}}}"));
                out
            }
            "return_statement" => {
                self.seen += 1;
                let value = named(s).into_iter().next();
                let line = match (value.map(|v| self.t(v)), self.shape.flow, self.shape.allow) {
                    (None, false, _) => Some("return;".to_string()),
                    (None, true, _) => Some("return Flow::Continue;".to_string()),
                    (Some("true"), true, true) => Some("return Flow::Continue;".to_string()),
                    (Some("false"), true, true) => Some("return Flow::Cancel;".to_string()),
                    _ => None,
                };
                match line {
                    Some(l) => {
                        self.done += 1;
                        vec![format!("{pad}{l}")]
                    }
                    None => self.keep_java(s, depth),
                }
            }
            "local_variable_declaration" => {
                self.seen += 1;
                let mut out = Vec::new();
                for d in named(s).into_iter().filter(|d| d.kind() == "variable_declarator") {
                    let (Some(name), Some(value)) = (d.child_by_field_name("name"), d.child_by_field_name("value")) else {
                        return self.keep_java(s, depth);
                    };
                    let Some(v) = self.expr(value) else { return self.keep_java(s, depth) };
                    let java = self.t(name).to_string();
                    let rust = snake(&java);
                    let m = if self.reassigned.contains(&java) { "mut " } else { "" };
                    out.push(format!("{pad}let {m}{rust} = {v};"));
                    self.lets.push((rust.clone(), self.t(s).to_string()));
                    self.locals.insert(java, rust);
                }
                self.done += 1;
                out
            }
            "expression_statement" => {
                self.seen += 1;
                let Some(e) = named(s).into_iter().next() else { return Vec::new() };
                match self.statement_expr(e) {
                    Some(line) => {
                        self.done += 1;
                        vec![format!("{pad}{line}")]
                    }
                    None => self.keep_java(s, depth),
                }
            }
            "line_comment" | "block_comment" => Vec::new(),
            _ => {
                self.seen += 1;
                self.keep_java(s, depth)
            }
        }
    }

    /// An expression used as a statement: calls whose result Horizon gives as a `Result` are
    /// written `let _ = ...;`.
    fn statement_expr(&mut self, e: Node) -> Option<String> {
        match e.kind() {
            "method_invocation" => {
                let (rust, result) = self.call(e)?;
                Some(if result { format!("let _ = {rust};") } else { format!("{rust};") })
            }
            "assignment_expression" => {
                let left = e.child_by_field_name("left")?;
                let name = self.locals.get(self.t(left))?.clone();
                let op = e.child_by_field_name("operator").map(|o| self.t(o))?;
                if !matches!(op, "=" | "+=" | "-=" | "*=" | "/=") {
                    return None;
                }
                let right = self.expr(e.child_by_field_name("right")?)?;
                Some(format!("{name} {op} {right};"))
            }
            "update_expression" => {
                let t = self.t(e);
                let var = named(e).into_iter().find(|c| c.kind() == "identifier")?;
                let name = self.locals.get(self.t(var))?.clone();
                if t.contains("++") {
                    Some(format!("{name} += 1;"))
                } else if t.contains("--") {
                    Some(format!("{name} -= 1;"))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// An `if` or `while` condition: Java's parentheses go.
    fn condition(&mut self, c: Node) -> Option<String> {
        let inner = if c.kind() == "parenthesized_expression" { named(c).into_iter().next()? } else { c };
        self.expr(inner)
    }

    /// `client.player`, `MinecraftClient.getInstance().player`: the local player.
    fn is_player(&self, e: Node) -> bool {
        if e.kind() != "field_access" {
            return false;
        }
        let field = e.child_by_field_name("field").map(|f| self.t(f));
        field == Some("player") && e.child_by_field_name("object").is_some_and(|o| self.is_client(o))
    }

    /// The client: a listener parameter in that role, or `Minecraft.getInstance()`.
    fn is_client(&self, e: Node) -> bool {
        match e.kind() {
            "identifier" => self.roles.get(self.t(e)) == Some(&Role::Client),
            "method_invocation" => {
                let name = e.child_by_field_name("name").map(|n| self.t(n));
                let object = e.child_by_field_name("object").map(|o| self.t(o));
                name == Some("getInstance") && matches!(object, Some("Minecraft" | "MinecraftClient"))
            }
            _ => false,
        }
    }

    fn role(&self, e: Node) -> Option<Role> {
        (e.kind() == "identifier").then(|| self.roles.get(self.t(e)).copied()).flatten()
    }

    fn args(&mut self, call: Node) -> Option<Vec<String>> {
        let args = call.child_by_field_name("arguments")?;
        named(args).into_iter().map(|a| self.expr(a)).collect()
    }

    fn arg_nodes<'t>(&self, call: Node<'t>) -> Vec<Node<'t>> {
        call.child_by_field_name("arguments").map(named).unwrap_or_default()
    }

    /// A key mapping variable: `OPEN_CONFIG`, `Keys.OPEN_CONFIG`, `this.zoomKey`.
    fn key_var(&self, e: Node) -> Option<String> {
        let name = match e.kind() {
            "identifier" => self.t(e),
            "field_access" => self.t(e.child_by_field_name("field")?),
            _ => return None,
        };
        self.keys.get(name).map(|lang| (self.key_id)(lang))
    }

    /// A method call: its Rust, and whether that returns a `Result` to be let go.
    fn call(&mut self, e: Node) -> Option<(String, bool)> {
        let name = self.t(e.child_by_field_name("name")?);
        let object = e.child_by_field_name("object");
        let args = self.arg_nodes(e);

        // Key mappings: `KEY.consumeClick()` / Yarn's `KEY.wasPressed()`, `isDown()` / `isPressed()`.
        if let Some(id) = object.and_then(|o| self.key_var(o)) {
            return match (name, args.len()) {
                ("consumeClick" | "wasPressed", 0) => Some((format!("host.consume_click(ID, {id:?})"), false)),
                ("isDown" | "isPressed", 0) => Some((format!("host.is_down(ID, {id:?})"), false)),
                _ => None,
            };
        }

        // Text: `Component.literal("x")` / `Text.literal("x")`, `translatable("key")`.
        if let Some(o) = object.filter(|o| matches!(self.t(*o), "Component" | "Text")) {
            let _ = o;
            let a = self.args(e)?;
            return match (name, a.as_slice()) {
                ("literal", [s]) => Some((format!("Text::literal({s})"), false)),
                ("translatable", [k]) => Some((format!("Text::translate({k}, Vec::new())"), false)),
                ("empty", []) => Some(("Text::literal(\"\")".to_string(), false)),
                _ => None,
            };
        }

        // The local player's chat: `client.player.sendMessage(text, false)` (Yarn) or
        // `displayClientMessage(text, false)` (Mojang): a line in this client's chat.
        if object.is_some_and(|o| self.is_player(o)) && matches!(name, "sendMessage" | "displayClientMessage") {
            let [text, overlay] = args.as_slice() else { return None };
            if self.t(*overlay) != "false" {
                return None;
            }
            let text = self.expr(*text)?;
            return Some((format!("host.chat_line({text})"), true));
        }

        // Sending chat and commands: `client.player.networkHandler.sendChatMessage(s)`,
        // `client.getConnection().sendChat(s)`, `...sendCommand(s)` / `sendChatCommand(s)`.
        if matches!(name, "sendChatMessage" | "sendChat" | "sendChatCommand" | "sendCommand")
            && object.is_some_and(|o| self.is_connection(o))
        {
            let [s] = args.as_slice() else { return None };
            let s = self.expr(*s)?;
            return Some(match name {
                "sendChatMessage" | "sendChat" => (format!("host.send_chat(&{s})"), true),
                _ => (format!("host.send_chat(&format!(\"/{{}}\", {s}))"), true),
            });
        }

        // The HUD: `context.fill(x1, y1, x2, y2, color)` and `drawString` / `text` / Yarn's
        // `drawText` / `drawTextWithShadow` with a string literal.
        if object.and_then(|o| self.role(o)) == Some(Role::Draw) {
            return self.draw_call(name, &args);
        }
        None
    }

    /// `client.player.networkHandler`, `client.player.connection`, `client.getConnection()`.
    fn is_connection(&self, o: Node) -> bool {
        match o.kind() {
            "field_access" => {
                let field = o.child_by_field_name("field").map(|f| self.t(f));
                matches!(field, Some("networkHandler" | "connection"))
                    && o.child_by_field_name("object").is_some_and(|p| self.is_player(p))
            }
            "method_invocation" => {
                let name = o.child_by_field_name("name").map(|n| self.t(n));
                matches!(name, Some("getConnection" | "getNetworkHandler"))
                    && o.child_by_field_name("object").is_some_and(|c| self.is_client(c))
            }
            _ => false,
        }
    }

    fn draw_call(&mut self, name: &str, args: &[Node]) -> Option<(String, bool)> {
        let num = |t: &mut Self, n: Node| t.expr(n).map(|x| format!("({x}) as f32"));
        match (name, args.len()) {
            ("fill", 5) => {
                let [x1, y1, x2, y2] = [num(self, args[0])?, num(self, args[1])?, num(self, args[2])?, num(self, args[3])?];
                let color = self.color(args[4])?;
                Some((format!("event.draw.fill([{x1}, {y1}, {x2} - {x1}, {y2} - {y1}], {color})"), false))
            }
            ("drawString" | "text" | "drawText" | "drawTextWithShadow", 5 | 6) => {
                // The font comes first; the text must be a plain string here.
                if args[1].kind() != "string_literal" {
                    return None;
                }
                let s = self.expr(args[1])?;
                let (x, y) = (num(self, args[2])?, num(self, args[3])?);
                let color = self.color(args[4])?;
                let shadow = match (name, args.get(5)) {
                    ("drawTextWithShadow", None) => "true".to_string(),
                    (_, Some(a)) => self.expr(*a)?,
                    (_, None) => "true".to_string(),
                };
                Some((format!("event.draw.text({s}, [{x}, {y}], {color}, {shadow})"), false))
            }
            _ => None,
        }
    }

    /// A colour argument: Java's ints are signed, Horizon's ARGB colours `u32`.
    fn color(&mut self, n: Node) -> Option<String> {
        let t = self.t(n);
        if n.kind() == "hex_integer_literal" {
            return Some(t.trim_end_matches(['L', 'l']).to_string());
        }
        self.expr(n).map(|x| format!("({x}) as u32"))
    }

    fn expr(&mut self, e: Node) -> Option<String> {
        let t = self.t(e);
        match e.kind() {
            "string_literal" => (!t.contains("\\u")).then(|| t.to_string()),
            "decimal_integer_literal" => Some(t.trim_end_matches(['L', 'l']).replace('_', "")),
            // Java's int wraps `0xFFFFFFFF` round to -1; in Rust it is a u32 (as colours are).
            "hex_integer_literal" => {
                let n = t.trim_end_matches(['L', 'l']).replace('_', "");
                let big = u64::from_str_radix(n.trim_start_matches("0x").trim_start_matches("0X"), 16).is_ok_and(|v| v > i32::MAX as u64);
                Some(if big { format!("{n}u32") } else { n })
            }
            "decimal_floating_point_literal" => {
                let n = t.trim_end_matches(['f', 'F', 'd', 'D']);
                Some(if n.contains('.') || n.contains('e') || n.contains('E') { n.to_string() } else { format!("{n}.0") })
            }
            "true" | "false" => Some(t.to_string()),
            "identifier" => match self.roles.get(t) {
                Some(Role::Message) => Some("event.message".into()),
                Some(Role::Overlay) => Some("event.overlay".into()),
                Some(_) => None,
                None => self.locals.get(t).cloned(),
            },
            "parenthesized_expression" => Some(format!("({})", self.expr(named(e).into_iter().next()?)?)),
            "unary_expression" => {
                let op = self.t(e.child_by_field_name("operator")?);
                let x = self.expr(e.child_by_field_name("operand")?)?;
                matches!(op, "!" | "-").then(|| format!("{op}{x}"))
            }
            "binary_expression" => self.binary(e),
            "method_invocation" => self.call(e).filter(|(_, result)| !result).map(|(r, _)| r),
            _ => None,
        }
    }

    fn binary(&mut self, e: Node) -> Option<String> {
        let op = self.t(e.child_by_field_name("operator")?);
        let (l, r) = (e.child_by_field_name("left")?, e.child_by_field_name("right")?);
        // `client.player == null` / `!= null`.
        if matches!(op, "==" | "!=") && (self.t(r) == "null" || self.t(l) == "null") {
            let other = if self.t(r) == "null" { l } else { r };
            if self.is_player(other) {
                let test = if op == "==" { "is_none" } else { "is_some" };
                return Some(format!("host.local_player().{test}()"));
            }
            return None;
        }
        // String joining: `"FPS: " + fps`.
        if op == "+" && (l.kind() == "string_literal" || r.kind() == "string_literal") {
            let parts = [self.expr(l)?, self.expr(r)?];
            return Some(format!("format!(\"{{}}{{}}\", {}, {})", parts[0], parts[1]));
        }
        if !matches!(op, "+" | "-" | "*" | "/" | "%" | "<" | ">" | "<=" | ">=" | "==" | "!=" | "&&" | "||") {
            return None;
        }
        Some(format!("{} {op} {}", self.expr(l)?, self.expr(r)?))
    }
}
