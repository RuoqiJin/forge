//! Strategy Distillation — pattern extraction from cascade repair history.
//!
//! Analyzes `RepairRecord` entries to discover recurring failure patterns
//! and generates actionable briefing updates for the Mechanic agent.
//! This is P5 of the flywheel: the self-evolution feedback loop.

use crate::repair_log::RepairRecord;
use std::collections::HashMap;

// ============================================================================
// Data types
// ============================================================================

/// A recurring repair pattern detected from historical records.
#[derive(Debug, Clone)]
pub struct RepairPattern {
    /// Human-readable description of the pattern.
    pub description: String,
    /// Number of times this pattern appeared in the history.
    pub frequency: usize,
    /// Services involved in this pattern.
    pub services: Vec<String>,
    /// Common trigger interface changes associated with this pattern.
    pub trigger_changes: Vec<String>,
    /// Average number of repair attempts across occurrences.
    pub avg_repair_attempts: f64,
    /// Suggested briefing text to add to Mechanic's knowledge base.
    pub suggested_briefing: String,

    // --- P6 前置数据聚合字段 ---

    /// 收集到的 error signatures（从 compiler_output 提取关键行）
    pub error_signatures: Vec<String>,
    /// 成功修复涉及的文件模式
    pub affected_file_patterns: Vec<String>,
}

/// Complete distillation report produced from analyzing repair history.
#[derive(Debug)]
pub struct DistillationReport {
    /// Number of records analyzed.
    pub total_records: usize,
    /// Detected high-frequency patterns.
    pub patterns: Vec<RepairPattern>,
    /// Aggregate statistics.
    pub stats: DistillationStats,
}

/// Aggregate statistics across all analyzed records.
#[derive(Debug)]
pub struct DistillationStats {
    /// Total number of cascade executions analyzed.
    pub total_cascades: usize,
    /// Total number of phases across all cascades.
    pub total_phases: usize,
    /// Fraction of phases that passed (0.0 - 1.0).
    pub success_rate: f64,
    /// Average repair attempts per phase.
    pub avg_attempts_per_phase: f64,
    /// Number of cascades that triggered a hard halt.
    pub hard_halt_count: usize,
    /// Service with the most failures (if any).
    pub most_failed_service: Option<String>,
}

// ============================================================================
// Clustering key
// ============================================================================

/// Internal grouping key for pattern detection.
/// Phases with the same (service_id, trigger_changes) are clustered together.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
struct ClusterKey {
    service_id: String,
    /// Sorted, comma-joined trigger changes for deterministic hashing.
    trigger_changes_key: String,
}

/// Accumulated data for a single cluster.
#[derive(Debug)]
struct ClusterData {
    service_id: String,
    trigger_changes: Vec<String>,
    total_attempts: usize,
    occurrences: usize,
    error_messages: Vec<String>,
    /// Collected compiler error signatures (from P6 compiler_output fields).
    compiler_outputs: Vec<String>,
    /// Collected modified file paths (from P6 files_modified fields).
    modified_files: Vec<String>,
}

// ============================================================================
// Distillation engine
// ============================================================================

