;;;; 04-eval-apply.scm — The agent loop (the coalgebra)
;;;;
;;;; This is Section 4.1.1 of SICP, rewritten for an AI agent.
;;;;
;;;; In SICP, the evaluator is two mutually recursive procedures:
;;;;
;;;;   (define (eval exp env) ...)      ; look at expression, decide what to do
;;;;   (define (apply proc args) ...)   ; execute a procedure, return a value
;;;;
;;;; Here:
;;;;
;;;;   eval  = call the model with the conversation (environment)
;;;;   apply = dispatch a tool with its input
;;;;
;;;; They call each other through the loop. The model produces ToolUse
;;;; (an expression). We dispatch the tool (apply). The result goes
;;;; back into the conversation (environment). The model sees it and
;;;; decides what to do next (eval).
;;;;
;;;; The loop is a COALGEBRA — an anamorphism. It unfolds the
;;;; conversation from a seed (the initial messages) one step at a time,
;;;; coinductively, until a termination condition fires.
;;;;
;;;;   step : State → Either<Outcome, State>
;;;;   unfold : State → step → step → ... → Outcome
;;;;
;;;; Maps to: src-rust/crates/query/src/lib.rs:406-1051 (run_query_loop)
;;;; SICP parallel: Section 4.1 — The Metacircular Evaluator

;;; ─────────────────────────────────────────────
;;; Outcomes — the terminal values of the coalgebra
;;; ─────────────────────────────────────────────
;;
;; When the loop terminates, it produces one of these.
;; This is the Left side of Either<Outcome, State>.
;; In Rust: enum QueryOutcome { EndTurn, MaxTokens, Cancelled, Error, BudgetExceeded }

