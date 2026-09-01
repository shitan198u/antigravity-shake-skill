use chrono::Utc;
use serde_json::json;
use std::fs::File;
use std::io::Write;
use std::path::Path;

pub fn write_artifact_metadata(markdown_path: &Path, summary: &str) -> std::io::Result<()> {
    let meta_path = markdown_path.with_extension("md.metadata.json");
    let now_iso = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    
    let meta_data = json!({
        "artifactType": "ARTIFACT_TYPE_OTHER",
        "summary": summary,
        "updatedAt": now_iso
    });

    let mut file = File::create(meta_path)?;
    file.write_all(serde_json::to_string_pretty(&meta_data)?.as_bytes())?;
    Ok(())
}