/// Run strategy distillation on a set of repair records.
///
/// Steps:
/// 1. Compute aggregate statistics (success rate, avg attempts, etc.)
/// 2. Cluster failed/hard_halt phases by (service_id, trigger_changes)
/// 3. For clusters with >= 2 occurrences, generate `RepairPattern`
/// 4. Sort patterns by frequency (descending)
pub fn distill(records: &[RepairRecord]) -> DistillationReport {
    if records.is_empty() {
        return DistillationReport {
            total_records: 0,
            patterns: Vec::new(),
            stats: DistillationStats {
                total_cascades: 0,
                total_phases: 0,
                success_rate: 0.0,
                avg_attempts_per_phase: 0.0,
                hard_halt_count: 0,
                most_failed_service: None,
            },
        };
    }

    // --- Step 1: Aggregate statistics ---
    let total_cascades = records.len();
    let mut total_phases: usize = 0;
    let mut passed_phases: usize = 0;
    let mut total_attempts: usize = 0;
    let mut hard_halt_count: usize = 0;
    let mut failure_counts: HashMap<String, usize> = HashMap::new();

    for record in records {
        if record.hard_halted {
            hard_halt_count += 1;
        }
        for phase in &record.phases {
            total_phases += 1;
            total_attempts += phase.repair_attempts;
            if phase.status == "passed" {
                passed_phases += 1;
            }
            if phase.status == "failed" || phase.status == "hard_halt" {
                *failure_counts
                    .entry(phase.service_id.clone())
                    .or_insert(0) += 1;
            }
        }
    }

    let success_rate = if total_phases > 0 {
        passed_phases as f64 / total_phases as f64
    } else {
        0.0
    };

    let avg_attempts_per_phase = if total_phases > 0 {
        total_attempts as f64 / total_phases as f64
    } else {
        0.0
    };

    let most_failed_service = failure_counts
        .iter()
        .max_by_key(|(_, count)| *count)
        .map(|(svc, _)| svc.clone());

    let stats = DistillationStats {
        total_cascades,
        total_phases,
        success_rate,
        avg_attempts_per_phase,
        hard_halt_count,
        most_failed_service,
    };

    // --- Step 2: Cluster failed/hard_halt phases ---
    let mut clusters: HashMap<ClusterKey, ClusterData> = HashMap::new();

    for record in records {
        let mut sorted_triggers = record.trigger_changes.clone();
        sorted_triggers.sort();
        let trigger_key = sorted_triggers.join(",");

        for phase in &record.phases {
            if phase.status != "failed" && phase.status != "hard_halt" {
                continue;
            }

            let key = ClusterKey {
                service_id: phase.service_id.clone(),
                trigger_changes_key: trigger_key.clone(),
            };

            let entry = clusters.entry(key).or_insert_with(|| ClusterData {
                service_id: phase.service_id.clone(),
                trigger_changes: record.trigger_changes.clone(),
                total_attempts: 0,
                occurrences: 0,
                error_messages: Vec::new(),
                compiler_outputs: Vec::new(),
                modified_files: Vec::new(),
            });

            entry.occurrences += 1;
            entry.total_attempts += phase.repair_attempts;
            if let Some(msg) = &phase.error_message {
                if !entry.error_messages.contains(msg) {
                    entry.error_messages.push(msg.clone());
                }
            }

            // Collect P6 compiler_output error signatures
            if let Some(ref output) = phase.compiler_output {
                let sigs = extract_error_signatures(output);
                for sig in sigs {
                    if !entry.compiler_outputs.contains(&sig) {
                        entry.compiler_outputs.push(sig);
                    }
                }
            }

            // Collect P6 files_modified
            for f in &phase.files_modified {
                if !entry.modified_files.contains(f) {
                    entry.modified_files.push(f.clone());
                }
            }
        }
    }

    // --- Step 3: Generate patterns for clusters with >= 2 occurrences ---
    let mut patterns: Vec<RepairPattern> = clusters
        .into_values()
        .filter(|c| c.occurrences >= 2)
        .map(|c| {
            let avg = if c.occurrences > 0 {
                c.total_attempts as f64 / c.occurrences as f64
            } else {
                0.0
            };

            let trigger_desc = if c.trigger_changes.is_empty() {
                "unknown changes".to_string()
            } else {
                c.trigger_changes.join(", ")
            };

            let description = format!(
                "Service '{}' fails repeatedly when triggered by [{}] ({} occurrences)",
                c.service_id, trigger_desc, c.occurrences
            );

            let error_hint = if c.error_messages.is_empty() {
                String::new()
            } else {
                format!(
                    " Common errors: {}.",
                    c.error_messages
                        .iter()
                        .take(3)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("; ")
                )
            };

            let suggested_briefing = format!(
                "When [{}] interface changes occur, service '{}' frequently fails \
                 (avg {:.1} repair attempts).{} \
                 Mechanic should prioritize checking type imports and interface \
                 compatibility before attempting broader fixes.",
                trigger_desc, c.service_id, avg, error_hint
            );

            RepairPattern {
                description,
                frequency: c.occurrences,
                services: vec![c.service_id],
                trigger_changes: c.trigger_changes,
                avg_repair_attempts: avg,
                suggested_briefing,
                error_signatures: c.compiler_outputs,
                affected_file_patterns: c.modified_files,
            }
        })
        .collect();

    // --- Step 4: Sort by frequency (highest first) ---
    patterns.sort_by(|a, b| b.frequency.cmp(&a.frequency));

    DistillationReport {
        total_records: records.len(),
        patterns,
        stats,
    }
}

