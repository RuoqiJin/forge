;; ══════════════════════════════════════════
;; Pattern: bootstrap
;; Project: Jarvis
;; Target: jarvis-daemon/src/main.rs — replace 400 lines of hand-wired Arc::new/clone
;; Generates: bootstrap_jarvis() with topological-sort init + module registration + worker spawn
;; ══════════════════════════════════════════

(bootstrap jarvis
  :target "crates/jarvis-daemon/src/gen_init.rs"

  ;; ── Foundation layer (order matters) ──
  (infra
    (component pool       :type PgPool   :init "PgPool::connect(&config.database_url).await?")
    (component storage    :type Storage  :wrap "Arc::new" :deps (pool)
      :post-init "storage.migrate().await?")
    (component event_bus  :type EventBus :init "EventBus::new(256)")
    (component session_mgr :type SessionManager :wrap "Arc::new" :deps (event_bus)
      :init "SessionManager::new(event_bus.sender())")
    (component slot_manager :type SlotManager :wrap "Arc::new"
      :init "load_slot_manager(&config)")
    (component project_root :type PathBuf
      :init "config.cwd.as_ref().map(PathBuf::from).unwrap_or_else(|| std::env::current_dir().unwrap_or_default())")
    (component awareness :type Awareness :wrap "Arc::new" :deps (project_root)
      :init "Awareness::new(project_root.clone())"
      :post-init "awareness.refresh().await?")
    (component topology_mgr :type TopologyManager :wrap "Arc::new" :deps (storage awareness project_root))
    (component reality_mirror :type RealityMirror :wrap "Arc::new" :deps (storage project_root))
    (component intent_mgr :type IntentManager :wrap "Arc::new" :deps (storage project_root)))

  ;; ── MCP Modules (register into ModuleHost) ──
  (modules
    (module board     :struct BoardModule    :deps (storage))
    (module code_map  :struct CodeMapModule  :deps (storage))
    (module kb        :struct KbModule       :deps (storage))
    (module context   :struct ContextModule  :deps-raw "event_bus.sender()"))

  ;; ── Workers (tokio::spawn) ──
  (workers
    (spawn autopilot
      :struct Autopilot
      :path "control::autopilot"
      :init "Autopilot::new(storage.clone(), event_bus.subscribe())"
      :method "run")
    (spawn topology-guardian
      :struct TopologyGuardian
      :path "control::topology_guardian"
      :init "TopologyGuardian::new(topology_mgr.clone(), event_bus.subscribe())"
      :method "run")
    (spawn intent-refiner
      :struct IntentRefiner
      :path "control::intent_refiner"
      :slot "intent-refiner"
      :method "run")
    (spawn verification-worker
      :struct VerificationWorker
      :path "control::verification_worker"
      :method "run")
    (spawn delta-monitor
      :struct DeltaMonitor
      :path "memory::delta_monitor"
      :method "run")
    (spawn message-persister
      :struct MessagePersister
      :path "memory::message_persister"
      :init "MessagePersister::start(storage.clone(), event_bus.clone())"
      :method "start")
    (spawn extraction-worker
      :struct ExtractionWorker
      :path "control::extraction"
      :slot "extraction-worker"
      :method "run")
    (spawn jsonl-watcher
      :struct JsonlWatcher
      :path "memory::watcher"
      :init "JsonlWatcher::new(event_bus.sender(), storage.clone())"
      :method "run")
    (spawn git-watcher
      :struct GitWatcher
      :path "memory::git_watcher"
      :init "GitWatcher::new(event_bus.sender(), project_root.clone())"
      :method "run")))
