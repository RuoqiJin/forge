;; ══════════════════════════════════════════
;; Pattern: state-machine
;; Project: Jarvis
;; Target: control/pty.rs — PTY process controller state machine
;; Generates: State enum + transition validator + tracing + trait contract
;; ══════════════════════════════════════════

(state-machine session-state
  :enum SessionState
  :derive (Debug Clone Copy PartialEq Eq Serialize Deserialize)
  :target "crates/jarvis-core/src/control/pty"

  ;; ── State definitions ──
  (states
    (Starting    :doc "PTY spawned, waiting for Claude Code TUI to initialize")
    (Idle        :doc "Waiting for user input, prompt visible")
    (Thinking    :doc "Claude processing, spinner visible, no tool indicator")
    (Responding  :doc "Claude generating output/response")
    (ToolRunning :doc "Tool execution active (bash, read, write, etc.)")
    (Confirming  :doc "Permission dialog pending user response")
    (Exited      :doc "Process terminated"))

  ;; ── Grouping: which states count as "processing" ──
  (group processing (Thinking Responding ToolRunning)
    :method is_processing
    :doc "Returns true if Claude is actively processing (not waiting for input)")

  ;; ── Legal transitions (whitelist) ──
  ;; Any transition NOT in this list will be rejected by try_transition()
  (transitions
    ;; Starting exits
    (Starting -> Idle        :trigger "prompt-detected or timeout-fallback")
    (Starting -> Exited      :trigger "process-terminated")

    ;; Idle branches
    (Idle -> Thinking        :trigger "spinner-detected-no-tool-hint")
    (Idle -> ToolRunning     :trigger "tool-activity-or-spinner-with-tool-hint")
    (Idle -> Confirming      :trigger "permission-dialog-detected")
    (Idle -> Exited          :trigger "process-terminated")

    ;; Thinking flow
    (Thinking -> ToolRunning :trigger "tool-hint-emerges" :debounce 300ms)
    (Thinking -> Idle        :trigger "spinner-vanishes-prompt-returns" :debounce 300ms)
    (Thinking -> Responding  :trigger "output-block-detected")
    (Thinking -> Exited      :trigger "process-terminated")

    ;; ToolRunning flow
    (ToolRunning -> Thinking :trigger "spinner-reappears-no-tool" :debounce 300ms)
    (ToolRunning -> Idle     :trigger "prompt-returns" :debounce 300ms)
    (ToolRunning -> Exited   :trigger "process-terminated")

    ;; Responding flow
    (Responding -> Idle      :trigger "prompt-returns")
    (Responding -> Thinking  :trigger "spinner-resumes")
    (Responding -> ToolRunning :trigger "tool-activity-resumes")
    (Responding -> Exited    :trigger "process-terminated")

    ;; Confirming flow
    (Confirming -> Thinking  :trigger "dialog-dismissed-spinner")
    (Confirming -> ToolRunning :trigger "dialog-dismissed-tool")
    (Confirming -> Idle      :trigger "dialog-dismissed-prompt")
    (Confirming -> Exited    :trigger "process-terminated"))

  ;; ── Guards: preconditions for API calls ──
  (guards
    (send :requires Idle :error "Cannot send: not idle")
    (kill :requires-not Exited :error "Cannot kill: already exited"))

  ;; ── Trait contract for screen detection (hand-written) ──
  (trait SessionStateDetector
    (detect-from-screen
      :params ((lines "&[String]") (current "&SessionState"))
      :returns "Option<SessionState>"
      :doc "Analyze terminal screen lines to determine current state")
    (has-activity-timer
      :params ((lines "&[&str]"))
      :returns bool
      :doc "Check if screen shows active timer pattern (Ns · or Nm Ns ·)")))
