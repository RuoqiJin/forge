;; ══════════════════════════════════════════
;; Pattern: crud-gateway
;; Project: Jarvis
;; Target: memory/storage.rs (1375 lines -> 0 hand-written SQL)
;; ══════════════════════════════════════════
(pattern crud-gateway
  :gateway Storage
  :pool    PgPool
  :target  "crates/jarvis-core/src/memory/storage.rs"

  ;; ── Column type vocabulary ──
  ;; uuid | text | integer | bigint | boolean | float8 | jsonb | timestamptz

  ;; ── Op kind vocabulary ──
  ;; insert            — INSERT, auto-gen id/ts, return constructed struct
  ;; insert-struct     — INSERT from &StructRef, all fields bound
  ;; select-one        — SELECT WHERE pk=$1, return Option<T>
  ;; select-many       — SELECT with WHERE/ORDER/LIMIT, return Vec<T>
  ;; select-json       — SELECT to_jsonb(t)/row_to_json(t), return Vec<Value>
  ;; update            — UPDATE SET explicit columns WHERE pk
  ;; update-dynamic    — build SET clause from JSON keys at runtime
  ;; update-cas        — compare-and-swap: UPDATE with preconditions, return bool
  ;; delete            — DELETE WHERE pk=$1
  ;; upsert            — INSERT ON CONFLICT DO UPDATE/NOTHING
  ;; count             — SELECT COUNT(*) WHERE ...
  ;; sync-batch        — DELETE old + INSERT N new (transactional)
  ;; custom            — raw SQL escape hatch, hand-written body preserved

  ;; ════════════════════════════════
  ;; Table 1/11: sessions
  ;; ════════════════════════════════
  (table sessions
    (col id           :type uuid        :pk)
    (col status       :type text        :default "idle")
    (col jsonl_path   :type text        :nullable)
    (col started_at   :type timestamptz :default-now)
    (col ended_at     :type timestamptz :nullable)
    (col project_name :type text        :nullable)

    (row-type SessionRow
      :derive (sqlx::FromRow)
      :convert into_session :to Session
      :enum-map ((status session_status_from_str SessionStatus)))

    (op create_session
      :kind insert
      :auto (id :via "Uuid::new_v4()")
      :auto (started_at :via "Utc::now()")
      :defaults ((status "idle"))
      :returns Session)

    (op get_session
      :kind select-one
      :where ((id uuid))
      :returns "Option<Session>")

    (op update_session_status
      :kind update
      :where ((id uuid))
      :set ((status SessionStatus :via session_status_to_str)
            (ended_at "Option<DateTime<Utc>>" :conditional "status == SessionStatus::Dead"))
      :returns "()")

    (op list_sessions
      :kind select-many
      :params ((limit i64))
      :order "started_at DESC"
      :returns "Vec<Session>")

    (op get_or_create_session
      :kind upsert
      :params ((id uuid))
      :on-conflict "(id) DO NOTHING"
      :returns Session)

    (op get_session_project
      :kind custom
      :params ((session_id uuid))
      :sql "SELECT project_name FROM sessions WHERE id = $1"
      :returns "Option<String>")

    (op set_session_project
      :kind update
      :where ((id uuid :bind-name session_id))
      :set ((project_name "&str"))
      :returns "()"))

  ;; ════════════════════════════════
  ;; Table 2/11: messages
  ;; ════════════════════════════════
  (table messages
    (col id          :type uuid        :pk)
    (col session_id  :type uuid        :fk sessions)
    (col role        :type text)
    (col content     :type text)
    (col raw_content :type jsonb       :nullable)
    (col tool_name   :type text        :nullable)
    (col seq         :type integer)
    (col created_at  :type timestamptz :default-now)
    (idx idx_messages_session_seq :cols (session_id seq))

    (row-type MessageRow
      :derive (sqlx::FromRow)
      :convert into_message :to Message)

    (op insert_message
      :kind insert-struct
      :param (msg "&Message")
      :returns "()")

    (op get_messages
      :kind select-many
      :where ((session_id uuid))
      :params ((limit i64) (offset i64))
      :order "seq ASC"
      :returns "Vec<Message>")

    (op store_message
      :kind insert
      :params ((session_id uuid) (role "&str") (content "&str"))
      :auto (id :via "Uuid::new_v4()")
      :auto (seq :via-query "SELECT COALESCE(MAX(seq), 0) + 1 FROM messages WHERE session_id = $1")
      :auto (created_at :via "Utc::now()")
      :returns Uuid)

    (op get_recent_messages
      :kind select-many
      :where ((session_id uuid))
      :params ((limit i64))
      :order "seq DESC"
      :post-process reverse
      :returns "Vec<Message>")

    (op get_message_count
      :kind count
      :where ((session_id uuid))
      :returns i64))

  ;; ════════════════════════════════
  ;; Table 3/11: watcher_cursors
  ;; ════════════════════════════════
  (table watcher_cursors
    (col file_path   :type text   :pk)
    (col byte_offset :type bigint :default 0)
    (col updated_at  :type timestamptz :default-now)

    (op save_watcher_cursor
      :kind upsert
      :params ((file_path "&str") (offset u64 :cast-to i64))
      :on-conflict "(file_path) DO UPDATE SET byte_offset = $2, updated_at = now()"
      :returns "()")

    (op load_watcher_cursors
      :kind select-many
      :select (file_path byte_offset)
      :cast ((byte_offset i64 :to u64))
      :returns "Vec<(String, u64)>"))

  ;; ════════════════════════════════
  ;; Table 4/11: board_tasks
  ;; ════════════════════════════════
  (table board_tasks
    (col id                :type text        :pk)
    (col title             :type text)
    (col description       :type text        :nullable)
    (col status            :type text        :default "open")
    (col priority          :type text        :default "medium")
    (col category          :type text        :default "other")
    (col parent_id         :type text        :nullable)
    (col assignee          :type text        :nullable)
    (col auto_execute      :type integer     :default 0)
    (col prompt_template   :type text        :nullable)
    (col claim_executor_id :type text        :nullable)
    (col claimed_at        :type timestamptz :nullable)
    (col lease_expires_at  :type timestamptz :nullable)
    (col depends_on        :type jsonb       :nullable)
    (col notes_count       :type integer     :default 0)
    (col created_at        :type timestamptz :default-now)
    (col updated_at        :type timestamptz :default-now)

    (op create_board_task
      :kind insert
      :params ((id "&str") (title "&str") (description "Option<&str>")
               (priority "&str") (category "&str") (auto_execute bool :cast-to i32)
               (prompt_template "Option<&str>") (assignee "Option<&str>")
               (parent_id "Option<&str>") (depends_on "Option<&serde_json::Value>"))
      :returns "()")

    (op get_board_task
      :kind select-json
      :where ((id "&str"))
      :returns "Option<serde_json::Value>")

    (op list_board_tasks
      :kind select-json
      :params ((status "Option<&str>") (limit i64))
      :where-optional ((status text))
      :order "created_at DESC"
      :returns "Vec<serde_json::Value>")

    (op update_board_task
      :kind update-dynamic
      :where ((id "&str"))
      :dynamic-fields (status title description priority assignee)
      :auto-set (updated_at :via "now()")
      :returns "()")

    (op delete_board_task
      :kind delete
      :where ((id "&str"))
      :returns "()")

    (op claim_board_task
      :kind update-cas
      :where ((id "&str"))
      :preconditions ("status = 'open'" "claim_executor_id IS NULL")
      :set ((status :literal "'running'")
            (claim_executor_id :param executor_id "&str")
            (claimed_at :via "now()")
            (updated_at :via "now()"))
      :returns bool)

    (op reset_board_task_claim
      :kind update
      :where ((id "&str"))
      :set ((status :literal "'open'")
            (claim_executor_id :literal "NULL")
            (claimed_at :literal "NULL")
            (updated_at :via "now()"))
      :returns "()")

    (op list_auto_execute_tasks
      :kind select-json
      :where-fixed ("auto_execute = 1" "status = 'open'")
      :order "created_at ASC"
      :returns "Vec<serde_json::Value>"))

  ;; ════════════════════════════════
  ;; Table 5/11: board_task_notes
  ;; ════════════════════════════════
  (table board_task_notes
    (col id         :type text        :pk)
    (col task_id    :type text        :fk board_tasks :on-delete cascade)
    (col content    :type text)
    (col note_type  :type text        :default "note")
    (col author     :type text        :nullable)
    (col created_at :type timestamptz :default-now)

    (op add_board_note
      :kind insert
      :params ((task_id "&str") (content "&str") (note_type "&str") (author "Option<&str>"))
      :auto (id :via "Uuid::new_v4().to_string()")
      :side-effect "UPDATE board_tasks SET notes_count = notes_count + 1, updated_at = now() WHERE id = $task_id"
      :returns "()")

    (op get_board_notes
      :kind select-json
      :where ((task_id "&str"))
      :order "created_at ASC"
      :returns "Vec<serde_json::Value>"))

  ;; ════════════════════════════════
  ;; Table 6/11: ast_nodes
  ;; ════════════════════════════════
  (table ast_nodes
    (col id           :type text    :pk)
    (col file_path    :type text)
    (col node_type    :type text)
    (col name         :type text)
    (col signature    :type text)
    (col start_line   :type integer)
    (col end_line     :type integer)
    (col is_exported  :type boolean :default false)
    (col docstring    :type text    :nullable)
    (col stub_content :type text)
    (col calls        :type text    :nullable)
    (col beacons      :type text    :nullable)
    (col updated_at   :type timestamptz :default-now)
    (idx idx_ast_nodes_file :cols (file_path))
    (idx idx_ast_nodes_name :cols (name))
    (idx idx_ast_nodes_type :cols (node_type))

    (op ast_sync_file
      :kind sync-batch
      :params ((file_path "&str") (nodes "&[crate::memory::ast_indexer::CodeNode]"))
      :delete-where "file_path = $1"
      :insert-each node
      :returns "()")

    (op ast_search
      :kind select-json
      :params ((query "&str" :ilike-wrap) (limit i64))
      :where-ilike (name signature)
      :order "file_path, start_line"
      :returns "Vec<serde_json::Value>")

    (op ast_get_file
      :kind select-json
      :where ((file_path "&str"))
      :order "start_line"
      :returns "Vec<serde_json::Value>")

    (op ast_get_tree
      :kind select-json
      :order "file_path, start_line"
      :returns "Vec<serde_json::Value>")

    (op ast_stats
      :kind custom
      :queries ((total "SELECT COUNT(*) FROM ast_nodes")
                (by_type "SELECT node_type, COUNT(*) FROM ast_nodes GROUP BY node_type ORDER BY COUNT(*) DESC")
                (by_file "SELECT file_path, COUNT(*) FROM ast_nodes GROUP BY file_path ORDER BY file_path"))
      :returns serde_json::Value))

  ;; ════════════════════════════════
  ;; Table 7/11: knowledge
  ;; ════════════════════════════════
  (table knowledge
    (col id             :type text    :pk)
    (col category       :type text)
    (col key            :type text)
    (col summary        :type text)
    (col detail         :type jsonb   :nullable)
    (col source         :type text    :default "conversation")
    (col confidence     :type float8  :default 1.0)
    (col access_count   :type integer :default 0)
    (col kb_type        :type text    :default "fact")
    (col utility_score  :type float8  :default 0.5)
    (col created_at     :type timestamptz :default-now)
    (col updated_at     :type timestamptz :default-now)
    (unique (category key))
    (idx idx_knowledge_category :cols (category))

    (op kb_remember
      :kind custom
      :params ((category "&str") (key "&str") (summary "&str")
               (detail "Option<&serde_json::Value>") (source "Option<&str>")
               (confidence "Option<f64>"))
      :logic "check-then-upsert"
      :returns "(String, String)")

    (op kb_search
      :kind select-json
      :params ((query "&str" :ilike-wrap) (category "Option<&str>" :prefix-like) (limit i64))
      :where-ilike (key summary)
      :where-optional ((category text :op "LIKE"))
      :order "utility_score DESC, updated_at DESC"
      :returns "Vec<serde_json::Value>")

    (op kb_list
      :kind select-json
      :params ((category "Option<&str>" :prefix-like) (limit i64))
      :where-optional ((category text :op "LIKE"))
      :order "updated_at DESC"
      :returns "Vec<serde_json::Value>")

    (op kb_get
      :kind select-json
      :where ((category "&str") (key "&str"))
      :returns "Option<serde_json::Value>")

    (op kb_delete
      :kind delete
      :where ((id "&str"))
      :returns bool)

    (op kb_summary
      :kind custom
      :sql "SELECT category, COUNT(*) as cnt FROM knowledge GROUP BY category ORDER BY cnt DESC"
      :returns "Vec<(String, i64)>"))

  ;; ════════════════════════════════
  ;; Table 8/11: consumer_watermarks
  ;; ════════════════════════════════
  (table consumer_watermarks
    (col consumer_name       :type text)
    (col session_id          :type text)
    (col last_processed_time :type timestamptz :nullable)
    (col last_processed_msg_id :type bigint    :nullable)
    (pk (consumer_name session_id))

    (op get_pending_extraction_messages
      :kind custom
      :params ((limit i64))
      :sql-cte "pending extraction CTE with ROW_NUMBER window + watermark join"
      :returns "Vec<serde_json::Value>")

    (op watermark_advance
      :kind upsert
      :params ((consumer "&str") (session_id "&str") (timestamp "&str"))
      :on-conflict "(consumer_name, session_id) DO UPDATE SET last_processed_time = $3::timestamptz"
      :returns "()"))

  ;; ════════════════════════════════
  ;; Table 9/11: topology_snapshots
  ;; ════════════════════════════════
  (table topology_snapshots
    (col id           :type text        :pk)
    (col commit_hash  :type text)
    (col commit_msg   :type text        :nullable)
    (col sexp_text    :type text)
    (col diff_summary :type text        :nullable)
    (col trigger      :type text        :default "unknown")
    (col project_name :type text        :nullable :default "jarvis")
    (col created_at   :type timestamptz :default-now)
    (idx idx_topology_snapshots_commit  :cols (commit_hash))
    (idx idx_topology_snapshots_project :cols (project_name "created_at DESC"))

    (op insert_topology_snapshot
      :kind insert
      :params ((id "&str") (commit_hash "&str") (commit_msg "Option<&str>")
               (sexp_text "&str") (diff_summary "Option<&str>") (trigger "&str"))
      :returns "()")

    (op get_latest_topology_snapshot
      :kind custom
      :select (id commit_hash commit_msg diff_summary trigger "created_at::text")
      :order "created_at DESC"
      :limit 1
      :returns "Option<serde_json::Value>")

    (op list_topology_snapshots
      :kind custom
      :params ((limit usize :cast-to i64))
      :select (id commit_hash commit_msg diff_summary trigger "created_at::text")
      :order "created_at DESC"
      :returns "Vec<serde_json::Value>")

    (op get_topology_snapshot_text
      :kind custom
      :where ((id "&str"))
      :select (sexp_text)
      :returns "Option<String>"))

  ;; ════════════════════════════════
  ;; Table 10/11: intent_snapshots
  ;; ════════════════════════════════
  (table intent_snapshots
    (col id           :type text        :pk)
    (col commit_hash  :type text)
    (col commit_msg   :type text        :nullable)
    (col lisp_text    :type text)
    (col diff_summary :type text        :nullable)
    (col trigger      :type text        :default "unknown")
    (col delta_report :type jsonb       :nullable)
    (col project_name :type text        :nullable :default "jarvis")
    (col created_at   :type timestamptz :default-now)
    (idx idx_intent_snapshots_created :cols ("created_at DESC"))
    (idx idx_intent_snapshots_project :cols (project_name "created_at DESC"))

    (op insert_intent_snapshot
      :kind insert
      :params ((id "&str") (commit_hash "&str") (commit_msg "Option<&str>")
               (lisp_text "&str") (diff_summary "Option<&str>")
               (trigger "&str") (delta_report "Option<&serde_json::Value>"))
      :returns "()")

    (op get_latest_intent_snapshot
      :kind custom
      :select (id commit_hash commit_msg diff_summary trigger "created_at::text")
      :order "created_at DESC"
      :limit 1
      :returns "Option<serde_json::Value>")

    (op list_intent_snapshots
      :kind custom
      :params ((limit usize :cast-to i64))
      :select (id commit_hash commit_msg diff_summary trigger "created_at::text")
      :order "created_at DESC"
      :returns "Vec<serde_json::Value>")

    (op get_intent_snapshot_text
      :kind custom
      :where ((id "&str"))
      :select (lisp_text)
      :returns "Option<String>")

    (op get_intent_snapshot_delta
      :kind custom
      :where ((id "&str"))
      :select (delta_report)
      :returns "Option<serde_json::Value>")

    (op get_project_context
      :kind custom
      :params ((project_name "&str"))
      :sql "SELECT lisp_text, delta_report FROM intent_snapshots WHERE project_name = $1 ORDER BY created_at DESC LIMIT 1"
      :returns "Option<(String, Option<serde_json::Value>)>"))

  ;; ════════════════════════════════
  ;; Table 11/11: projects
  ;; ════════════════════════════════
  (table projects
    (col name           :type text        :pk)
    (col project_root   :type text)
    (col last_synced_at :type timestamptz :nullable)
    (col created_at     :type timestamptz :default-now)

    (op register_project
      :kind upsert
      :params ((name "&str") (project_root "&str"))
      :on-conflict "(name) DO UPDATE SET project_root = $2"
      :returns "()")

    (op list_projects
      :kind custom
      :sql "SELECT name, project_root FROM projects ORDER BY name"
      :returns "Vec<(String, String)>"))

  ;; ── Helpers (generated alongside CRUD) ──
  (helpers
    (fn session_status_to_str :param (SessionStatus) :returns "&'static str"
      :map ((Idle "idle") (Busy "busy") (Dead "dead")))
    (fn session_status_from_str :param ("&str") :returns SessionStatus
      :map (("idle" Idle) ("busy" Busy) ("dead" Dead))
      :default Idle)))
