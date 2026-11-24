//! The converter: reads a mod's Java source and turns the code inside its Fabric event
//! listeners into Rust for the matching Horizon hooks, statement by statement.
//!
//! It is rule-based and small on purpose: a statement is converted only when every part of it
//! has a rule; anything else is kept as a `// TODO (Java):` comment with the original line, so
//! nothing unconverted looks finished. Key mappings declared with `new KeyMapping(...)` also give
//! their default keys to the generator.

mod java;

pub use crate::generate::{Block, Converted};
use crate::library::Library;
use java::Translator;
use std::collections::BTreeMap;
use tree_sitter::{Node, Parser};

/// A Java source file: its path in the repository, and its text.
pub struct JavaFile {
    pub path: String,
    pub text: String,
}

/// What a listener's parameters are, for the hooks the converter has rules for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// `MinecraftClient` / `Minecraft`: reached through the host.
    Client,
    /// The HUD's draw context: `event.draw`.
    Draw,
    /// A chat message (`event.message`) and whether it is for the action bar (`event.overlay`).
    Message,
    Overlay,
    /// Something the hook has no equivalent for (a tick counter): any use stays Java.
    Other,
}

/// How a hook's listener is converted: its parameters, and what its `return`s become.
#[derive(Clone, Copy)]
pub struct HookShape {
    pub hook: &'static str,
    pub params: &'static [Role],
    /// The hook returns `Flow`.
    pub flow: bool,
    /// The Fabric listener returns a boolean that allows (true) or cancels (false).
    pub allow: bool,
}

/// The Fabric events the converter knows, by `Class#FIELD` (the class's simple name).
fn shape(class: &str, field: &str) -> Option<HookShape> {
    use Role::*;
    let s = |hook, params, flow, allow| Some(HookShape { hook, params, flow, allow });
    match (class, field) {
        ("ClientTickEvents", "END_CLIENT_TICK") => s("client_tick_post", &[Client], false, false),
        ("ClientTickEvents", "START_CLIENT_TICK") => s("client_tick_pre", &[Client], false, false),
        ("HudRenderCallback", "EVENT") => s("render_gui_post", &[Draw, Other], false, false),
        ("ClientReceiveMessageEvents", "GAME") => s("client_chat_received", &[Message, Overlay], true, false),
        ("ClientReceiveMessageEvents", "ALLOW_GAME") => s("client_chat_received", &[Message, Overlay], true, true),
        _ => None,
    }
}

/// Fabric's HUD element registry: `addLast(id, element)`, `attachElementBefore(layer, id, element)`.
const HUD_REGISTRY: &str = "net.fabricmc.fabric.api.client.rendering.v1.hud.HudElementRegistry";

/// A `VanillaHudElements` constant's Horizon HUD layer (`event.layer` in `gui_layer_pre`/`_post`).
fn hud_layer(fabric: &str) -> Option<&'static str> {
    Some(match fabric {
        "CROSSHAIR" => "crosshair",
        "HOTBAR" => "hotbar",
        "HEALTH_BAR" | "ARMOR_BAR" | "FOOD_BAR" | "AIR_BAR" => "player_health",
        "MOUNT_HEALTH" => "vehicle_health",
        "INFO_BAR" | "EXPERIENCE_LEVEL" | "INFO_BAR_FOREGROUND" => "contextual_info_bar",
        "HELD_ITEM_TOOLTIP" => "selected_item_name",
        "MOB_EFFECTS" => "effects",
        "BOSS_BAR" => "boss_overlay",
        "SLEEP" => "sleep_overlay",
        "SCOREBOARD" => "scoreboard_sidebar",
        "OVERLAY_MESSAGE" => "overlay_message",
        "TITLE_AND_SUBTITLE" => "title",
        "CHAT" => "chat",
        "PLAYER_LIST" => "tab_list",
        "SUBTITLES" => "subtitle_overlay",
        _ => return None,
    })
}

/// A `HudElementRegistry` call: the hook its element draws in, the layer it is tied to, and the
/// element (its last argument).
fn hud_element<'t>(call: Node<'t>, name: &str, src: &str) -> Option<(HookShape, Option<&'static str>, Node<'t>)> {
    use Role::*;
    let args = call.child_by_field_name("arguments")?;
    let mut c = args.walk();
    let args: Vec<Node> = args.named_children(&mut c).collect();
    let element = *args.last()?;
    let layer_of = |n: Option<&Node>| n.and_then(|n| hud_layer(text(*n, src).rsplit('.').next()?));
    let shape = |hook, flow| HookShape { hook, params: &[Draw, Other], flow, allow: false };
    Some(match name {
        "addLast" => (shape("render_gui_post", false), None, element),
        "addFirst" => (shape("gui_layer_pre", true), Some("crosshair"), element),
        "attachElementBefore" => (shape("gui_layer_pre", true), Some(layer_of(args.first())?), element),
        "attachElementAfter" => (shape("gui_layer_post", false), Some(layer_of(args.first())?), element),
        _ => return None,
    })
}

