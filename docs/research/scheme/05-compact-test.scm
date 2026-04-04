;;;; 05-compact-test.scm — Tests for context compaction (GC)
;;;;
;;;; When the conversation grows too long, it won't fit in the model's
;;;; context window. Compaction summarizes older messages into a shorter
;;;; form, freeing space for new turns.
;;;;
;;;; This is GARBAGE COLLECTION for conversations:
;;;; - Recent messages = nursery (kept intact)
;;;; - Old messages = tenured (collected into a summary)
;;;;
;;;; And it's self-referential: the system uses its own model to
;;;; summarize its own history. The evaluator examining its own state.
;;;;
;;;; Maps to: src-rust/crates/query/src/compact.rs
;;;; SICP parallel: Section 5.3 — Storage Allocation and Garbage Collection

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

(display-section "Token estimation")

(run-test "estimate-tokens gives rough count"
  (lambda ()
    (let ((env (list (make-user-msg "Hello world"))))
      ;; Rough heuristic: ~4 chars per token
      (let ((tokens (estimate-tokens env)))
        (assert-true (> tokens 0) "should be positive")
        (assert-true (< tokens 100) "should be reasonable for short msg")))))

(run-test "longer conversations have more tokens"
  (lambda ()
    (let ((short (list (make-user-msg "Hi")))
          (long  (list (make-user-msg "Tell me about the architecture of this system")
                       (make-assistant-msg
                         (list (make-text-block (make-string 500 #\x))))
                       (make-user-msg "Now tell me more")
                       (make-assistant-msg
                         (list (make-text-block (make-string 500 #\y)))))))
      (assert-true (> (estimate-tokens long) (estimate-tokens short))
                   "longer conversation → more tokens"))))

(display-section "Compaction decision")

(run-test "no compaction when under threshold"
  (lambda ()
    (let ((env (list (make-user-msg "Hi")))
          (max-tokens 1000))
      (let ((result (compact-if-needed env max-tokens mock-summarizer)))
        ;; Should return the environment unchanged
        (assert-equal (length result) (length env)
                      "environment unchanged when under threshold")))))

(run-test "compaction triggers when over threshold"
  (lambda ()
    ;; Build a conversation that's definitely over the threshold
    (let* ((big-msg (make-string 2000 #\a))
           (env (list (make-user-msg "start")
                      (make-assistant-msg (list (make-text-block big-msg)))
                      (make-user-msg "middle")
                      (make-assistant-msg (list (make-text-block big-msg)))
                      (make-user-msg "end")
                      (make-assistant-msg (list (make-text-block "recent")))))
          (max-tokens 500))  ;; way below the conversation size
      (let ((result (compact-if-needed env max-tokens mock-summarizer)))
        ;; Should be shorter than the original
        (assert-true (< (length result) (length env))
                     "compacted environment is shorter")
        ;; Should still contain the most recent messages
        (assert-true (> (length result) 0)
                     "compacted environment is not empty")))))

(display-section "Compaction preserves recent context")

(run-test "recent messages survive compaction"
  (lambda ()
    (let* ((big-msg (make-string 2000 #\a))
           (env (list (make-user-msg "ancient history")
                      (make-assistant-msg (list (make-text-block big-msg)))
                      (make-user-msg "old stuff")
                      (make-assistant-msg (list (make-text-block big-msg)))
                      (make-user-msg "recent question")
                      (make-assistant-msg (list (make-text-block "recent answer")))))
           (max-tokens 500)
           (result (compact-if-needed env max-tokens mock-summarizer)))
      ;; The last message should be preserved
      (let ((last-msg (last-element result)))
        (assert-equal (get-message-text last-msg) "recent answer"
                      "most recent message preserved")))))

(display-section "Mock summarizer")

(run-test "mock summarizer produces a summary"
  (lambda ()
    (let* ((msgs (list (make-user-msg "What is 2+2?")
                       (make-assistant-msg (list (make-text-block "It's 4.")))))
           (summary (mock-summarizer msgs)))
      (assert-true (string? summary) "summary is a string")
      (assert-true (> (string-length summary) 0) "summary is not empty"))))

(test-summary)
