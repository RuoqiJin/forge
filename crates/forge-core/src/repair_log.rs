//! Repair Log — structured persistence of cascade repair outcomes.
//!
//! After each cascade execution, a `RepairRecord` is generated from the
//! `CascadeReport` and appended to a JSONL file (`repair_history.jsonl`).
//! This history feeds the strategy distillation engine (P5 flywheel).

use crate::cascade::{CascadeReport, PhaseStatus};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ============================================================================
// Data types
// ============================================================================

/// A single repair record written after a cascade completes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairRecord {
    /// The service whose intent change triggered the cascade.
    pub trigger_service: String,
    /// Human-readable descriptions of the triggering interface changes.
    pub trigger_changes: Vec<String>,
    /// ISO 8601 timestamp of when the record was created.
    pub timestamp: String,
    /// Per-phase results.
    pub phases: Vec<PhaseRecord>,
    /// Whether the cascade triggered a hard halt (circuit breaker).
    pub hard_halted: bool,
    /// Total wall-clock duration of the cascade in seconds.
    pub total_duration_secs: f64,
}

/// Result of a single cascade phase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseRecord {
    /// Service identifier.
    pub service_id: String,
    /// Topological depth from the trigger service.
    pub depth: usize,
    /// Outcome: "passed", "failed", "hard_halt", or "skipped".
    pub status: String,
    /// Number of mechanic repair attempts made.
    pub repair_attempts: usize,
    /// Wall-clock duration of this phase in seconds.
    pub duration_secs: f64,
    /// Error message (if status is "failed" or "hard_halt").
    pub error_message: Option<String>,

    // --- P6 前置数据字段 (向后兼容) ---

    /// cargo check/clippy 的 stderr 输出（截断最后 2048 字符）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compiler_output: Option<String>,

    /// 修复后变更的文件列表
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files_modified: Vec<String>,

    /// 成功修复的 git diff 摘要（截断最后 4096 字符）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repair_diff: Option<String>,
}

// ============================================================================
// Conversion
// ============================================================================

/// Format the current system time as an ISO 8601 string (UTC).
///
/// Uses `std::time::SystemTime` without pulling in `chrono`.
fn now_iso8601() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();

    // Manual UTC decomposition (no leap-second handling, good enough for logs)
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Days since 1970-01-01 -> year/month/day
    let (year, month, day) = days_to_ymd(days);

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

/// Convert days since 1970-01-01 to (year, month, day).
fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Algorithm adapted from Howard Hinnant's civil_from_days
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Generate a `RepairRecord` from a completed `CascadeReport`.
pub fn record_from_report(report: &CascadeReport) -> RepairRecord {
    let phases: Vec<PhaseRecord> = report
        .plan
        .phases
        .iter()
        .map(|phase| {
            let (status_str, error_msg) = match &phase.status {
                PhaseStatus::Passed => ("passed".to_string(), None),
                PhaseStatus::Failed(msg) => ("failed".to_string(), Some(msg.clone())),
                PhaseStatus::HardHalt(msg) => ("hard_halt".to_string(), Some(msg.clone())),
                PhaseStatus::Skipped => ("skipped".to_string(), None),
                PhaseStatus::Pending => ("pending".to_string(), None),
                PhaseStatus::Running => ("running".to_string(), None),
            };
            let duration_secs = phase
                .duration
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0);

            PhaseRecord {
                service_id: phase.service_id.clone(),
                depth: phase.depth,
                status: status_str,
                repair_attempts: phase.repair_attempts,
                duration_secs,
                error_message: error_msg,
                compiler_output: phase.last_compiler_output.clone(),
                files_modified: phase.files_modified.clone(),
                repair_diff: phase.repair_diff.clone(),
            }
        })
        .collect();

    RepairRecord {
        trigger_service: report.plan.trigger_service.clone(),
        trigger_changes: report.plan.trigger_changes.clone(),
        timestamp: now_iso8601(),
        phases,
        hard_halted: report.hard_halted,
        total_duration_secs: report.total_duration.as_secs_f64(),
    }
}

