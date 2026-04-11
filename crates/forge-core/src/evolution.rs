//! Evolution Engine — STOP-inspired meta-utility evaluation for Forge mold proposals.
//!
//! Analyzes high-frequency repair patterns from P5 strategy distillation,
//! evaluates whether a new Forge pattern could eliminate the need for
//! Mechanic repairs, and generates mold proposals.
//!
//! This is P6 of the flywheel: mold fission — the system learns which
//! code-generation patterns to create next based on recurring repair failures.

use crate::repair_log::RepairRecord;
use crate::strategy::RepairPattern;

// ============================================================================
// Data types
// ============================================================================

/// A mold fission proposal — suggests a new Forge pattern that could
/// prevent a class of recurring Mechanic repairs.
#[derive(Debug, Clone)]
pub struct MoldProposal {
    /// Proposal title.
    pub title: String,
    /// Problem description: what recurring failure this addresses.
    pub problem: String,
    /// Proposed solution: what kind of pattern to generate.
    pub proposed_solution: String,
    /// Frequency of the source pattern.
    pub source_frequency: usize,
    /// Meta-utility score (0.0-1.0): fraction of historical failures
    /// that this mold could have prevented.
    pub estimated_utility: f64,
    /// Services affected by this pattern.
    pub affected_services: Vec<String>,
    /// Indices into the original records array that support this proposal.
    pub evidence_records: Vec<usize>,
    /// Priority derived from utility score.
    pub priority: ProposalPriority,
    /// Error signatures aggregated from P6 data (if available).
    pub error_signatures: Vec<String>,
    /// Affected file patterns aggregated from P6 data (if available).
    pub affected_file_patterns: Vec<String>,
}

/// Priority classification for a mold proposal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProposalPriority {
    /// utility >= 0.6, strongly recommended for implementation.
    High,
    /// utility >= 0.3.
    Medium,
    /// utility < 0.3.
    Low,
}

impl ProposalPriority {
    /// Human-readable label.
    pub fn label(&self) -> &'static str {
        match self {
            ProposalPriority::High => "HIGH",
            ProposalPriority::Medium => "MEDIUM",
            ProposalPriority::Low => "LOW",
        }
    }
}

/// Evolution report produced by analyzing patterns and repair history.
#[derive(Debug)]
pub struct EvolutionReport {
    /// Number of input patterns analyzed.
    pub patterns_analyzed: usize,
    /// Generated proposals, sorted by utility (descending).
    pub proposals: Vec<MoldProposal>,
    /// Human-readable summary.
    pub summary: String,
}

// ============================================================================
// Proposal generation
// ============================================================================

