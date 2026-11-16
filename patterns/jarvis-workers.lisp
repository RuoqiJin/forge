;; ══════════════════════════════════════════
;; Pattern: event-listener / cron-worker
;; Project: Jarvis
;; Target: memory/message_persister.rs, memory/git_watcher.rs, memory/watcher.rs, control/extraction.rs
;; ══════════════════════════════════════════

;; ── Pattern A: Event Listener ──
;; Subscribes to EventBus, dispatches matching events to trait handler.
;; Generated shell: tokio::spawn + broadcast::Receiver + match loop
;; Hand-written: trait impl with business logic per event variant.

(worker message-persister
  :pattern event-listener
  :struct MessagePersister
  :constructor start
  :deps ((storage "Arc<Storage>") (event_bus EventBus))

  (listens-to
    (UserMessageReceived :fields (session_id content))
    (TextComplete        :fields (session_id content)))

  (trait MessagePersisterLogic
    (on-event UserMessageReceived
      :params ((session_id Uuid) (content String))
      :returns "()")
    (on-event TextComplete
      :params ((session_id Uuid) (content String))
      :returns "()"))

  (shutdown :on broadcast-close)
  (error-strategy log-and-continue))

;; ── Pattern B: Cron Worker (stateless) ──
;; Time-driven polling loop. No event subscription.
;; Generated shell: tokio::spawn + interval/sleep + tick dispatch
;; Hand-written: trait impl with tick() business logic.

(worker git-watcher
  :pattern cron-worker
  :struct GitWatcher
  :constructor new
  :deps ((event_tx "broadcast::Sender<Event>") (project_root PathBuf))

  (schedule :interval 5s :method sleep)

  ;; Internal mutable state managed by the generated shell
  (state
    (last_hash "Option<String>" :default None))

  (trait GitWatcherLogic
    (on-tick
      :params ((state "&mut GitWatcherState"))
      :returns "()"))

  (emits (CodebaseMutated :fields (commit_hash reason)))
  (shutdown :none)
  (error-strategy silent-continue))

;; ── Pattern B: Cron Worker (with storage) ──

(worker jsonl-watcher
  :pattern cron-worker
  :struct JsonlWatcher
  :constructor new
  :deps ((event_tx "broadcast::Sender<Event>") (storage "Arc<Storage>"))

  (schedule :interval 2s :method sleep)

  (state
    (cursors "Arc<RwLock<HashMap<String, u64>>>" :default "Default::default()")
    (base_path PathBuf :init "compute_base_path()"))

  (init load_cursors
    :doc "Restore cursor positions from DB on startup")

  (trait JsonlWatcherLogic
    (on-tick
      :params ((state "&JsonlWatcherState"))
      :returns "()"))

  (emits
    (MessagesIngested :fields (session_id count))
    (CodebaseMutated  :fields (commit_hash reason)))
  (shutdown :none)
  (error-strategy log-and-continue))

;; ── Pattern B: Cron Worker (with PTY) ──

(worker extraction-worker
  :pattern cron-worker
  :struct ExtractionWorker
  :constructor new
  :deps ((storage "Arc<Storage>")
         (session_mgr "Arc<SessionManager>")
         (event_tx "broadcast::Sender<Event>")
         (model String)
         (mcp_config_path "Option<String>")
         (cwd "Option<String>"))

  (schedule :interval 60s :method interval)

  (trait ExtractionWorkerLogic
    (on-tick
      :returns "()"))

  (shutdown :none)
  (error-strategy log-and-continue))
