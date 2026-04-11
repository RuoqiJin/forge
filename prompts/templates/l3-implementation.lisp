;; ══════════════════════════════════════════════════════
;; {SERVICE} -- Implementation Detail ({DOMAIN})
;; Generated: {DATE} | Forge Deep Cartography v3
;; ══════════════════════════════════════════════════════

(intent {service-name}-db-{domain}
  (granularity L3-Implementation)
  (parent "intent.lisp")

  (component {domain}-tables
    :pattern crud-gateway
    :target "{path}/mod.rs"

    (table {table-name}
      (col id :type uuid :pk)
      (col {field} :type {text|integer|bigint|boolean|float8|jsonb|timestamptz|uuid})
      (col created_at :type timestamptz)
      (col updated_at :type timestamptz)

      (op insert (binds {field1} {field2}))
      (op select-one (binds id))
      (op select-many (order-by created_at :desc) (limit 100))
      (op update (binds {field}) (where id))
      (op delete (where id)))))
