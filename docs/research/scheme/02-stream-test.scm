;;;; 02-stream-test.scm — Tests for the inner fold (the algebra)
;;;;
;;;; The stream accumulator takes a sequence of SSE delta events and
;;;; folds them into a complete Message. This is a catamorphism:
;;;;
;;;;   F(A) → A
;;;;   (accumulator, event) → accumulator
;;;;
;;;; The entire stream is consumed by fold-left. Structure in, value out.
;;;;
;;;; Maps to: src-rust/crates/api/src/lib.rs (StreamAccumulator)
;;;; SICP parallel: Section 2.2.3 — Sequences as Conventional Interfaces

(import (scheme base)
        (scheme write)
        (scheme process-context)
        (scheme load))

(load "00-prelude.scm")
(load "01-types.scm")
(load "02-stream.scm")

(display-section "Stream Events — the delta alphabet")

;; In the Rust code, SSE events arrive as bytes over HTTP.
;; SseLineParser parses them into StreamEvent variants.
;; Here we model them as tagged lists — same as ContentBlocks.

(run-test "text delta event construction"
  (lambda ()
    (let ((e (make-text-delta 0 "hello")))
      (assert-equal (car e) 'text-delta "tag is text-delta")
      (assert-equal (delta-index e) 0 "index is 0")
      (assert-equal (delta-text e) "hello" "text is hello"))))

(display-section "The Empty Accumulator — the initial state")

(run-test "empty accumulator has no blocks"
  (lambda ()
    (let ((acc (make-empty-accumulator)))
      (assert-equal (acc-blocks acc) '() "no blocks")
      (assert-false (acc-stop-reason acc) "no stop reason"))))

(display-section "Folding a simple text stream")

;; A model that just says "Hello world!" arrives as:
;;   message-start → block-start(text) → delta("Hello ") → delta("world!") → block-stop → message-delta → message-stop
;; We fold this into a single text block.

(run-test "fold a simple text response"
  (lambda ()
    (let* ((events (list
                     (make-message-start "msg-1" "claude-mock")
                     (make-content-block-start 0 (make-text-block ""))
                     (make-text-delta 0 "Hello ")
                     (make-text-delta 0 "world!")
                     (make-content-block-stop 0)
                     (make-message-delta "end_turn")
                     (make-message-stop)))
           (result (fold-stream events))
           (msg    (stream-result-message result))
           (stop   (stream-result-stop-reason result)))
      (assert-equal (message-role msg) 'assistant "role is assistant")
      (assert-equal (length (message-blocks msg)) 1 "one block")
      (assert-true (text-block? (car (message-blocks msg))) "it's a text block")
      (assert-equal (text-block-text (car (message-blocks msg)))
                    "Hello world!" "text was concatenated")
      (assert-equal stop "end_turn" "stop reason is end_turn"))))

(display-section "Folding a tool-use stream")

;; When the model wants to use a tool, the stream looks different.
;; The tool input arrives as partial JSON fragments:
;;   block-start(tool-use) → json-delta('{"com') → json-delta('mand":"ls"}') → block-stop
;; The accumulator must concatenate the fragments and parse at the end.

(run-test "fold a tool-use response"
  (lambda ()
    (let* ((events (list
                     (make-message-start "msg-2" "claude-mock")
                     (make-content-block-start 0
                       (make-text-block ""))
                     (make-text-delta 0 "Let me check.")
                     (make-content-block-stop 0)
                     (make-content-block-start 1
                       (make-tool-use "id-1" "Bash" '()))
                     (make-input-json-delta 1 "((command ")
                     (make-input-json-delta 1 ". \"ls\"))")
                     (make-content-block-stop 1)
                     (make-message-delta "tool_use")
                     (make-message-stop)))
           (result (fold-stream events))
           (msg    (stream-result-message result))
           (stop   (stream-result-stop-reason result))
           (blocks (message-blocks msg)))
      (assert-equal (length blocks) 2 "two blocks: text + tool-use")
      (assert-true (text-block? (car blocks)) "first is text")
      (assert-equal (text-block-text (car blocks)) "Let me check." "text content")
      (assert-true (tool-use? (cadr blocks)) "second is tool-use")
      (assert-equal (tool-use-name (cadr blocks)) "Bash" "tool name is Bash")
      ;; Input was accumulated from json deltas and parsed
      (assert-equal (tool-use-input (cadr blocks))
                    '((command . "ls")) "parsed tool input")
      (assert-equal stop "tool_use" "stop reason is tool_use"))))

(display-section "Folding a thinking + text stream")

;; Extended thinking arrives before the text response.

(run-test "fold a thinking response"
  (lambda ()
    (let* ((events (list
                     (make-message-start "msg-3" "claude-mock")
                     (make-content-block-start 0
                       (make-thinking-block "" ""))
                     (make-thinking-delta 0 "Let me reason ")
                     (make-thinking-delta 0 "about this...")
                     (make-content-block-stop 0)
                     (make-content-block-start 1
                       (make-text-block ""))
                     (make-text-delta 1 "The answer is 42.")
                     (make-content-block-stop 1)
                     (make-message-delta "end_turn")
                     (make-message-stop)))
           (result (fold-stream events))
           (msg    (stream-result-message result))
           (blocks (message-blocks msg)))
      (assert-equal (length blocks) 2 "two blocks: thinking + text")
      (assert-true (thinking-block? (car blocks)) "first is thinking")
      (assert-equal (thinking-block-text (car blocks))
                    "Let me reason about this..." "thinking concatenated")
      (assert-true (text-block? (cadr blocks)) "second is text")
      (assert-equal (text-block-text (cadr blocks))
                    "The answer is 42." "text content"))))

(display-section "The fold is pure — same input, same output")

(run-test "folding the same stream twice gives the same result"
  (lambda ()
    (let ((events (list
                    (make-message-start "msg-4" "claude-mock")
                    (make-content-block-start 0 (make-text-block ""))
                    (make-text-delta 0 "deterministic")
                    (make-content-block-stop 0)
                    (make-message-delta "end_turn")
                    (make-message-stop))))
      (let ((r1 (fold-stream events))
            (r2 (fold-stream events)))
        (assert-equal (stream-result-message r1)
                      (stream-result-message r2)
                      "same input → same output (referential transparency)")))))

(test-summary)
