;;;; 06-agent-tool.scm — The Agent tool (compound procedures)
;;;;
;;;; In SICP, a compound procedure is a procedure whose body is
;;;; itself an evaluation. When you call it, a new environment is
;;;; created, the parameters are bound, and the body is evaluated
;;;; in that fresh scope.
;;;;
;;;; The Agent tool is exactly this. Its `execute` function:
;;;;   1. Creates a FRESH environment (just the task prompt)
;;;;   2. Creates a FILTERED tool registry (Agent removed)
;;;;   3. Runs a NEW eval-apply loop in that scope
;;;;   4. Returns the final message as the tool result
;;;;
;;;; Fresh environment = new lexical scope.
;;;; Filtered tools = preventing infinite recursion (no base case).
;;;; Nested loop = eval calling apply calling eval.
;;;;
;;;; This is where the metacircular evaluator becomes truly
;;;; metacircular: the evaluator evaluates sub-evaluations.
;;;;
;;;; Maps to: src-rust/crates/query/src/agent_tool.rs
;;;; SICP parallel: Section 4.1.3 — compound-apply

;;; ─────────────────────────────────────────────
;;; The Agent tool
;;; ─────────────────────────────────────────────

(define (make-agent-tool mock-model tool-registry)
  (make-tool "Agent"
    "Launch a sub-agent to handle a complex task autonomously"
    (lambda (input ctx)
      (let* ((prompt (cdr (assq 'prompt input)))
             ;; 1. Fresh environment — new scope
             (sub-env (list (make-user-msg prompt)))
             ;; 2. Filter out Agent to prevent infinite recursion
             ;; In SICP: you can't have a function that calls itself
             ;; with no base case. Removing Agent IS the base case.
             (sub-tools (remove-tool-by-name "Agent" tool-registry))
             ;; 3. Run a nested eval-apply loop
             ;; THIS IS THE RECURSION. The evaluator evaluating itself.
             (result (run-agent-loop mock-model sub-tools sub-env 5)))
        ;; 4. Extract the final message and return as tool result
        (if (outcome? result)
            (make-tool-result (get-message-text (outcome-message result)) #f)
            (make-tool-result "Sub-agent did not terminate." #t))))))

;;; ─────────────────────────────────────────────
;;; Helper: remove a tool by name from a registry
;;; ─────────────────────────────────────────────

(define (remove-tool-by-name name registry)
  (filter (lambda (t) (not (string=? (tool-name t) name)))
          registry))