// ============================================================================
// Persistence (JSONL)
// ============================================================================

const LOG_FILENAME: &str = "repair_history.jsonl";

/// Append a repair record to the JSONL log file.
///
/// Creates `log_dir` if it doesn't exist. Each record is serialized as a
/// single JSON line and appended to `log_dir/repair_history.jsonl`.
pub fn save_repair_record(record: &RepairRecord, log_dir: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(log_dir)
        .map_err(|e| format!("failed to create log dir {}: {}", log_dir.display(), e))?;

    let log_path = log_dir.join(LOG_FILENAME);
    let json_line = serde_json::to_string(record)
        .map_err(|e| format!("failed to serialize repair record: {}", e))?;

    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|e| format!("failed to open {}: {}", log_path.display(), e))?;

    writeln!(file, "{}", json_line)
        .map_err(|e| format!("failed to write to {}: {}", log_path.display(), e))?;

    Ok(log_path)
}

/// Load all repair records from the JSONL log file.
///
/// Lines that fail to parse are silently skipped (fault tolerance for
/// partially corrupted or evolving schema).
pub fn load_repair_history(log_dir: &Path) -> Result<Vec<RepairRecord>, String> {
    let log_path = log_dir.join(LOG_FILENAME);

    if !log_path.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&log_path)
        .map_err(|e| format!("failed to read {}: {}", log_path.display(), e))?;

    let records: Vec<RepairRecord> = content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();

    Ok(records)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_now_iso8601_format() {
        let ts = now_iso8601();
        // Should match YYYY-MM-DDTHH:MM:SSZ
        assert!(ts.len() == 20, "timestamp should be 20 chars: {}", ts);
        assert!(ts.ends_with('Z'));
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[7..8], "-");
        assert_eq!(&ts[10..11], "T");
    }

    #[test]
    fn test_days_to_ymd_epoch() {
        // 1970-01-01
        let (y, m, d) = days_to_ymd(0);
        assert_eq!((y, m, d), (1970, 1, 1));
    }

    #[test]
    fn test_days_to_ymd_known_date() {
        // 2024-01-01 = day 19723
        let (y, m, d) = days_to_ymd(19723);
        assert_eq!((y, m, d), (2024, 1, 1));
    }

    #[test]
    fn test_save_and_load_round_trip() {
        let dir = std::env::temp_dir().join("forge-repair-log-test");
        let _ = std::fs::remove_dir_all(&dir);

        let record = RepairRecord {
            trigger_service: "svc-auth".into(),
            trigger_changes: vec!["type-signature-change".into()],
            timestamp: "2025-01-01T00:00:00Z".into(),
            phases: vec![
                PhaseRecord {
                    service_id: "svc-gateway".into(),
                    depth: 1,
                    status: "passed".into(),
                    repair_attempts: 1,
                    duration_secs: 5.2,
                    error_message: None,
                    compiler_output: None,
                    files_modified: vec![],
                    repair_diff: None,
                },
                PhaseRecord {
                    service_id: "svc-consumer".into(),
                    depth: 2,
                    status: "failed".into(),
                    repair_attempts: 3,
                    duration_secs: 30.0,
                    error_message: Some("type mismatch".into()),
                    compiler_output: Some("error[E0308]: mismatched types".into()),
                    files_modified: vec!["src/lib.rs".into()],
                    repair_diff: None,
                },
            ],
            hard_halted: false,
            total_duration_secs: 35.2,
        };

        // Save
        let path = save_repair_record(&record, &dir).expect("save should succeed");
        assert!(path.exists());

        // Load
        let loaded = load_repair_history(&dir).expect("load should succeed");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].trigger_service, "svc-auth");
        assert_eq!(loaded[0].phases.len(), 2);
        assert_eq!(loaded[0].phases[1].status, "failed");

        // Append another record
        let record2 = RepairRecord {
            trigger_service: "svc-api".into(),
            trigger_changes: vec!["api-removal".into()],
            timestamp: "2025-01-02T00:00:00Z".into(),
            phases: vec![],
            hard_halted: true,
            total_duration_secs: 1.0,
        };
        save_repair_record(&record2, &dir).expect("second save should succeed");

        let loaded = load_repair_history(&dir).expect("load should succeed");
        assert_eq!(loaded.len(), 2);

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_load_empty_dir() {
        let dir = std::env::temp_dir().join("forge-repair-log-empty");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);

        let loaded = load_repair_history(&dir).expect("load should succeed");
        assert!(loaded.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_load_nonexistent_dir() {
        let dir = std::env::temp_dir().join("forge-repair-log-nonexistent");
        let _ = std::fs::remove_dir_all(&dir);

        let loaded = load_repair_history(&dir).expect("load should succeed");
        assert!(loaded.is_empty());
    }

    #[test]
    fn test_backward_compat_old_jsonl() {
        // Simulate an old-format JSONL line without the new P6 fields.
        // serde(default) should fill in None/empty values.
        let old_json = r#"{"trigger_service":"svc-old","trigger_changes":[],"timestamp":"2025-01-01T00:00:00Z","phases":[{"service_id":"svc-a","depth":1,"status":"passed","repair_attempts":0,"duration_secs":1.0,"error_message":null}],"hard_halted":false,"total_duration_secs":1.0}"#;
        let record: RepairRecord = serde_json::from_str(old_json).expect("old JSONL should parse");
        assert_eq!(record.phases[0].compiler_output, None);
        assert!(record.phases[0].files_modified.is_empty());
        assert_eq!(record.phases[0].repair_diff, None);
    }

    #[test]
    fn test_new_fields_roundtrip() {
        let record = RepairRecord {
            trigger_service: "svc-new".into(),
            trigger_changes: vec![],
            timestamp: "2025-06-01T00:00:00Z".into(),
            phases: vec![PhaseRecord {
                service_id: "svc-a".into(),
                depth: 1,
                status: "failed".into(),
                repair_attempts: 2,
                duration_secs: 5.0,
                error_message: Some("type error".into()),
                compiler_output: Some("error[E0308]: mismatched types".into()),
                files_modified: vec!["src/lib.rs".into(), "src/main.rs".into()],
                repair_diff: Some("diff --git a/src/lib.rs b/src/lib.rs\n...".into()),
            }],
            hard_halted: false,
            total_duration_secs: 5.0,
        };

        let json = serde_json::to_string(&record).unwrap();
        let loaded: RepairRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.phases[0].compiler_output.as_deref(), Some("error[E0308]: mismatched types"));
        assert_eq!(loaded.phases[0].files_modified.len(), 2);
        assert!(loaded.phases[0].repair_diff.is_some());
    }

    #[test]
    fn test_load_tolerates_bad_lines() {
        let dir = std::env::temp_dir().join("forge-repair-log-bad");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let log_path = dir.join(LOG_FILENAME);
        let good_record = RepairRecord {
            trigger_service: "svc-x".into(),
            trigger_changes: vec![],
            timestamp: "2025-01-01T00:00:00Z".into(),
            phases: vec![],
            hard_halted: false,
            total_duration_secs: 1.0,
        };
        let good_json = serde_json::to_string(&good_record).unwrap();

        // Write: good line, bad line, good line
        std::fs::write(
            &log_path,
            format!("{}\n{{bad json}}\n{}\n", good_json, good_json),
        )
        .unwrap();

        let loaded = load_repair_history(&dir).expect("load should succeed");
        assert_eq!(loaded.len(), 2); // bad line skipped

        let _ = std::fs::remove_dir_all(&dir);
    }
}
