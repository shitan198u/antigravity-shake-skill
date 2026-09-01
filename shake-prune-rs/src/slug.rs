use chrono::Local;
use regex::Regex;
use std::collections::HashSet;
use std::sync::OnceLock;

static RE_XML: OnceLock<Regex> = OnceLock::new();
static RE_URL: OnceLock<Regex> = OnceLock::new();
static RE_NON_ALNUM: OnceLock<Regex> = OnceLock::new();
static RE_CONV_ID: OnceLock<Regex> = OnceLock::new();

pub fn generate_topic_slug(first_user_text: &str) -> String {
    let re_xml = RE_XML.get_or_init(|| Regex::new(r"<[^>]+>").unwrap());
    let clean = re_xml.replace_all(first_user_text, " ");

    let re_url = RE_URL.get_or_init(|| Regex::new(r"https?://\S+").unwrap());
    let clean = re_url.replace_all(&clean, " ");

    let re_non_alnum = RE_NON_ALNUM.get_or_init(|| Regex::new(r"[^a-zA-Z0-9\s]").unwrap());
    let clean = re_non_alnum.replace_all(&clean, " ");

    let stop_words: HashSet<&'static str> = [
        "please", "want", "also", "this", "that", "with", "from", "have", "need",
        "make", "check", "the", "and", "for", "you", "are", "how", "what", "why"
    ].into_iter().collect();

    let words: Vec<String> = clean
        .split_whitespace()
        .map(|w| w.to_lowercase())
        .filter(|w| w.len() > 2 && !stop_words.contains(w.as_str()))
        .take(4)
        .collect();

    if words.is_empty() {
        "session".to_string()
    } else {
        words.join("_")
    }
}

pub fn generate_suggested_filename(topic_slug: &str) -> String {
    let timestamp = Local::now().format("%Y%m%d_%H%M").to_string();
    format!("shake_{}_{}.md", topic_slug, timestamp)
}

pub fn extract_conversation_id(path_str: &str) -> String {
    let re = RE_CONV_ID.get_or_init(|| Regex::new(r"brain[/\\]([a-zA-Z0-9_-]+)[/\\]").unwrap());
    if let Some(caps) = re.captures(path_str) {
        caps.get(1).map(|m| m.as_str().to_string()).unwrap_or_else(|| "unknown-session".to_string())
    } else {
        "unknown-session".to_string()
    }
}