(define (make-outcome type message environment)
  (list 'outcome type message environment))

(define (outcome? x)
  (and (pair? x) (eq? (car x) 'outcome)))

(define (outcome-type o)        (list-ref o 1))
(define (outcome-message o)     (list-ref o 2))
(define (outcome-environment o) (list-ref o 3))

;;; ─────────────────────────────────────────────
;;; agent-step — one step of the coalgebra
;;; ─────────────────────────────────────────────
;;
;; Given the current environment (conversation), call the model (eval),
;; look at the stop reason, and either:
;;   - Return an Outcome (terminal — the Left branch)
;;   - Execute tools and return a new, longer environment (continue — the Right branch)
;;
;; This is the step function:
;;   State → Either<Outcome, State>

(define (agent-step env mock-model tool-registry)
  ;; ── EVAL: call the model ──
  ;; The model is a function: (environment, tools) → stream-of-events
  ;; We fold the stream into a Message. (The algebra inside the coalgebra.)
  (let* ((events (mock-model env tool-registry))
         (result (fold-stream events))
         (msg    (stream-result-message result))
         (stop   (stream-result-stop-reason result))
         ;; Append the assistant's message to the conversation
         (env+msg (append env (list msg))))

    (cond
      ;; ── END TURN: the model is done talking ──
      ;; Terminal. Return an outcome.
      ((string=? stop "end_turn")
       (make-outcome 'end-turn msg env+msg))

      ;; ── TOOL USE: the model wants to act on the world ──
      ;; Extract tool-use blocks, dispatch each one, collect results,
      ;; append results to the conversation, and return the new state.
      ((string=? stop "tool_use")
       (let* ((tool-uses (get-tool-uses msg))
              ;; ── APPLY: dispatch each tool ──
              (result-blocks
                (map (lambda (tu)
                       ;; Look up the tool, execute it, wrap the result
                       (let* ((name   (tool-use-name tu))
                              (input  (tool-use-input tu))
                              (result (dispatch-tool tool-registry name input '()))
                              (content (tool-result-content result))
                              (err?    (tool-result-error? result)))
                         ;; Wrap as a ContentBlock::ToolResult
                         ;; (re-using the types from 01-types.scm)
                         (make-tool-result-block (tool-use-id tu) content err?)))
                     tool-uses))
              ;; Package tool results as a user message
              (results-msg (make-message 'user result-blocks))
              ;; The new environment: original + assistant msg + results
              (new-env (append env+msg (list results-msg))))
         ;; Return the new state (Right branch — continue the loop)
         new-env))

      ;; ── UNKNOWN: treat as end turn ──
      (else
       (make-outcome 'end-turn msg env+msg)))))

;;; ─────────────────────────────────────────────
;;; run-agent-loop — the unfold
;;; ─────────────────────────────────────────────
;;
;; Drive `agent-step` repeatedly until an outcome is reached
;; or max-turns is exceeded.
;;
;; This is the anamorphism:
;;   unfold step seed = case (step seed) of
;;     Left outcome  → outcome
;;     Right state   → unfold step state
;;
;; In Rust, this is the `loop { ... }` in run_query_loop.
;; In Scheme, it's tail recursion. Same thing.

(define (run-agent-loop mock-model tool-registry env max-turns)
  (let loop ((state env)
             (turn 0))
    (if (>= turn max-turns)
        ;; Guard: max turns reached. Prevent infinite loops.
        ;; In Rust: if turn > config.max_turns { return EndTurn }
        (make-outcome 'max-turns
                      (make-assistant-msg
                        (list (make-text-block "Max turns reached.")))
                      state)
        ;; Take one step
        (let ((result (agent-step state mock-model tool-registry)))
          (if (outcome? result)
              ;; Terminal — we're done
              result
              ;; Continue — result is the new state
              (loop result (+ turn 1)))))))

;;; ─────────────────────────────────────────────
;;; Mock models
;;; ─────────────────────────────────────────────
;;
;; A mock model is a function:
;;   (environment, tool-registry) → list-of-stream-events
;;
;; It pattern-matches on the conversation to decide what to respond.
;; This makes explicit what the real API hides: eval is a function
;; from environment to expression.

;; ── simple-mock-model ──
;; Handles greetings with text, actionable requests with a single tool call.
;; After receiving tool results, responds with a summary.
(define (simple-mock-model env tool-registry)
  (let* ((last-msg (last-element env))
         (last-text (get-message-text last-msg)))
    (cond
      ;; If the last message contains tool results, summarize
      ((has-tool-results? last-msg)
       (make-text-stream "Here's what I found based on the tool results."))

      ;; Requests about files → use Bash tool (check BEFORE greetings
      ;; because "this" contains "hi" — a lesson in pattern specificity!)
      ((or (string-contains last-text "file")
           (string-contains last-text "list")
           (string-contains last-text "List"))
       (make-tool-use-stream "id-auto-1" "Bash" '((command . "ls"))))

      ;; Greetings → just respond
      ((or (string-contains last-text "Hello")
           (string-contains last-text "Hi,")
           (string-contains last-text "hello")
           (string-contains last-text "Hello!"))
       (make-text-stream "Hello! How can I help you today?"))

      ;; Default → just respond
      (else
       (make-text-stream "I understand. Let me help with that.")))))

;; ── always-tool-model ──
;; Always requests a tool. Used to test max-turns guard.
(define (always-tool-model env tool-registry)
  (make-tool-use-stream "id-loop" "Bash" '((command . "echo loop"))))

;; ── multi-step-mock-model ──
;; Simulates a multi-step workflow: grep → read → summarize
(define (multi-step-mock-model env tool-registry)
  (let* ((last-msg (last-element env))
         (last-text (get-message-text last-msg))
         (turn-count (length env)))
    (cond
      ;; First turn: grep for TODOs
      ((= turn-count 1)
       (make-tool-use-stream "id-grep" "Grep" '((pattern . "TODO"))))

      ;; After grep results: read the worst file
      ((and (has-tool-results? last-msg)
            (< turn-count 5))
       (make-tool-use-stream "id-read" "Read" '((file_path . "src/main.rs"))))

      ;; After read results: summarize
      (else
       (make-text-stream "I found 3 TODOs. The most critical is in src/main.rs: error handling is missing.")))))

;;; ─────────────────────────────────────────────
;;; Stream builders for mock models
;;; ─────────────────────────────────────────────
;;
;; These build the list-of-events that a mock model returns.
;; They simulate what the real API would stream over SSE.

(define (make-text-stream text)
  (list (make-message-start "mock-msg" "claude-mock")
        (make-content-block-start 0 (make-text-block ""))
        (make-text-delta 0 text)
        (make-content-block-stop 0)
        (make-message-delta "end_turn")
        (make-message-stop)))

(define (make-tool-use-stream id name input)
  (list (make-message-start "mock-msg" "claude-mock")
        (make-content-block-start 0 (make-text-block ""))
        (make-text-delta 0 "Let me check.")
        (make-content-block-stop 0)
        (make-content-block-start 1 (make-tool-use id name '()))
        (make-input-json-delta 1 (write-to-string input))
        (make-content-block-stop 1)
        (make-message-delta "tool_use")
        (make-message-stop)))

;;; ─────────────────────────────────────────────
;;; Helpers
;;; ─────────────────────────────────────────────

(define (last-element lst)
  (if (null? (cdr lst))
      (car lst)
      (last-element (cdr lst))))

;; Extract all text from a message's text blocks
(define (get-message-text msg)
  (let ((texts (filter text-block? (message-blocks msg))))
    (if (null? texts)
        ""
        (fold-left (lambda (acc b) (string-append acc (text-block-text b)))
                   "" texts))))

;; Check if a message contains any tool-result blocks
(define (has-tool-results? msg)
  (let ((blocks (message-blocks msg)))
    (any? tool-result? blocks)))

(define (any? pred lst)
  (cond ((null? lst) #f)
        ((pred (car lst)) #t)
        (else (any? pred (cdr lst)))))

;; write-to-string for the stream builders
;; (also defined in prelude, but we need it here too for standalone loading)
(define (write-to-string obj)
  (let ((port (open-output-string)))
    (write obj port)
    (get-output-string port)))
