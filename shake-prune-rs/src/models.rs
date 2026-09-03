use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct CompactionEvent {
    pub timestamp_iso: String,
    pub timestamp_display: String,
    pub trigger: String, // "Manual (/shake)", "Manual (/full-shake)", or "Auto (80k Threshold)"
    pub anchored_step: u64,
    pub bytes_before: usize,
    pub bytes_after: usize,
    pub reduction_pct: f64,
    pub backup_file: String,
    pub artifact_file: String,
}

#[derive(Serialize, Debug, Clone)]
pub struct PruningStats {
    pub conv_id: String,
    pub raw_bytes: usize,
    pub pruned_bytes: usize,
    pub raw_tokens: usize,
    pub pruned_tokens: usize,
    pub reduction_pct: f64,
    pub this_run_before_bytes: usize,
    pub this_run_after_bytes: usize,
    pub this_run_savings_pct: f64,
    pub cumulative_full_bytes: usize,
    pub cumulative_savings_pct: f64,
    pub user_turns: usize,
    pub assistant_turns: usize,
    pub pruned_tools: usize,
    pub retained_errors: usize,
    pub retained_short_cmds: usize,
    pub retained_recent_steps: usize,
    pub topic_slug: String,
    pub suggested_filename: String,
    pub history_events: Vec<CompactionEvent>,
}
