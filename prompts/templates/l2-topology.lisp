;; ══════════════════════════════════════════════════════
;; {SERVICE} -- Topology (Flow & Wiring)
;; Generated: {DATE} | Forge Deep Cartography v3
;; ══════════════════════════════════════════════════════

(intent {service-name}-topology
  (granularity L2-Topology)
  (parent "intent.lisp")

  (state-machine {name}
    (states ({State1}) ({State2}) ({State3}))
    (transitions
      ({State1} -> {State2} :trigger "{event}")
      ({State2} -> {State3} :trigger "{event}")))

  (flow {pipeline-name}
    (description "{what this flow does}")
    (steps
      {step-1}
      -> {step-2}
      -> {step-3}))

  (bootstrap {name}
    (depends {module-a} {module-b})
    (init-order {infra} -> {modules} -> {workers})))