/// The Fabric event classes the converter knows, as their packages name them.
const EVENT_PACKAGES: &[&str] = &[
    "net.fabricmc.fabric.api.client.event.lifecycle.v1.ClientTickEvents",
    "net.fabricmc.fabric.api.client.rendering.v1.HudRenderCallback",
    "net.fabricmc.fabric.api.client.message.v1.ClientReceiveMessageEvents",
];

/// `GLFW.GLFW_KEY_LEFT_SHIFT` → `key.keyboard.left.shift`, as Controls names keys.
pub fn glfw_key_name(constant: &str) -> Option<String> {
    let name = constant.rsplit('.').next()?.strip_prefix("GLFW_KEY_")?;
    if name == "UNKNOWN" {
        return Some("key.keyboard.unknown".into());
    }
    let name = name.to_ascii_lowercase();
    let name = match name.strip_prefix("kp_") {
        Some(rest) => format!("keypad.{rest}"),
        None => name,
    };
    Some(format!("key.keyboard.{}", name.replace('_', ".")))
}

fn parser() -> Parser {
    let mut p = Parser::new();
    p.set_language(&tree_sitter_java::LANGUAGE.into()).expect("the Java grammar loads");
    p
}

fn text<'a>(node: Node, src: &'a str) -> &'a str {
    node.utf8_text(src.as_bytes()).unwrap_or("")
}

fn walk<'t>(node: Node<'t>, out: &mut Vec<Node<'t>>) {
    out.push(node);
    let mut c = node.walk();
    for child in node.children(&mut c) {
        walk(child, out);
    }
}

/// The simple names a file imports, to their full names.
fn imports(root: Node, src: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut c = root.walk();
    for node in root.children(&mut c) {
        if node.kind() == "import_declaration" {
            let full = text(node, src).trim_start_matches("import").trim().trim_end_matches(';').trim();
            if let Some(simple) = full.rsplit('.').next().filter(|s| *s != "*" && !full.starts_with("static")) {
                out.insert(simple.to_string(), full.to_string());
            }
        }
    }
    out
}

/// Key mappings made with `new KeyMapping("key...", ..., GLFW.GLFW_KEY_X, ...)` (or Yarn's
/// `KeyBinding`): the variable or field it is kept in → its lang key, and lang key → default key.
fn key_mappings(files: &[(String, &str, tree_sitter::Tree)]) -> (BTreeMap<String, String>, BTreeMap<String, String>) {
    let mut vars = BTreeMap::new();
    let mut defaults = BTreeMap::new();
    for (_, src, tree) in files {
        let mut nodes = Vec::new();
        walk(tree.root_node(), &mut nodes);
        for node in nodes {
            if node.kind() != "object_creation_expression" {
                continue;
            }
            let ty = node.child_by_field_name("type").map(|t| text(t, src)).unwrap_or("");
            if !matches!(ty.rsplit('.').next(), Some("KeyMapping" | "KeyBinding")) {
                continue;
            }
            let Some(args) = node.child_by_field_name("arguments") else { continue };
            let mut c = args.walk();
            let args: Vec<Node> = args.named_children(&mut c).collect();
            let Some(lang) = args.first().filter(|a| a.kind() == "string_literal").map(|a| text(*a, src).trim_matches('"').to_string())
            else {
                continue;
            };
            if let Some(key) = args.iter().find_map(|a| glfw_key_name(text(*a, src))) {
                defaults.insert(lang.clone(), key);
            }
            // The variable it is kept in: `NAME = new KeyMapping(...)`, maybe inside a registration call.
            let mut up = node.parent();
            while let Some(p) = up {
                match p.kind() {
                    "variable_declarator" => {
                        if let Some(n) = p.child_by_field_name("name") {
                            vars.insert(text(n, src).to_string(), lang.clone());
                        }
                        break;
                    }
                    "assignment_expression" => {
                        if let Some(l) = p.child_by_field_name("left") {
                            let name = text(l, src).rsplit('.').next().unwrap_or("").to_string();
                            vars.insert(name, lang.clone());
                        }
                        break;
                    }
                    "block" | "class_body" => break,
                    _ => up = p.parent(),
                }
            }
        }
    }
    (vars, defaults)
}

