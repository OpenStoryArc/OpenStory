;;;; 01-types.scm — Core data types
;;;;
;;;; The foundation. Everything in the system flows through these types.
;;;;
;;;; In Rust, ContentBlock is an enum — a sum type with variants like
;;;; Text, ToolUse, ToolResult, Thinking. In Scheme, we use tagged lists:
;;;; the car is the tag, the cdr is the payload. This is exactly SICP's
;;;; "tagged data" from Section 2.4 — the same technique Sussman uses
;;;; to implement a type system without a type system.
;;;;
;;;; Maps to: src-rust/crates/core/src/lib.rs:142-280
;;;; SICP parallel: Section 2.1, 2.4 — Data Abstraction, Tagged Data

;;; ─────────────────────────────────────────────
;;; Content Blocks — the universal expression type
;;; ─────────────────────────────────────────────
;;
;; Why is this one enum the heart of everything?
;;
;; Because the model and the tool system communicate ONLY through
;; ContentBlocks. A model turn produces them. A tool consumes a
;; ToolUse block and produces a ToolResult block. The conversation
;; is a list of messages, each containing a list of these blocks.
;;
;; It's the universal currency. The fixed point around which the
;; entire eval-apply loop revolves.

;; ── Text: what the model says to the user ──

(define (make-text-block text)
  (list 'text text))

(define (text-block? b)
  (and (pair? b) (eq? (car b) 'text)))

(define (text-block-text b)
  (cadr b))

;; ── ToolUse: the model requesting an action ──
;; This is an EXPRESSION in SICP terms — a combination
;; whose operator is the tool name and whose operand is the input.

(define (make-tool-use id name input)
  (list 'tool-use id name input))

(define (tool-use? b)
  (and (pair? b) (eq? (car b) 'tool-use)))

(define (tool-use-id b)    (list-ref b 1))
(define (tool-use-name b)  (list-ref b 2))
(define (tool-use-input b) (list-ref b 3))

;; ── ToolResult: the world's response ──
;; This is a VALUE — the result of evaluating the expression above.
;; The is-error flag distinguishes normal returns from exceptions.

;; Note: "tool-result-block" is the ContentBlock variant (carries tool-use-id).
;; "tool-result" (defined in 03-tools.scm) is the simpler execution result.
;; In Rust these are ContentBlock::ToolResult vs ToolResult — same distinction.

(define (make-tool-result-block tool-use-id content is-error)
  (list 'tool-result tool-use-id content is-error))

(define (tool-result? b)
  (and (pair? b) (eq? (car b) 'tool-result)))

(define (tool-result-block-id b)      (list-ref b 1))
(define (tool-result-block-content b) (list-ref b 2))
(define (tool-result-block-error? b)  (list-ref b 3))

;; ── Thinking: the model's internal reasoning ──
;; Extended thinking with a cryptographic signature.
;; Like showing your work on a math exam.

(define (make-thinking-block thinking signature)
  (list 'thinking thinking signature))

(define (thinking-block? b)
  (and (pair? b) (eq? (car b) 'thinking)))

(define (thinking-block-text b)      (list-ref b 1))
(define (thinking-block-signature b) (list-ref b 2))

;;; ─────────────────────────────────────────────
;;; Messages — a role and a list of content blocks
;;; ─────────────────────────────────────────────
;;
;; A message is one turn in the conversation. Two roles: user, assistant.
;; The content is a LIST of blocks — not a single string! This is key:
;; a single assistant turn can say text AND request tool use simultaneously.
;;
;; In Rust: Message { role: Role, content: MessageContent }
;; In Scheme: (role (block1 block2 ...))

(define (make-message role blocks)
  (list role blocks))

(define (message-role m)   (car m))
(define (message-blocks m) (cadr m))

;; Convenience constructors

(define (make-user-msg text)
  (make-message 'user (list (make-text-block text))))

(define (make-assistant-msg blocks)
  (make-message 'assistant blocks))

;; Extract all tool-use blocks from a message.
;; This is what the query loop calls after each model turn
;; to find out what tools the model wants to invoke.

(define (get-tool-uses msg)
  (filter tool-use? (message-blocks msg)))

;;; ─────────────────────────────────────────────
;;; The Environment
;;; ─────────────────────────────────────────────
;;
;; There is no Environment type. It's just a list of messages.
;; That's the whole point.
;;
;; In SICP, the environment is a chain of frames binding names to values.
;; Here, it's a sequence of turns binding tool calls to their results.
;; When the model "looks up" what happened, it reads the history —
;; same as looking up a binding in the enclosing frame.
;;
;; (list msg1 msg2 msg3 ...)   ; newest last
