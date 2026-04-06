;;;; 06-agent-tool-test.scm — Tests for the Agent tool (compound procedures)
;;;;
;;;; The Agent tool is a tool whose execute function spawns a NEW
;;;; eval-apply loop with a FRESH environment. It's a compound procedure:
;;;; a procedure whose body is itself an evaluation.
;;;;
;;;; In SICP:
;;;;   (define (compound-apply proc args)
;;;;     (eval-sequence (procedure-body proc)
;;;;       (extend-environment (procedure-parameters proc) args
;;;;         (procedure-environment proc))))
;;;;
;;;; Here:
;;;;   Agent.execute(input) =
;;;;     run-agent-loop(model, tools, fresh-env-from-input, max-turns)
;;;;
;;;; Fresh environment = new lexical scope.
;;;; Removing Agent from sub-tools = preventing infinite recursion.
;;;; The result flows back as a tool result in the parent conversation.
;;;;
;;;; Maps to: src-rust/crates/query/src/agent_tool.rs
;;;; SICP parallel: Section 4.1.3 — Evaluator Data Structures (compound procedures)

(import (scheme base)
        (scheme write)
        (scheme read)
        (scheme process-context)
        (scheme load))

(load "00-prelude.scm")
(load "01-types.scm")
(load "02-stream.scm")
(load "03-tools.scm")
(load "04-eval-apply.scm")
(load "05-compact.scm")
(load "06-agent-tool.scm")

(display-section "Agent tool construction")

(run-test "agent tool has the right name"
  (lambda ()
    (let ((agent (make-agent-tool simple-mock-model default-mock-registry)))
      (assert-equal (tool-name agent) "Agent" "name is Agent"))))

(display-section "Agent tool execution — nested eval-apply")

(run-test "agent tool runs a sub-conversation and returns result"
  (lambda ()
    (let* ((agent (make-agent-tool simple-mock-model default-mock-registry))
           (input '((description . "test agent")
                    (prompt . "Hello from parent!")))
           (result (tool-execute agent input '())))
      (assert-false (tool-result-error? result) "should not be an error")
      (assert-true (string? (tool-result-content result))
                   "returns a string result"))))

(run-test "agent tool sub-conversation can use tools"
  (lambda ()
    ;; The sub-agent should be able to use tools
    (let* ((agent (make-agent-tool simple-mock-model default-mock-registry))
           (input '((description . "file lister")
                    (prompt . "List the files in the project")))
           (result (tool-execute agent input '())))
      (assert-false (tool-result-error? result) "should not be an error")
      (assert-true (> (string-length (tool-result-content result)) 0)
                   "returned non-empty result"))))

(display-section "Agent tool prevents infinite recursion")

(run-test "agent tool is not available to sub-agents"
  (lambda ()
    ;; The sub-agent's tool registry should NOT contain the Agent tool.
    ;; This prevents infinite recursion: Agent spawning Agent spawning Agent...
    ;; In SICP terms: we prevent a function from calling itself without a base case.
    (let* ((agent (make-agent-tool simple-mock-model default-mock-registry))
           ;; Create a registry that includes the agent tool
           (registry-with-agent
             (make-tool-registry (cons agent (list mock-bash-tool mock-read-tool))))
           ;; Now make an agent from THAT registry
           (outer-agent (make-agent-tool simple-mock-model registry-with-agent))
           ;; The sub-agent's registry should have Agent filtered out
           (input '((description . "test")
                    (prompt . "Hello")))
           (result (tool-execute outer-agent input '())))
      ;; If this returns at all (doesn't stack overflow), the guard works
      (assert-false (tool-result-error? result) "completed without infinite recursion"))))

(display-section "Agent tool in the full loop")

(run-test "model can delegate to a sub-agent"
  (lambda ()
    ;; Build a model that delegates work to an Agent tool
    (let* ((agent (make-agent-tool simple-mock-model default-mock-registry))
           (registry-with-agent
             (make-tool-registry (cons agent (list mock-bash-tool mock-read-tool mock-grep-tool))))
           ;; A model that always delegates to Agent
           (delegating-model
             (lambda (env tools)
               (let ((last-msg (last-element env)))
                 (if (has-tool-results? last-msg)
                     (make-text-stream "The sub-agent found the answer.")
                     (make-tool-use-stream "id-agent" "Agent"
                       '((description . "research")
                         (prompt . "Hello from sub-agent")))))))
           (env (list (make-user-msg "Delegate this task")))
           (result (run-agent-loop delegating-model registry-with-agent env 5)))
      (assert-true (outcome? result) "should terminate")
      (assert-equal (outcome-type result) 'end-turn "ends normally"))))

(test-summary)
