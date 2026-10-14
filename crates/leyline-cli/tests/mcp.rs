//! End to end over stdio (ADR 0154): the real binary, launched the way an
//! MCP client launches it, driven by newline-delimited JSON-RPC.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

fn cli(args: &[&str]) {
    let out = Command::new(env!("CARGO_BIN_EXE_leyline-cli"))
        .args(args)
        .output()
        .expect("the cli binary should run");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A real 4×4 PNG the engine can decode and measure.
fn sample_png(path: &Path) {
    image::save_buffer(
        path,
        &[128u8; 4 * 4 * 3],
        4,
        4,
        image::ExtendedColorType::Rgb8,
    )
    .unwrap();
}

struct Agent {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl Agent {
    fn launch(library: &str) -> Agent {
        let mut child = Command::new(env!("CARGO_BIN_EXE_leyline-cli"))
            .args(["mcp", "--library", library])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("the cli binary should run");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut agent = Agent {
            child,
            stdin,
            stdout,
            next_id: 0,
        };
        let init = agent.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "test-agent", "version": "0"},
            }),
        );
        assert_eq!(init["serverInfo"]["name"], "leyline");
        agent.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        agent
    }

    fn send(&mut self, message: Value) {
        writeln!(self.stdin, "{message}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let mut line = String::new();
            assert!(
                self.stdout.read_line(&mut line).unwrap() > 0,
                "server hung up"
            );
            let message: Value = serde_json::from_str(&line).expect("stdout is only JSON-RPC");
            if message["id"] == id {
                assert!(message["error"].is_null(), "{message}");
                return message["result"].clone();
            }
        }
    }

    /// Calls a tool; returns its content and whether it was a refusal.
    fn call(&mut self, tool: &str, arguments: Value) -> (Value, bool) {
        let result = self.request("tools/call", json!({"name": tool, "arguments": arguments}));
        let refused = result["isError"].as_bool().unwrap_or(false);
        (result["content"][0].clone(), refused)
    }

    fn call_json(&mut self, tool: &str, arguments: Value) -> Value {
        let (content, refused) = self.call(tool, arguments);
        assert!(!refused, "{tool} refused: {content}");
        serde_json::from_str(content["text"].as_str().unwrap()).unwrap()
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn an_agent_imports_edits_previews_and_exports() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Lib");
    let root = root.to_str().unwrap();
    cli(&["new", root]);
    let shoot = dir.path().join("shoot");
    std::fs::create_dir(&shoot).unwrap();
    sample_png(&shoot.join("photo.png"));

    let mut agent = Agent::launch(root);

    let tools = agent.request("tools/list", json!({}));
    let names: Vec<&str> = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"apply_edits"), "{names:?}");
    assert!(
        !names.iter().any(|name| name.contains("delete")),
        "{names:?}"
    );

    // References only unless asked; a folder outside every root cannot be
    // referenced, and the refusal says what to do instead.
    let source = shoot.to_str().unwrap();
    let referenced = agent.call_json("import", json!({"source": source}));
    assert!(referenced["imported"].as_array().unwrap().is_empty());
    let reason = referenced["skipped"][0]["reason"].as_str().unwrap();
    assert!(reason.contains("import by copy"), "{reason}");
    let copied = agent.call_json("import", json!({"source": source, "copy_files": true}));
    assert_eq!(copied["imported"].as_array().unwrap().len(), 1, "{copied}");
    assert!(shoot.join("photo.png").exists());

    let hits = agent.call_json("search", json!({}));
    let version = hits[0]["version_id"].as_i64().unwrap();
    let asset = hits[0]["asset_id"].as_i64().unwrap();
    assert!(
        agent
            .call_json("get_metadata", json!({"asset_id": asset}))
            .is_object()
    );

    let applied = agent.call_json(
        "apply_edits",
        json!({"version_id": version, "settings": {"exposure": 0.5}}),
    );
    // A first edit also pins the stage versions, so `stages` may change too.
    let changed = |value: &Value| {
        value["changed"]
            .as_array()
            .unwrap()
            .contains(&json!("exposure"))
    };
    assert!(changed(&applied), "{applied}");
    let settings = agent.call_json("get_settings", json!({"version_id": version}));
    assert_eq!(settings["exposure"], 0.5);
    let history = agent.call_json("history", json!({"version_id": version}));
    assert_eq!(history[0]["revision_id"], applied["revision_id"]);
    assert!(changed(&history[0]), "{history}");

    // A key the settings do not have is refused by name, and writes nothing.
    let (refusal, refused) = agent.call(
        "apply_edits",
        json!({"version_id": version, "settings": {"brightness": 1}}),
    );
    assert!(refused);
    assert!(refusal["text"].as_str().unwrap().contains("`brightness`"));
    let history = agent.call_json("history", json!({"version_id": version}));
    assert_eq!(history[0]["revision_id"], applied["revision_id"]);

    let (image, refused) = agent.call("render_preview", json!({"version_id": version}));
    assert!(!refused, "{image}");
    assert_eq!(image["type"], "image");
    assert_eq!(image["mimeType"], "image/jpeg");

    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();
    let exported = agent.call_json(
        "export",
        json!({"version_ids": [version], "destination_dir": out.to_str().unwrap()}),
    );
    let path = exported["exported"][0]["path"].as_str().unwrap();
    assert!(Path::new(path).exists(), "{exported}");
}
