;;;; 05-compact.scm — Context compaction (garbage collection)
;;;;
;;;; When the conversation grows too long for the model's context window,
;;;; we need to make it shorter without losing critical information.
;;;; This is GARBAGE COLLECTION applied to conversation history.
;;;;
;;;; The analogy to generational GC is precise:
;;;;   - Recent messages = nursery → kept intact (high locality)
;;;;   - Old messages = tenured → collected into a summary (low locality)
;;;;
;;;; The most remarkable thing: compaction uses the SAME MODEL to
;;;; summarize the conversation. The evaluator is examining and
;;;; compressing its own state. This is the self-referential quality
;;;; that makes the metacircular evaluator metacircular.
;;;;
;;;; Maps to: src-rust/crates/query/src/compact.rs
;;;; SICP parallel: Section 5.3 — Storage Allocation and Garbage Collection

;;; ─────────────────────────────────────────────
;;; Token estimation
;;; ─────────────────────────────────────────────
;;
;; A rough heuristic: ~4 characters per token.
;; The real implementation uses the API's token counter,
;; but for our purposes this is close enough.

(define (estimate-tokens env)
  (fold-left (lambda (acc msg)
               (+ acc (estimate-message-tokens msg)))
             0 env))

(define (estimate-message-tokens msg)
  (let ((blocks (message-blocks msg)))
    (fold-left (lambda (acc block)
                 (+ acc (quotient (string-length (block->string block)) 4)))
               0 blocks)))

(define (block->string block)
  (cond ((text-block? block)     (text-block-text block))
        ((tool-use? block)       (string-append "tool:" (tool-use-name block)))
        ((tool-result? block)    (tool-result-block-content block))
        ((thinking-block? block) (thinking-block-text block))
        (else "")))

;;; ─────────────────────────────────────────────
;;; Mock summarizer
;;; ─────────────────────────────────────────────
;;
;; In the real system, this would be an API call to the model:
;;   "Summarize this conversation, preserving all information
;;    needed to continue."
;;
;; Here we just concatenate the first line of each message.
;; The point isn't the summary quality — it's the STRUCTURE:
;; an algebra applied to the coalgebra's trace.

(define (mock-summarizer messages)
  (let ((parts (map (lambda (msg)
                      (let ((text (get-message-text msg)))
                        (if (> (string-length text) 50)
                            (string-append (substring text 0 50) "...")
                            text)))
                    messages)))
    (string-append "[Summary of "
                   (number->string (length messages))
                   " messages] "
                   (fold-left (lambda (acc p)
                                (if (string=? p "")
                                    acc
                                    (string-append acc p "; ")))
                              "" parts))))

;;; ─────────────────────────────────────────────
;;; compact-if-needed
;;; ─────────────────────────────────────────────
;;
;; The compaction algorithm:
;;   1. Estimate token usage
;;   2. If over 80% of max-tokens, split into old/recent
;;   3. Summarize old messages
;;   4. Return (summary-msg . recent-messages)
;;
;; In Rust: auto_compact_if_needed / reactive_compact / context_collapse
;; Three strategies with increasing aggressiveness.
;; We implement the simplest one here.

(define compact-threshold 0.8)  ;; 80% of context window

(define (compact-if-needed env max-tokens summarizer)
  (let ((tokens (estimate-tokens env)))
    (if (< tokens (* compact-threshold max-tokens))
        ;; Under threshold — no compaction needed
        env
        ;; Over threshold — compact!
        (let* ((split-point (find-split-point env))
               (old-msgs    (take-n env split-point))
               (recent-msgs (drop-n env split-point))
               ;; Summarize the old messages
               ;; (In the real system, this is ANOTHER API call)
               (summary-text (summarizer old-msgs))
               (summary-msg  (make-message 'user
                               (list (make-text-block
                                       (string-append
                                         "[conversation compacted]\n"
                                         summary-text))))))
          ;; Return: summary + recent messages
          (cons summary-msg recent-msgs)))))

;; Find where to split: keep the last ~40% of messages
(define (find-split-point env)
  (let ((len (length env)))
    (max 1 (- len (max 2 (quotient (* len 4) 10))))))

;;; ─────────────────────────────────────────────
;;; List utilities
;;; ─────────────────────────────────────────────

(define (take-n lst n)
  (if (or (= n 0) (null? lst))
      '()
      (cons (car lst) (take-n (cdr lst) (- n 1)))))

(define (drop-n lst n)
  (if (or (= n 0) (null? lst))
      lst
      (drop-n (cdr lst) (- n 1))))
