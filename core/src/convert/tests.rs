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