/// Generate mold fission proposals from distilled repair patterns.
///
/// For each pattern, computes a meta-utility score: the fraction of
/// total failed/hard_halt phases that this pattern accounts for.
/// Higher utility means a new Forge mold would eliminate more manual repairs.
pub fn generate_proposals(
    patterns: &[RepairPattern],
    records: &[RepairRecord],
) -> EvolutionReport {
    // Count total failed/hard_halt phases across all records.
    let total_failed_phases: usize = records
        .iter()
        .flat_map(|r| r.phases.iter())
        .filter(|p| p.status == "failed" || p.status == "hard_halt")
        .count();

    let mut proposals = Vec::new();

    for pattern in patterns {
        // Meta-utility: this pattern's frequency / total failed phases
        let utility = if total_failed_phases > 0 {
            pattern.frequency as f64 / total_failed_phases as f64
        } else {
            0.0
        };

        let priority = if utility >= 0.6 {
            ProposalPriority::High
        } else if utility >= 0.3 {
            ProposalPriority::Medium
        } else {
            ProposalPriority::Low
        };

        let proposal = MoldProposal {
            title: format!(
                "Auto-generate {} adapter for {} changes",
                pattern.services.join("/"),
                pattern.trigger_changes.join(", ")
            ),
            problem: format!(
                "Service(s) [{}] fail {} times (avg {:.1} attempts) when [{}] changes occur. {}",
                pattern.services.join(", "),
                pattern.frequency,
                pattern.avg_repair_attempts,
                pattern.trigger_changes.join(", "),
                pattern.description
            ),
            proposed_solution: generate_solution_description(pattern),
            source_frequency: pattern.frequency,
            estimated_utility: utility,
            affected_services: pattern.services.clone(),
            evidence_records: find_evidence_indices(pattern, records),
            priority,
            error_signatures: pattern.error_signatures.clone(),
            affected_file_patterns: pattern.affected_file_patterns.clone(),
        };

        proposals.push(proposal);
    }

    // Sort by utility descending
    proposals.sort_by(|a, b| {
        b.estimated_utility
            .partial_cmp(&a.estimated_utility)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let summary = generate_evolution_summary(&proposals);

    EvolutionReport {
        patterns_analyzed: patterns.len(),
        proposals,
        summary,
    }
}

// ============================================================================
// Internal helpers
// ============================================================================

fn generate_solution_description(pattern: &RepairPattern) -> String {
    let mut desc = format!(
        "Create a new Forge pattern that auto-generates adapter code for {} \
         when {} interfaces change. \
         This would eliminate ~{:.0} manual Mechanic repairs per occurrence.",
        pattern.services.join("/"),
        pattern.trigger_changes.join("/"),
        pattern.avg_repair_attempts,
    );

    // Enrich with P6 data if available
    if !pattern.error_signatures.is_empty() {
        desc.push_str(&format!(
            " Known error signatures: {}.",
            pattern
                .error_signatures
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }
    if !pattern.affected_file_patterns.is_empty() {
        desc.push_str(&format!(
            " Typically modifies: {}.",
            pattern
                .affected_file_patterns
                .iter()
                .take(5)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    desc
}

fn find_evidence_indices(pattern: &RepairPattern, records: &[RepairRecord]) -> Vec<usize> {
    records
        .iter()
        .enumerate()
        .filter(|(_, r)| {
            r.phases.iter().any(|p| {
                pattern.services.contains(&p.service_id)
                    && (p.status == "failed" || p.status == "hard_halt")
            })
        })
        .map(|(i, _)| i)
        .collect()
}

fn generate_evolution_summary(proposals: &[MoldProposal]) -> String {
    let high = proposals
        .iter()
        .filter(|p| p.priority == ProposalPriority::High)
        .count();
    let medium = proposals
        .iter()
        .filter(|p| p.priority == ProposalPriority::Medium)
        .count();
    let low = proposals
        .iter()
        .filter(|p| p.priority == ProposalPriority::Low)
        .count();

    if proposals.is_empty() {
        "No mold proposals generated. The system is operating efficiently.".into()
    } else {
        format!(
            "{} proposal(s) generated: {} high priority, {} medium, {} low. \
             High-priority proposals should be reviewed for implementation in jarvis-forge.",
            proposals.len(),
            high,
            medium,
            low
        )
    }
}

/// Format an evolution report as Markdown.
pub fn format_report_markdown(report: &EvolutionReport) -> String {
    let mut out = String::new();

    out.push_str("# Forge Evolution Report (P6 Mold Fission)\n\n");
    out.push_str(&format!(
        "**Patterns analyzed:** {}  \n",
        report.patterns_analyzed
    ));
    out.push_str(&format!(
        "**Proposals generated:** {}  \n\n",
        report.proposals.len()
    ));
    out.push_str(&format!("{}\n\n", report.summary));

    for (i, p) in report.proposals.iter().enumerate() {
        out.push_str(&format!(
            "## {}. {} [{}]\n\n",
            i + 1,
            p.title,
            p.priority.label()
        ));
        out.push_str(&format!(
            "- **Utility:** {:.2}  \n",
            p.estimated_utility
        ));
        out.push_str(&format!(
            "- **Frequency:** {}  \n",
            p.source_frequency
        ));
        out.push_str(&format!(
            "- **Services:** {}  \n",
            p.affected_services.join(", ")
        ));
        out.push_str(&format!(
            "- **Evidence records:** {}  \n\n",
            p.evidence_records.len()
        ));
        out.push_str(&format!("**Problem:** {}  \n\n", p.problem));
        out.push_str(&format!("**Solution:** {}  \n\n", p.proposed_solution));

        if !p.error_signatures.is_empty() {
            out.push_str("**Error signatures:**\n");
            for sig in &p.error_signatures {
                out.push_str(&format!("- `{}`\n", sig));
            }
            out.push('\n');
        }

        if !p.affected_file_patterns.is_empty() {
            out.push_str("**Affected files:**\n");
            for f in &p.affected_file_patterns {
                out.push_str(&format!("- `{}`\n", f));
            }
            out.push('\n');
        }

        out.push_str("---\n\n");
    }

    out
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repair_log::{PhaseRecord, RepairRecord};
    use crate::strategy::RepairPattern;

    fn make_record(
        trigger: &str,
        phases: Vec<PhaseRecord>,
        hard_halted: bool,
    ) -> RepairRecord {
        RepairRecord {
            trigger_service: trigger.into(),
            trigger_changes: vec!["change".into()],
            timestamp: "2025-01-01T00:00:00Z".into(),
            phases,
            hard_halted,
            total_duration_secs: 10.0,
        }
    }

    fn make_phase(service: &str, status: &str, attempts: usize) -> PhaseRecord {
        PhaseRecord {
            service_id: service.into(),
            depth: 1,
            status: status.into(),
            repair_attempts: attempts,
            duration_secs: 5.0,
            error_message: None,
            compiler_output: None,
            files_modified: vec![],
            repair_diff: None,
        }
    }

    fn make_pattern(
        services: &[&str],
        triggers: &[&str],
        frequency: usize,
        avg_attempts: f64,
    ) -> RepairPattern {
        RepairPattern {
            description: format!("test pattern freq={}", frequency),
            frequency,
            services: services.iter().map(|s| s.to_string()).collect(),
            trigger_changes: triggers.iter().map(|s| s.to_string()).collect(),
            avg_repair_attempts: avg_attempts,
            suggested_briefing: "test briefing".into(),
            error_signatures: vec![],
            affected_file_patterns: vec![],
        }
    }

    #[test]
    fn test_empty_patterns_empty_proposals() {
        let report = generate_proposals(&[], &[]);
        assert_eq!(report.patterns_analyzed, 0);
        assert!(report.proposals.is_empty());
        assert!(report.summary.contains("No mold proposals"));
    }

    #[test]
    fn test_high_frequency_pattern_high_priority() {
        // 4 total failed phases, pattern accounts for 3 => utility = 0.75 => High
        let records = vec![
            make_record("p", vec![make_phase("svc-a", "failed", 2)], false),
            make_record("p", vec![make_phase("svc-a", "failed", 3)], false),
            make_record("p", vec![make_phase("svc-a", "failed", 1)], false),
            make_record("p", vec![make_phase("svc-b", "failed", 1)], false),
        ];

        let pattern = make_pattern(&["svc-a"], &["change"], 3, 2.0);
        let report = generate_proposals(&[pattern], &records);

        assert_eq!(report.proposals.len(), 1);
        assert_eq!(report.proposals[0].priority, ProposalPriority::High);
        assert!(report.proposals[0].estimated_utility >= 0.6);
    }

    #[test]
    fn test_low_frequency_pattern_low_priority() {
        // 10 total failed phases, pattern accounts for 1 => utility = 0.1 => Low
        let phases: Vec<PhaseRecord> = (0..10)
            .map(|i| make_phase(&format!("svc-{}", i), "failed", 1))
            .collect();
        let records = vec![make_record("p", phases, false)];

        let pattern = make_pattern(&["svc-0"], &["change"], 1, 1.0);
        let report = generate_proposals(&[pattern], &records);

        assert_eq!(report.proposals.len(), 1);
        assert_eq!(report.proposals[0].priority, ProposalPriority::Low);
        assert!(report.proposals[0].estimated_utility < 0.3);
    }

    #[test]
    fn test_utility_calculation_correctness() {
        // 5 total failed phases, pattern has frequency 2 => utility = 0.4 => Medium
        let records = vec![
            make_record(
                "p",
                vec![
                    make_phase("svc-a", "failed", 1),
                    make_phase("svc-b", "failed", 1),
                ],
                false,
            ),
            make_record(
                "p",
                vec![
                    make_phase("svc-a", "failed", 1),
                    make_phase("svc-c", "failed", 1),
                    make_phase("svc-d", "hard_halt", 3),
                ],
                true,
            ),
        ];

        let pattern = make_pattern(&["svc-a"], &["change"], 2, 1.0);
        let report = generate_proposals(&[pattern], &records);

        assert_eq!(report.proposals.len(), 1);
        let u = report.proposals[0].estimated_utility;
        assert!((u - 0.4).abs() < 0.01, "utility should be 0.4, got {}", u);
        assert_eq!(report.proposals[0].priority, ProposalPriority::Medium);
    }

    #[test]
    fn test_proposals_sorted_by_utility_descending() {
        let records = vec![
            make_record(
                "p",
                vec![
                    make_phase("svc-a", "failed", 1),
                    make_phase("svc-b", "failed", 1),
                ],
                false,
            ),
            make_record(
                "p",
                vec![
                    make_phase("svc-a", "failed", 1),
                    make_phase("svc-b", "failed", 1),
                    make_phase("svc-a", "failed", 1),
                ],
                false,
            ),
        ];

        let patterns = vec![
            make_pattern(&["svc-b"], &["change"], 2, 1.0),
            make_pattern(&["svc-a"], &["change"], 3, 1.0),
        ];

        let report = generate_proposals(&patterns, &records);

        assert_eq!(report.proposals.len(), 2);
        assert!(
            report.proposals[0].estimated_utility >= report.proposals[1].estimated_utility,
            "proposals should be sorted by utility descending"
        );
    }

    #[test]
    fn test_evidence_records_found() {
        let records = vec![
            make_record("p", vec![make_phase("svc-a", "failed", 1)], false),
            make_record("p", vec![make_phase("svc-b", "passed", 0)], false),
            make_record("p", vec![make_phase("svc-a", "hard_halt", 3)], true),
        ];

        let pattern = make_pattern(&["svc-a"], &["change"], 2, 2.0);
        let report = generate_proposals(&[pattern], &records);

        // Records 0 and 2 have svc-a failures
        assert_eq!(report.proposals[0].evidence_records, vec![0, 2]);
    }

    #[test]
    fn test_markdown_output_nonempty() {
        let records = vec![
            make_record("p", vec![make_phase("svc-a", "failed", 2)], false),
            make_record("p", vec![make_phase("svc-a", "failed", 3)], false),
        ];

        let pattern = make_pattern(&["svc-a"], &["type-change"], 2, 2.5);
        let report = generate_proposals(&[pattern], &records);
        let md = format_report_markdown(&report);

        assert!(md.contains("# Forge Evolution Report"));
        assert!(md.contains("svc-a"));
        assert!(md.contains("type-change"));
        assert!(md.contains("HIGH") || md.contains("MEDIUM") || md.contains("LOW"));
    }

    #[test]
    fn test_priority_label() {
        assert_eq!(ProposalPriority::High.label(), "HIGH");
        assert_eq!(ProposalPriority::Medium.label(), "MEDIUM");
        assert_eq!(ProposalPriority::Low.label(), "LOW");
    }
}
