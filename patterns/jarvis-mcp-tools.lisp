;; ══════════════════════════════════════════
;; Pattern: mcp-tool
;; Project: Jarvis
;; Target: tools/board.rs, tools/kb.rs, tools/code_map.rs, tools/context.rs
;; Generates: ToolSchema registration + arg struct + handle_tool dispatch + trait contract
;; ══════════════════════════════════════════

;; ── Arg type vocabulary ──
;; string | integer | number | boolean | array | object
;; :enum restricts to fixed values
;; :required marks field as mandatory (goes into JSON Schema "required")
;; :default provides fallback for lenient extraction

(mcp-module board
  :struct BoardModule
  :deps ((storage "Arc<Storage>"))
  :dep-trait StorageCustomLogic

  (tool jarvis_board_create
    :description "Create a new Board task for tracking work items"
    (input
      (title     string :required :description "Task title")
      (description string :description "Detailed description")
      (priority  string :enum (high medium low) :default "medium")
      (category  string :enum (deploy dev infra test other) :default "other")
      (autoExecute boolean :default false :description "Auto-execute by Autopilot")
      (promptTemplate string :description "Prompt for auto-execution")
      (assignee  string :description "Assigned executor")
      (parentId  string :description "Parent task ID for hierarchy")
      (dependsOn array  :description "Task IDs this depends on"))
    :returns "Value"
    :events ((task_ready :when "auto_execute == true" :payload "{\"task_id\": id}")))

  (tool jarvis_board_query
    :description "Query Board tasks — list, get, search, or summary"
    (input
      (action string :required :enum (list get search summary))
      (id     string :description "Task ID for get action")
      (query  string :description "Search query text")
      (status string :description "Filter by status")
      (limit  integer :default 50))
    :returns "Value"
    :dispatch-on action
    :sub-actions
      ((list    :storage list_board_tasks :args (status limit))
       (get     :storage (get_board_task get_board_notes) :args (id))
       (search  :storage list_board_tasks :args (status limit))
       (summary :storage kb_summary)))

  (tool jarvis_board_update
    :description "Update a Board task's status or fields"
    (input
      (id          string :required)
      (status      string :enum (open running done failed blocked))
      (title       string)
      (description string)
      (priority    string)
      (assignee    string))
    :returns "Value"
    :events ((task_completed :when "status == done || status == failed"
              :payload "{\"task_id\": id, \"status\": status}")))

  (tool jarvis_board_note_add
    :description "Add a progress note to a Board task"
    (input
      (taskId   string :required)
      (content  string :required)
      (noteType string :enum (progress summary note) :default "note"))
    :returns "Value")

  (tool jarvis_board_delete
    :description "Delete a Board task and its notes"
    (input
      (id string :required))
    :returns "Value"))

(mcp-module kb
  :struct KbModule
  :deps ((storage "Arc<Storage>"))
  :dep-trait StorageCustomLogic

  (tool jarvis_kb_remember
    :description "Remember/upsert a knowledge entry into Jarvis KB"
    (input
      (category   string :required :enum (preference memory memory:architecture memory:bugfix memory:debug memory:ops memory:decision policy:decision architecture project))
      (key        string :required :description "Unique key within category")
      (summary    string :required :description "Conclusion statement, max 120 chars")
      (detail     object :description "Structured detail with trigger/conclusion/action")
      (confidence number :default 1.0 :description "Confidence score 0.0-1.0"))
    :returns "Value"
    :validation ((summary :min-len 5 :max-len 400)))

  (tool jarvis_kb_search
    :description "Search KB entries by keyword"
    (input
      (query    string :required)
      (category string :description "Filter by category prefix")
      (limit    integer :default 10))
    :returns "Value")

  (tool jarvis_kb_list
    :description "List KB entries, optionally filtered by category"
    (input
      (category string)
      (limit    integer :default 20))
    :returns "Value")

  (tool jarvis_memory_pending
    :description "Get pending messages for memory extraction"
    (input
      (limit integer :default 60))
    :returns "Value"))

(mcp-module code_map
  :struct CodeMapModule
  :deps ((storage "Arc<Storage>"))
  :dep-trait StorageCustomLogic

  (tool jarvis_code_map
    :description "Query AST code structure — tree, search, file, or stats"
    (input
      (action    string :required :enum (tree search file stats))
      (query     string :description "Search query for symbol names")
      (file_path string :description "File path for file action")
      (node_type string :enum (function struct enum trait impl type const mod class interface))
      (limit     integer :default 50))
    :returns "Value"
    :dispatch-on action))

(mcp-module context
  :struct ContextModule
  :deps ((event_tx "broadcast::Sender<Event>"))

  (tool jarvis_context_inject
    :description "Inject enriched context into a target foreground session"
    (input
      (target_session_id string :required :description "UUID of target session")
      (enriched_message  string :required :description "Rich context + user message"))
    :returns "Value"
    :strict-args true))