// ============================================================================
// Error signature extraction (P6)
// ============================================================================

/// Extract key error signatures from compiler stderr output.
///
/// Looks for lines matching `error[Exxxx]:` patterns from rustc, as well as
/// common cargo/clippy diagnostic patterns. Returns deduplicated signatures.
fn extract_error_signatures(compiler_output: &str) -> Vec<String> {
    let mut sigs = Vec::new();
    for line in compiler_output.lines() {
        let trimmed = line.trim();
        // Match rustc error codes: error[E0308], error[E0433], etc.
        if trimmed.starts_with("error[E") || trimmed.starts_with("error:") {
            let sig = trimmed.to_string();
            if !sigs.contains(&sig) {
                sigs.push(sig);
            }
        }
    }
    sigs
}

// ============================================================================
// Briefing generation
// ============================================================================

/// Generate a Markdown briefing update from detected patterns.
///
/// This text is intended to be appended to the Mechanic's briefing template,
/// giving it learned knowledge about recurring failure modes.
pub fn generate_briefing_update(patterns: &[RepairPattern]) -> String {
    if patterns.is_empty() {
        return String::from(
            "## Strategy Distillation Update (auto-generated)\n\n\
             No recurring failure patterns detected. Current repair strategies are effective.\n",
        );
    }

    let mut output = String::from("## Strategy Distillation Update (auto-generated)\n\n");

    for (i, pattern) in patterns.iter().enumerate() {
        output.push_str(&format!("### Pattern {}: {}\n\n", i + 1, pattern.description));
        output.push_str(&format!(
            "- **Trigger**: {}\n",
            if pattern.trigger_changes.is_empty() {
                "unknown".to_string()
            } else {
                pattern.trigger_changes.join(", ")
            }
        ));
        output.push_str(&format!(
            "- **Services**: {}\n",
            pattern.services.join(", ")
        ));
        output.push_str(&format!(
            "- **Frequency**: {} occurrences\n",
            pattern.frequency
        ));
        output.push_str(&format!(
            "- **Avg repair attempts**: {:.1}\n",
            pattern.avg_repair_attempts
        ));
        output.push_str(&format!(
            "- **Suggested strategy**: {}\n",
            pattern.suggested_briefing
        ));
        output.push('\n');
    }

    output
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repair_log::{PhaseRecord, RepairRecord};

    fn make_record(
        trigger: &str,
        trigger_changes: &[&str],
        phases: Vec<PhaseRecord>,
        hard_halted: bool,
    ) -> RepairRecord {
        RepairRecord {
            trigger_service: trigger.into(),
            trigger_changes: trigger_changes.iter().map(|s| s.to_string()).collect(),
            timestamp: "2025-01-01T00:00:00Z".into(),
            phases,
            hard_halted,
            total_duration_secs: 10.0,
        }
    }

    fn make_phase(
        service: &str,
        depth: usize,
        status: &str,
        attempts: usize,
        error: Option<&str>,
    ) -> PhaseRecord {
        PhaseRecord {
            service_id: service.into(),
            depth,
            status: status.into(),
            repair_attempts: attempts,
            duration_secs: 5.0,
            error_message: error.map(|s| s.into()),
            compiler_output: None,
            files_modified: vec![],
            repair_diff: None,
        }
    }

    #[test]
    fn test_distill_empty() {
        let report = distill(&[]);
        assert_eq!(report.total_records, 0);
        assert!(report.patterns.is_empty());
        assert_eq!(report.stats.total_cascades, 0);
    }

    #[test]
    fn test_distill_detects_pattern() {
        let records = vec![
            // Record 1: consumer fails on type-signature-change
            make_record(
                "provider",
                &["type-signature-change"],
                vec![
                    make_phase("gateway", 1, "passed", 0, None),
                    make_phase("consumer", 2, "failed", 3, Some("type mismatch")),
                ],
                false,
            ),
            // Record 2: consumer fails again on same trigger
            make_record(
                "provider",
                &["type-signature-change"],
                vec![
                    make_phase("gateway", 1, "passed", 0, None),
                    make_phase("consumer", 2, "failed", 2, Some("type mismatch")),
                ],
                false,
            ),
            // Record 3: different trigger, consumer passes
            make_record(
                "other-svc",
                &["api-removal"],
                vec![make_phase("consumer", 1, "passed", 0, None)],
                false,
            ),
        ];

        let report = distill(&records);

        assert_eq!(report.total_records, 3);
        assert_eq!(report.stats.total_cascades, 3);
        assert_eq!(report.stats.total_phases, 5);
        assert_eq!(report.stats.hard_halt_count, 0);

        // Should detect 1 pattern: consumer + type-signature-change
        assert_eq!(report.patterns.len(), 1);
        let pattern = &report.patterns[0];
        assert_eq!(pattern.frequency, 2);
        assert!(pattern.services.contains(&"consumer".to_string()));
        assert!(pattern
            .trigger_changes
            .contains(&"type-signature-change".to_string()));
        assert!((pattern.avg_repair_attempts - 2.5).abs() < 0.01);
    }

    #[test]
    fn test_distill_hard_halt_counted() {
        let records = vec![
            make_record(
                "provider",
                &["change"],
                vec![make_phase("svc-a", 1, "hard_halt", 3, Some("max cycles"))],
                true,
            ),
            make_record(
                "provider",
                &["change"],
                vec![make_phase("svc-a", 1, "hard_halt", 3, Some("max cycles"))],
                true,
            ),
        ];

        let report = distill(&records);
        assert_eq!(report.stats.hard_halt_count, 2);
        assert_eq!(report.patterns.len(), 1);
        assert_eq!(report.patterns[0].frequency, 2);
    }

    #[test]
    fn test_distill_no_pattern_below_threshold() {
        let records = vec![
            // Only 1 failure -- below threshold of 2
            make_record(
                "provider",
                &["change"],
                vec![make_phase("svc-a", 1, "failed", 2, None)],
                false,
            ),
        ];

        let report = distill(&records);
        assert!(report.patterns.is_empty());
    }

    #[test]
    fn test_distill_most_failed_service() {
        let records = vec![
            make_record(
                "p",
                &["c"],
                vec![
                    make_phase("svc-a", 1, "failed", 1, None),
                    make_phase("svc-b", 2, "failed", 1, None),
                ],
                false,
            ),
            make_record(
                "p",
                &["c"],
                vec![
                    make_phase("svc-a", 1, "failed", 1, None),
                    make_phase("svc-b", 2, "passed", 0, None),
                ],
                false,
            ),
        ];

        let report = distill(&records);
        assert_eq!(report.stats.most_failed_service, Some("svc-a".to_string()));
    }

    #[test]
    fn test_generate_briefing_empty() {
        let output = generate_briefing_update(&[]);
        assert!(output.contains("No recurring failure patterns"));
    }

    #[test]
    fn test_generate_briefing_nonempty() {
        let patterns = vec![RepairPattern {
            description: "consumer fails on type change".into(),
            frequency: 5,
            services: vec!["consumer".into()],
            trigger_changes: vec!["type-signature-change".into()],
            avg_repair_attempts: 2.5,
            suggested_briefing: "Check type imports first.".into(),
            error_signatures: vec![],
            affected_file_patterns: vec![],
        }];

        let output = generate_briefing_update(&patterns);
        assert!(output.contains("## Strategy Distillation Update"));
        assert!(output.contains("Pattern 1"));
        assert!(output.contains("consumer"));
        assert!(output.contains("type-signature-change"));
        assert!(output.contains("5 occurrences"));
        assert!(!output.is_empty());
    }

    #[test]
    fn test_distill_success_rate() {
        let records = vec![make_record(
            "p",
            &["c"],
            vec![
                make_phase("a", 1, "passed", 0, None),
                make_phase("b", 1, "passed", 0, None),
                make_phase("c", 2, "failed", 3, None),
                make_phase("d", 2, "passed", 1, None),
            ],
            false,
        )];

        let report = distill(&records);
        assert_eq!(report.stats.total_phases, 4);
        assert!((report.stats.success_rate - 0.75).abs() < 0.01);
    }
}
