;;;; 07-simulation.scm — The Full Simulation
;;;;
;;;; This is the capstone. We load every layer and run a complete
;;;; multi-turn simulated conversation through the agent loop.
;;;;
;;;; Watch the eval-apply cycle unfold:
;;;;   User asks a question
;;;;     → Model decides to use a tool (eval → expression)
;;;;       → Tool executes and returns (apply → value)
;;;;         → Model sees result, uses another tool
;;;;           → Eventually model responds with text
;;;;
;;;; This is SICP's metacircular evaluator made concrete,
;;;; with an LLM as the eval function and tool dispatch as apply.

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

;;; ─────────────────────────────────────────────
;;; Display helpers for the simulation
;;; ─────────────────────────────────────────────

(define (display-env env)
  (let loop ((msgs env) (i 1))
    (when (not (null? msgs))
      (let* ((msg (car msgs))
             (role (message-role msg))
             (blocks (message-blocks msg)))
        (display "  ") (display i) (display ". [")
        (display role) (display "] ")
        (for-each (lambda (b)
                    (cond
                      ((text-block? b)
                       (let ((t (text-block-text b)))
                         (if (> (string-length t) 60)
                             (begin (display (substring t 0 60)) (display "..."))
                             (display t))))
                      ((tool-use? b)
                       (display "{tool-use: ") (display (tool-use-name b))
                       (display " ") (write (tool-use-input b)) (display "}"))
                      ((tool-result? b)
                       (let ((c (tool-result-block-content b)))
                         (display "{result: ")
                         (if (> (string-length c) 40)
                             (begin (display (substring c 0 40)) (display "..."))
                             (display c))
                         (display "}")))
                      ((thinking-block? b)
                       (display "{thinking...}"))
                      (else (display "{?}"))))
                  blocks)
        (newline)
        (loop (cdr msgs) (+ i 1))))))

