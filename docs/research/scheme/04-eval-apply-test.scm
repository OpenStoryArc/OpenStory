;;;; 04-eval-apply-test.scm — Tests for the agent loop (the coalgebra)
;;;;
;;;; This is the test for the HEART of the system: the eval-apply loop.
;;;;
;;;; eval = model call (given the conversation, what does the model say?)
;;;; apply = tool dispatch (given a tool-use, what does the world say back?)
;;;;
;;;; The loop unfolds the conversation coinductively:
;;;;   state → step → step → step → ... → outcome
;;;;
;;;; We don't know in advance how many steps there will be.
;;;; That's the coalgebra. That's the unfold.
;;;;
;;;; Maps to: src-rust/crates/query/src/lib.rs (run_query_loop)
;;;; SICP parallel: Section 4.1.1 — The Core of the Evaluator

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

(display-section "Mock model — eval as a function")

;; The mock model IS a function from environment to stream-of-events.
;; It pattern-matches on the conversation to decide what to say.
;; This makes explicit what's usually hidden behind an API call:
;; "eval" is just a function from environment to expression.

(run-test "mock model returns text for simple greeting"
  (lambda ()
    (let* ((env (list (make-user-msg "Hello!")))
           (events (simple-mock-model env default-mock-registry))
           (result (fold-stream events))
           (msg (stream-result-message result))
           (stop (stream-result-stop-reason result)))
      (assert-equal stop "end_turn" "should end turn")
      (assert-equal (message-role msg) 'assistant "assistant role"))))

(run-test "mock model requests tool-use for actionable requests"
  (lambda ()
    (let* ((env (list (make-user-msg "List the files in src/")))
           (events (simple-mock-model env default-mock-registry))
           (result (fold-stream events))
           (msg (stream-result-message result))
           (stop (stream-result-stop-reason result)))
      (assert-equal stop "tool_use" "should request tool use")
      (assert-true (> (length (get-tool-uses msg)) 0) "has tool-use blocks"))))

(display-section "Single step of the coalgebra")

;; One step: call the model, get a response, check stop reason.
;; If tool_use: dispatch tools, append results, return new state.
;; If end_turn: return an outcome (terminal value).

(run-test "step that ends the turn returns an outcome"
  (lambda ()
    (let* ((env (list (make-user-msg "Hello!")))
           (result (agent-step env simple-mock-model default-mock-registry)))
      (assert-true (outcome? result) "should be a terminal outcome")
      (assert-equal (outcome-type result) 'end-turn "type is end-turn"))))

(run-test "step with tool-use returns new environment (not outcome)"
  (lambda ()
    (let* ((env (list (make-user-msg "List the files")))
           (result (agent-step env simple-mock-model default-mock-registry)))
      ;; Should NOT be an outcome — should be a new, longer environment
      (assert-false (outcome? result) "should be new state, not outcome")
      (assert-true (list? result) "new state is a list")
      ;; The environment grew: original msg + assistant msg + tool results
      (assert-true (> (length result) (length env))
                   "environment grew"))))

(display-section "The full loop — coalgebra unfolding")

;; The loop drives `step` repeatedly until an outcome is reached.
;; This is the anamorphism — the unfold.

(run-test "simple conversation: greeting → response"
  (lambda ()
    (let* ((env (list (make-user-msg "Hello, Claude!")))
           (result (run-agent-loop simple-mock-model default-mock-registry env 10)))
      (assert-true (outcome? result) "should terminate")
      (assert-equal (outcome-type result) 'end-turn "ends normally"))))

(run-test "tool-using conversation: request → tool → response"
  (lambda ()
    ;; The mock model will: 1) use a tool, 2) then respond with text
    (let* ((env (list (make-user-msg "What files are in this project?")))
           (result (run-agent-loop simple-mock-model default-mock-registry env 10)))
      (assert-true (outcome? result) "should terminate")
      (assert-equal (outcome-type result) 'end-turn "ends normally")
      ;; The conversation should have multiple turns
      (let ((final-env (outcome-environment result)))
        (assert-true (> (length final-env) 2)
                     "conversation has multiple turns")))))

(run-test "max turns guard prevents infinite loops"
  (lambda ()
    ;; Use a model that always requests tools — it would loop forever
    ;; without the max-turns guard.
    (let* ((env (list (make-user-msg "Do something")))
           (result (run-agent-loop always-tool-model default-mock-registry env 3)))
      (assert-true (outcome? result) "should terminate")
      (assert-equal (outcome-type result) 'max-turns
                    "stopped by max turns guard"))))

(display-section "Multi-tool conversation")

(run-test "model uses grep then read (multi-step tool chain)"
  (lambda ()
    ;; The mock model should: grep for TODO → read a file → summarize
    (let* ((env (list (make-user-msg "Find all TODOs and show me the worst one")))
           (result (run-agent-loop multi-step-mock-model default-mock-registry env 10)))
      (assert-true (outcome? result) "should terminate")
      (assert-equal (outcome-type result) 'end-turn "ends normally")
      (let ((final-env (outcome-environment result)))
        ;; Should have at least: user, assistant(grep), user(result),
        ;; assistant(read), user(result), assistant(summary)
        (assert-true (>= (length final-env) 5)
                     "multiple tool round-trips happened")))))

(test-summary)
