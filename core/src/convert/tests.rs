use super::*;

fn key_id(lang: &str) -> String {
    crate::generate::key_id(lang)
}

fn run(java: &str) -> Converted {
    let files = [JavaFile { path: "Test.java".into(), text: java.into() }];
    convert(&Library::default(), &files, &key_id)
}

#[test]
fn key_loop_in_a_tick() {
    let c = run(r#"
import net.fabricmc.fabric.api.client.event.lifecycle.v1.ClientTickEvents;
import net.minecraft.client.KeyMapping;
import org.lwjgl.glfw.GLFW;

public class Keys {
    public static final KeyMapping OPEN_CONFIG = new KeyMapping("key.showfps.open_config", InputConstants.Type.KEYSYM, GLFW.GLFW_KEY_N, KeyMapping.Category.MISC);

    public static void register() {
        ClientTickEvents.END_CLIENT_TICK.register(client -> {
            while (OPEN_CONFIG.consumeClick()) {
                client.gui.setScreen(new ConfigScreen(null));
            }
        });
    }
}"#);
    assert_eq!(c.key_defaults["key.showfps.open_config"], "key.keyboard.n");
    let lines = &c.hooks["client_tick_post"][0].lines;
    assert_eq!(lines[0], "while host.consume_click(ID, \"open_config\") {");
    assert_eq!(lines[1], "    // TODO (Java): client.gui.setScreen(new ConfigScreen(null));");
    assert_eq!(lines[2], "}");
    assert_eq!((c.statements, c.converted), (2, 1));
}

#[test]
fn chat_and_null_checks() {
    let c = run(r#"
import net.fabricmc.fabric.api.client.event.lifecycle.v1.ClientTickEvents;
class A {
    void init() {
        ClientTickEvents.END_CLIENT_TICK.register(this::tick);
    }
    private void tick(MinecraftClient client) {
        if (client.player == null) return;
        int count = 0;
        count++;
        client.player.sendMessage(Text.literal("Count: " + count), false);
        client.player.networkHandler.sendChatCommand("spawn");
    }
}"#);
    let lines = &c.hooks["client_tick_post"][0].lines;
    assert_eq!(lines, &[
        "if host.local_player().is_none() {",
        "    return;",
        "}",
        "let mut count = 0;",
        "count += 1;",
        "let _ = host.chat_line(Text::literal(format!(\"{}{}\", \"Count: \", count)));",
        "let _ = host.send_chat(&format!(\"/{}\", \"spawn\"));",
    ]);
    assert_eq!(c.statements, c.converted);
}

#[test]
fn hud_drawing_and_messages() {
    let c = run(r#"
import net.fabricmc.fabric.api.client.rendering.v1.HudRenderCallback;
import net.fabricmc.fabric.api.client.message.v1.ClientReceiveMessageEvents;
class B {
    void init() {
        HudRenderCallback.EVENT.register((context, tickCounter) -> {
            context.fill(2, 2, 50, 12, 0x80000000);
            context.drawString(font, "Hello", 4, 4, 0xFFFFFFFF, true);
        });
        ClientReceiveMessageEvents.ALLOW_GAME.register((message, overlay) -> {
            if (overlay) return false;
            return true;
        });
    }
}"#);
    let hud = &c.hooks["render_gui_post"][0].lines;
    assert_eq!(hud[0], "event.draw.fill([(2) as f32, (2) as f32, (50) as f32 - (2) as f32, (12) as f32 - (2) as f32], 0x80000000);");
    assert_eq!(hud[1], "event.draw.text(\"Hello\", [(4) as f32, (4) as f32], 0xFFFFFFFF, true);");
    let chat = &c.hooks["client_chat_received"][0].lines;
    assert_eq!(chat, &["if event.overlay {", "    return Flow::Cancel;", "}", "return Flow::Continue;"]);
}

#[test]
fn glfw_names() {
    assert_eq!(glfw_key_name("GLFW.GLFW_KEY_LEFT_SHIFT").as_deref(), Some("key.keyboard.left.shift"));
    assert_eq!(glfw_key_name("GLFW_KEY_KP_1").as_deref(), Some("key.keyboard.keypad.1"));
    assert_eq!(glfw_key_name("GLFW_KEY_F3").as_deref(), Some("key.keyboard.f3"));
    assert_eq!(glfw_key_name("x"), None);
}

#[test]
fn hud_elements_by_layer() {
    let c = run(r#"
import net.fabricmc.fabric.api.client.rendering.v1.hud.HudElementRegistry;
import net.fabricmc.fabric.api.client.rendering.v1.hud.VanillaHudElements;
class C {
    void init() {
        HudElementRegistry.attachElementBefore(VanillaHudElements.CHAT, Identifier.fromNamespaceAndPath("x", "y"), this::draw);
        HudElementRegistry.addLast(Identifier.fromNamespaceAndPath("x", "z"), (graphics, delta) -> graphics.fill(0, 0, 4, 4, 0xFF000000));
    }
    private void draw(GuiGraphicsExtractor graphics, DeltaTracker deltaTracker) {
        if (!enabled) return;
        graphics.fill(1, 1, 2, 2, 0xFFFFFFFF);
    }
}"#);
    let pre = &c.hooks["gui_layer_pre"][0].lines;
    assert_eq!(pre[0], "if event.layer.ends_with(\"chat\") {");
    assert_eq!(pre[1], "    // TODO (Java): if (!enabled) return;");
    assert!(pre[2].starts_with("    event.draw.fill("));
    assert_eq!(pre[3], "}");
    assert!(c.hooks["render_gui_post"][0].lines[0].starts_with("event.draw.fill("));
}

#[test]
fn unused_variables_stay_java() {
    let c = run(r#"
import net.fabricmc.fabric.api.client.event.lifecycle.v1.ClientTickEvents;
class D {
    void init() {
        ClientTickEvents.END_CLIENT_TICK.register(client -> {
            int color = 0xFFFFFFFF;
            if (client.options.foo) color = 0xFF00FF00;
            int small = 0x10;
            client.player.sendMessage(Text.literal("x" + small), false);
        });
    }
}"#);
    let lines = &c.hooks["client_tick_post"][0].lines;
    assert_eq!(lines[0], "// TODO (Java): int color = 0xFFFFFFFF;");
    assert_eq!(lines[1], "// TODO (Java): if (client.options.foo) color = 0xFF00FF00;");
    assert_eq!(lines[2], "let small = 0x10;");
    assert_eq!(c.converted, 2);
}

#[test]
fn client_commands() {
    let c = run(r#"
import net.fabricmc.fabric.api.client.command.v2.ClientCommandRegistrationCallback;
import net.fabricmc.fabric.api.client.command.v2.ClientCommands;
class E {
    void init() {
        ClientCommandRegistrationCallback.EVENT.register((dispatcher, registryAccess) -> {
            dispatcher.register(ClientCommands.literal("hello")
                .executes(context -> {
                    context.getSource().sendFeedback(Component.literal("Hi!"));
                    return 1;
                })
                .then(ClientCommands.literal("times")
                    .then(ClientCommands.argument("count", IntegerArgumentType.integer())
                        .executes(context -> {
                            int count = IntegerArgumentType.getInteger(context, "count");
                            context.getSource().sendFeedback(Component.literal("x" + count));
                            return 1;
                        }))));
            dispatcher.register(ClientCommands.literal("say").then(ClientCommands.argument("message", StringArgumentType.greedyString())
                .executes(ctx -> { ctx.getSource().sendFeedback(Component.literal(StringArgumentType.getString(ctx, "message"))); return 1; })));
        });
    }
}"#);
    assert_eq!(c.hooks["register_client_commands"][0].lines, &[
        "event.commands.push(Command::new(\"hello\"));",
        "event.commands.push(Command::new(\"hello times <count:integer>\"));",
        "event.commands.push(Command::new(\"say <message:text>\"));",
    ]);
    let run = c.hooks["command"][0].lines.join("\n");
    assert!(run.contains("if let [\"hello\"] = event.command.split_whitespace().collect::<Vec<_>>()[..] {\n    let _ = host.chat_line(Text::literal(\"Hi!\"));\n    return Flow::Cancel;\n}"), "{run}");
    assert!(run.contains("if let [\"hello\", \"times\", count] = "), "{run}");
    assert!(run.contains("    if let Ok(count) = count.parse::<i64>() {\n        let _ = host.chat_line("), "{run}");
    assert!(run.contains("if let Some(message) = event.command.strip_prefix(\"say \") {\n    let _ = host.chat_line(Text::literal(message));"), "{run}");
    assert_eq!(c.statements, c.converted);
}

#[test]
fn join_and_leave() {
    let c = run(r#"
import net.fabricmc.fabric.api.client.networking.v1.ClientPlayConnectionEvents;
class F {
    void init() {
        ClientPlayConnectionEvents.JOIN.register((handler, sender, client) -> client.player.sendMessage(Text.literal("Welcome"), false));
        ClientPlayConnectionEvents.DISCONNECT.register((handler, client) -> { return; });
    }
}"#);
    assert_eq!(c.hooks["client_player_logging_in"][0].lines, &["let _ = host.chat_line(Text::literal(\"Welcome\"));"]);
    assert_eq!(c.hooks["client_player_logging_out"][0].lines, &["return;"]);
}

#[test]
fn more_client_rules() {
    let c = run(r#"
import net.fabricmc.fabric.api.client.event.lifecycle.v1.ClientTickEvents;
import net.minecraft.client.KeyMapping;
class G {
    static KeyMapping MENU = new KeyMapping("key.g.menu", InputConstants.Type.KEYSYM, GLFW.GLFW_KEY_M, KeyMapping.Category.MISC);
    void init() {
        ClientTickEvents.END_CLIENT_TICK.register(c -> {
            Minecraft mc = Minecraft.getInstance();
            while (MENU != null && MENU.consumeClick()) {
                if (mc.gui.screen() == null) {
                    mc.player.sendSystemMessage(Component.literal("Menu").withStyle(ChatFormatting.GOLD));
                }
            }
            mc.execute(() -> {
                long now = System.currentTimeMillis();
                if (Objects.equals(now, 0)) return;
            });
        });
    }
}"#);
    let lines = c.hooks["client_tick_post"][0].lines.join("\n");
    assert!(lines.starts_with("while host.consume_click(ID, \"menu\") {\n    if host.playing() {"), "{lines}");
    assert!(lines.contains("host.chat_line(Text::literal(\"Menu\").with_style({ let mut s = mod_api::text::TextStyle::default(); s.color = Some(\"gold\".into()); s }))"), "{lines}");
    assert!(lines.contains("{\n    let now = std::time::SystemTime::now()"), "{lines}");
    assert!(lines.contains("    if now == 0 {\n        return;"), "{lines}");
    assert_eq!(c.statements, c.converted, "{lines}");
}
