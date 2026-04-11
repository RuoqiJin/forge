;; ══════════════════════════════════════════════════════
;; {SERVICE} -- Intent Declaration (Architecture-Grade)
;; Generated: {DATE} | Forge Deep Cartography v3
;; ══════════════════════════════════════════════════════

(intent {service-name}
  (granularity L1-Blueprint)
  (version "1.0")
  (generated-at {TIMESTAMP})

  (service-type "{rust-axum-microservice|nextjs-app|python-comfyui}")
  (port {PORT} :env "{PORT_ENV_VAR}")
  (database "{postgresql|sqlite|none}")

  ;; ── DESIGN CONSTRAINTS ──
  (design-constraints
    ;; 3-8 one-line constraints, no implementation detail
    ({constraint-key} "{one-line description}"))

  ;; ── CROSS-SERVICE DEPENDENCIES ──
  (downstream-services
    ;; Service names + purpose only, no URLs or auth details
    (calls {service} :purpose "{one-line}"))

  ;; ── PILLARS ──
  (pillar {pillar-name}
    (purpose "{one-line}")
    (module-link ".missiond/intent-{pillar}-db.lisp" (level L3-Implementation))
    (module-link ".missiond/intent-{pillar}-flows.lisp" (level L2-Topology)))

  ;; ── FLOWS ──
  (flows
    ;; Step names only, no detail
    (flow {name} (steps A -> B -> C -> D))))
