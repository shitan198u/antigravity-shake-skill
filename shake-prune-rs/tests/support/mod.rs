#![allow(dead_code)]
use serde_json::{json, Value};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_shake-prune"))
}

pub fn run_shake(args: &[&str]) -> std::process::Output {
    std::process::Command::new(bin())
        .args(args)
        .output()
        .expect("failed to run shake-prune binary")
}

pub fn read_jsonl(path: &Path) -> Vec<Value> {
    let content = fs::read_to_string(path).unwrap();
    content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("invalid JSON line"))
        .collect()
}

pub fn assert_valid_jsonl(path: &Path) {
    let content = fs::read_to_string(path).expect("failed to read transcript file");
    assert!(!content.trim().is_empty(), "JSONL file must not be empty");
    for (i, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        serde_json::from_str::<Value>(line)
            .unwrap_or_else(|e| panic!("Line {} is invalid JSON ({}): '{}'", i + 1, e, line));
    }
}

pub fn fake_home(tmp: &tempfile::TempDir) -> PathBuf {
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).unwrap();
    home
}

pub fn trusted_transcript_path(home: &Path, conv: &str) -> PathBuf {
    home.join(".gemini")
        .join("antigravity-ide")
        .join("brain")
        .join(conv)
        .join(".system_generated")
        .join("logs")
        .join("transcript.jsonl")
}

pub struct TranscriptBuilder {
    lines: Vec<Value>,
    step: u64,
}

impl TranscriptBuilder {
    pub fn new() -> Self {
        Self {
            lines: Vec::new(),
            step: 1,
        }
    }

    pub fn user(mut self, content: &str) -> Self {
        self.lines.push(json!({
            "step_index": self.step,
            "type": "USER_INPUT",
            "source": "USER_EXPLICIT",
            "content": format!("<USER_REQUEST>{}</USER_REQUEST>", content)
        }));
        self.step += 1;
        self
    }

    pub fn assistant(mut self, content: &str) -> Self {
        self.lines.push(json!({
            "step_index": self.step,
            "type": "PLANNER_RESPONSE",
            "content": content
        }));
        self.step += 1;
        self
    }

    pub fn assistant_with_thinking(mut self, content: &str, thinking: &str) -> Self {
        self.lines.push(json!({
            "step_index": self.step,
            "type": "PLANNER_RESPONSE",
            "content": content,
            "thinking": thinking
        }));
        self.step += 1;
        self
    }

    pub fn tool_output(mut self, tool_type: &str, content: &str, exit_code: i64) -> Self {
        self.lines.push(json!({
            "step_index": self.step,
            "type": tool_type,
            "status": if exit_code == 0 { "DONE" } else { "ERROR" },
            "exit_code": exit_code,
            "content": content
        }));
        self.step += 1;
        self
    }

    pub fn tool_output_with_status(
        mut self,
        tool_type: &str,
        content: &str,
        status: &str,
        exit_code: Option<i64>,
    ) -> Self {
        let mut obj = json!({
            "step_index": self.step,
            "type": tool_type,
            "status": status,
            "content": content
        });
        if let Some(code) = exit_code {
            obj["exit_code"] = json!(code);
        }
        self.lines.push(obj);
        self.step += 1;
        self
    }

    pub fn ephemeral(mut self, content: &str) -> Self {
        self.lines.push(json!({
            "step_index": self.step,
            "type": "EPHEMERAL_MESSAGE",
            "content": content
        }));
        self.step += 1;
        self
    }

    pub fn raw_json(mut self, value: Value) -> Self {
        self.lines.push(value);
        self.step += 1;
        self
    }

    pub fn write(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut f = File::create(path).unwrap();
        for line in &self.lines {
            writeln!(f, "{}", serde_json::to_string(line).unwrap()).unwrap();
        }
    }
}