/// Converts what it can of `files`. `key_id` turns a key mapping's lang key into the id the
/// generator registers it under.
pub fn convert(_library: &Library, files: &[JavaFile], key_id: &dyn Fn(&str) -> String) -> Converted {
    let mut parser = parser();
    let parsed: Vec<(String, &str, tree_sitter::Tree)> = files
        .iter()
        .filter_map(|f| Some((f.path.clone(), f.text.as_str(), parser.parse(&f.text, None)?)))
        .collect();
    let (key_vars, key_defaults) = key_mappings(&parsed);
    let mut out = Converted { key_defaults, ..Converted::default() };

    for (path, src, tree) in &parsed {
        let root = tree.root_node();
        let imported = imports(root, src);
        let mut nodes = Vec::new();
        walk(root, &mut nodes);
        for call in nodes.iter().filter(|n| n.kind() == "method_invocation") {
            let Some(name) = call.child_by_field_name("name").map(|n| text(n, src)) else { continue };
            let Some(object) = call.child_by_field_name("object") else { continue };
            let (shape, layer, listener) = if name == "register" && object.kind() == "field_access" {
                // `ClientTickEvents.END_CLIENT_TICK.register(listener)`
                let (Some(class), Some(field)) = (object.child_by_field_name("object"), object.child_by_field_name("field")) else {
                    continue;
                };
                let class = text(class, src);
                let full = imported.get(class).map(String::as_str).unwrap_or(class);
                if !EVENT_PACKAGES.contains(&full) {
                    continue;
                }
                let Some(shape) = shape(class, text(field, src)) else { continue };
                let Some(args) = call.child_by_field_name("arguments") else { continue };
                let mut c = args.walk();
                let Some(listener) = args.named_children(&mut c).next() else { continue };
                (shape, None, listener)
            } else {
                // `HudElementRegistry.attachElementBefore(VanillaHudElements.CHAT, id, element)`
                let class = text(object, src);
                if imported.get(class).map(String::as_str) != Some(HUD_REGISTRY) {
                    continue;
                }
                let Some(found) = hud_element(*call, name, src) else { continue };
                found
            };
            let Some((params, body)) = listener_body(listener, root, src) else { continue };
            let roles: BTreeMap<String, Role> =
                params.iter().zip(shape.params.iter()).map(|(p, r)| (p.clone(), *r)).collect();
            let mut t = Translator::new(src, roles, &key_vars, key_id, shape);
            let mut lines = t.body(body);
            if let Some(layer) = layer {
                // Only around that layer: the hook fires for each.
                lines = std::iter::once(format!("if event.layer.ends_with(\"{layer}\") {{"))
                    .chain(lines.into_iter().map(|l| format!("    {l}")))
                    .chain(std::iter::once("}".to_string()))
                    .collect();
            }
            out.statements += t.seen;
            out.converted += t.done;
            out.hooks.entry(shape.hook.to_string()).or_default().push(Block { file: path.clone(), lines });
        }
    }
    out
}

/// A listener's parameter names and body: a lambda's, or the method a method reference names.
fn listener_body<'t>(listener: Node<'t>, root: Node<'t>, src: &str) -> Option<(Vec<String>, Node<'t>)> {
    match listener.kind() {
        "lambda_expression" => {
            let params = listener.child_by_field_name("parameters")?;
            // `client -> ...`, `(context, counter) -> ...`, or typed `(MinecraftClient client) -> ...`.
            let names = match params.kind() {
                "identifier" => vec![text(params, src).to_string()],
                _ => {
                    let mut c = params.walk();
                    params
                        .named_children(&mut c)
                        .filter_map(|p| match p.kind() {
                            "identifier" => Some(text(p, src).to_string()),
                            "formal_parameter" => p.child_by_field_name("name").map(|n| text(n, src).to_string()),
                            _ => None,
                        })
                        .collect()
                }
            };
            Some((names, listener.child_by_field_name("body")?))
        }
        "method_reference" => {
            // `this::onTick`, `MyMod::onTick`: the method of that name in this file.
            let name = text(listener, src).rsplit("::").next()?.trim().to_string();
            let mut nodes = Vec::new();
            walk(root, &mut nodes);
            let method = nodes.into_iter().find(|n| {
                n.kind() == "method_declaration" && n.child_by_field_name("name").map(|x| text(x, src)) == Some(name.as_str())
            })?;
            let mut names = Vec::new();
            if let Some(ps) = method.child_by_field_name("parameters") {
                let mut c = ps.walk();
                for p in ps.named_children(&mut c) {
                    if let Some(n) = p.child_by_field_name("name") {
                        names.push(text(n, src).to_string());
                    }
                }
            }
            Some((names, method.child_by_field_name("body")?))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
