use serde::{Deserialize, Serialize};

#[derive(Deserialize, Debug, Clone)]
pub struct ToolCall {
    pub name: String,
    pub args: Option<serde_json::Value>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Step {
    #[serde(rename = "type")]
    pub step_type: Option<String>,
    #[allow(dead_code)]
    pub source: Option<String>,
    pub content: Option<String>,
    pub status: Option<String>,
    pub exit_code: Option<i32>,
    pub tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Serialize, Debug, Clone)]
pub struct PruningStats {
    pub conv_id: String,
    pub raw_bytes: usize,
    pub pruned_bytes: usize,
    pub raw_tokens: usize,
    pub pruned_tokens: usize,
    pub reduction_pct: f64,
    pub user_turns: usize,
    pub assistant_turns: usize,
    pub pruned_tools: usize,
    pub retained_errors: usize,
    pub retained_short_cmds: usize,
    pub retained_recent_steps: usize,
    pub topic_slug: String,
    pub suggested_filename: String,
}