(define (display-divider title)
  (newline)
  (display "╔══════════════════════════════════════════════════════╗") (newline)
  (display "║  ") (display title)
  ;; pad to width
  (let ((pad (- 52 (string-length title))))
    (display (make-string (max 0 pad) #\space)))
  (display "║") (newline)
  (display "╚══════════════════════════════════════════════════════╝") (newline))

;;; ═══════════════════════════════════════════════════════
;;; SIMULATION 1: Simple Tool-Using Conversation
;;; ═══════════════════════════════════════════════════════

(display-divider "Simulation 1: Tool-Using Conversation")

(display "\nA user asks about files. The model uses the Bash tool,") (newline)
(display "sees the results, and summarizes them.") (newline)
(display "This demonstrates the eval-apply cycle:") (newline)
(display "  eval (model) → tool_use → apply (Bash) → eval (model) → text") (newline)

(let* ((env (list (make-user-msg "What files are in this project?")))
       (result (run-agent-loop simple-mock-model default-mock-registry env 10)))

  (newline)
  (display "─── Conversation trace ───") (newline)
  (display-env (outcome-environment result))

  (newline)
  (display "Outcome: ") (display (outcome-type result)) (newline)
  (display "Turns: ") (display (length (outcome-environment result))) (newline))

;;; ═══════════════════════════════════════════════════════
;;; SIMULATION 2: Multi-Step Tool Chain
;;; ═══════════════════════════════════════════════════════

(display-divider "Simulation 2: Multi-Step Tool Chain")

(display "\nThe model chains tools: Grep → Read → Summarize.") (newline)
(display "Each tool result feeds the next eval step.") (newline)
(display "The coalgebra unfolds: step → step → step → outcome.") (newline)

(let* ((env (list (make-user-msg "Find all TODOs and show me the worst one")))
       (result (run-agent-loop multi-step-mock-model default-mock-registry env 10)))

  (newline)
  (display "─── Conversation trace ───") (newline)
  (display-env (outcome-environment result))

  (newline)
  (display "Outcome: ") (display (outcome-type result)) (newline)
  (display "Turns: ") (display (length (outcome-environment result))) (newline))

;;; ═══════════════════════════════════════════════════════
;;; SIMULATION 3: Max-Turns Guard (Infinite Loop Prevention)
;;; ═══════════════════════════════════════════════════════

(display-divider "Simulation 3: Max-Turns Guard")

(display "\nA model that always requests tools would loop forever.") (newline)
(display "The max-turns guard is the termination condition of") (newline)
(display "the coalgebra — without it, the unfold never stops.") (newline)

(let* ((env (list (make-user-msg "Do something")))
       (result (run-agent-loop always-tool-model default-mock-registry env 3)))

  (newline)
  (display "─── Conversation trace ───") (newline)
  (display-env (outcome-environment result))

  (newline)
  (display "Outcome: ") (display (outcome-type result)) (newline)
  (display "Stopped by: max-turns guard (3 turns)") (newline))

;;; ═══════════════════════════════════════════════════════
;;; SIMULATION 4: Sub-Agent Delegation (Compound Procedures)
;;; ═══════════════════════════════════════════════════════

(display-divider "Simulation 4: Sub-Agent (Compound Procedure)")

(display "\nThe model delegates to a sub-agent via the Agent tool.") (newline)
(display "This creates a NESTED eval-apply loop with fresh scope.") (newline)
(display "Like SICP's compound procedure: eval calls apply calls eval.") (newline)

(let* ((agent (make-agent-tool simple-mock-model default-mock-registry))
       (registry-with-agent
         (make-tool-registry
           (cons agent (list mock-bash-tool mock-read-tool mock-grep-tool))))
       ;; A model that delegates once, then summarizes
       (delegating-model
         (lambda (env tools)
           (let ((last-msg (last-element env)))
             (if (has-tool-results? last-msg)
                 (make-text-stream "Based on the sub-agent's research, the answer is 42.")
                 (make-tool-use-stream "id-agent" "Agent"
                   '((description . "research task")
                     (prompt . "List the files in the project")))))))
       (env (list (make-user-msg "Delegate: find out what files we have")))
       (result (run-agent-loop delegating-model registry-with-agent env 5)))

  (newline)
  (display "─── Conversation trace (outer agent) ───") (newline)
  (display-env (outcome-environment result))

  (newline)
  (display "Outcome: ") (display (outcome-type result)) (newline)
  (display "(The sub-agent ran its own loop invisibly inside step 2)") (newline))

;;; ═══════════════════════════════════════════════════════
;;; SIMULATION 5: Context Compaction (Garbage Collection)
;;; ═══════════════════════════════════════════════════════

(display-divider "Simulation 5: Context Compaction (GC)")

(display "\nWhen the conversation gets too long, we compact it.") (newline)
(display "Old messages are summarized. Recent ones are kept.") (newline)
(display "An algebra applied to the coalgebra's trace.") (newline)

(let* (;; Build a long conversation
       (big-text (make-string 800 #\x))
       (env (list (make-user-msg "First question — a long time ago")
                  (make-assistant-msg (list (make-text-block big-text)))
                  (make-user-msg "Second question — also old")
                  (make-assistant-msg (list (make-text-block big-text)))
                  (make-user-msg "Third question — getting old")
                  (make-assistant-msg (list (make-text-block big-text)))
                  (make-user-msg "Recent question — keep this!")
                  (make-assistant-msg (list (make-text-block "Recent answer"))))))

  (display "\nBefore compaction:") (newline)
  (display "  Messages: ") (display (length env)) (newline)
  (display "  Estimated tokens: ") (display (estimate-tokens env)) (newline)

  (let ((compacted (compact-if-needed env 300 mock-summarizer)))
    (display "\nAfter compaction (threshold: 300 tokens):") (newline)
    (display "  Messages: ") (display (length compacted)) (newline)
    (display "  Estimated tokens: ") (display (estimate-tokens compacted)) (newline)

    (newline)
    (display "─── Compacted conversation ───") (newline)
    (display-env compacted)))

;;; ═══════════════════════════════════════════════════════
;;; THE BIG PICTURE
;;; ═══════════════════════════════════════════════════════

(display-divider "The Big Picture")

(display "
What you just saw:

  1. TYPES (01): Tagged lists as sum types. ContentBlock is the
     universal expression — text, tool-use, tool-result, thinking.

  2. FOLD (02): The inner algebra. A stream of SSE deltas
     collapses into a Message. Structure in, value out.

  3. APPLY (03): Tool dispatch. The model names a tool,
     we look it up and call it. Procedure application.

  4. EVAL-APPLY (04): The coalgebra. The outer loop unfolds
     the conversation: eval → apply → eval → ... → outcome.
     Tail recursion IS the anamorphism.

  5. COMPACT (05): Garbage collection. When the conversation
     grows too long, we fold it back down. An algebra applied
     to the coalgebra's trace. Self-referential.

  6. AGENT (06): Compound procedures. A tool that spawns a
     nested eval-apply loop. The evaluator evaluating itself.

  This is SICP's metacircular evaluator.
  The model is eval. Tools are apply. Messages are the environment.
  ContentBlock is the expression. The conversation is the computation.

  And the fixed point at the center:

    ToolUse ──execute──▶ ToolResult ──api──▶ ToolUse ──...
        │                                        │
        └──── same type ◀────────────────────────┘

") (newline)
