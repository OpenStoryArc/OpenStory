;;;; 02-stream.scm — The inner fold: stream accumulation (the algebra)
;;;;
;;;; When the model responds, it doesn't arrive all at once. It streams
;;;; as a sequence of Server-Sent Events (SSE): small deltas that say
;;;; "append these characters to block 0" or "start a new tool-use block."
;;;;
;;;; The StreamAccumulator consumes these events one at a time and builds
;;;; up the complete Message. This is a FOLD — a catamorphism:
;;;;
;;;;   fold-left on-event empty-accumulator events → finished-message
;;;;
;;;; Structure in, value out. The most fundamental operation in
;;;; functional programming, and it's sitting right at the heart
;;;; of an AI agent's streaming layer.
;;;;
;;;; Maps to: src-rust/crates/api/src/lib.rs:186-248 (StreamEvent, ContentDelta)
;;;;          src-rust/crates/api/src/lib.rs:829-978 (StreamAccumulator)
;;;; SICP parallel: Section 2.2.3 — Sequences as Conventional Interfaces
;;;;                (fold as the universal accumulation pattern)

;;; ─────────────────────────────────────────────
;;; Stream event constructors
;;; ─────────────────────────────────────────────
;;
;; Each event is a tagged list, just like ContentBlocks.
;; The Rust enum StreamEvent has variants; we have tags.

(define (make-message-start id model)
  (list 'message-start id model))

(define (make-content-block-start index block)
  (list 'content-block-start index block))

(define (make-text-delta index text)
  (list 'text-delta index text))

(define (make-input-json-delta index fragment)
  (list 'input-json-delta index fragment))

(define (make-thinking-delta index text)
  (list 'thinking-delta index text))

(define (make-content-block-stop index)
  (list 'content-block-stop index))

(define (make-message-delta stop-reason)
  (list 'message-delta stop-reason))

(define (make-message-stop)
  (list 'message-stop))

;; Accessors for delta events
(define (delta-index e)  (list-ref e 1))
(define (delta-text e)   (list-ref e 2))

;;; ─────────────────────────────────────────────
;;; The Accumulator — state threaded through the fold
;;; ─────────────────────────────────────────────
;;
;; In Rust, StreamAccumulator is a struct with fields:
;;   content_blocks, partials, stop_reason, usage
;;
;; Here it's an alist. We use association lists because they're
;; the simplest possible key-value store — no imports, no magic.
;; This is the "carrier" of the algebra.

(define (make-empty-accumulator)
  (list (cons 'blocks '())       ; completed content blocks (vector-like list)
        (cons 'partials '())     ; index → partial string (for json deltas)
        (cons 'stop-reason #f)))

;; Accessors — look up a key in the alist
(define (acc-get key acc)
  (cdr (assq key acc)))

(define (acc-blocks acc)      (acc-get 'blocks acc))
(define (acc-stop-reason acc) (acc-get 'stop-reason acc))

;; "Update" — return a new alist with one key replaced.
;; This is functional update. No mutation.
(define (acc-set key val acc)
  (map (lambda (pair)
         (if (eq? (car pair) key)
             (cons key val)
             pair))
       acc))

;;; ─────────────────────────────────────────────
;;; Helper: update the block at a given index
;;; ─────────────────────────────────────────────

;; Replace the element at `index` in `lst` by applying `f` to it.
(define (list-update lst index f)
  (if (= index 0)
      (cons (f (car lst)) (cdr lst))
      (cons (car lst) (list-update (cdr lst) (- index 1) f))))

;; Get or default from an alist (for partials)
(define (alist-ref key alist default)
  (let ((pair (assv key alist)))
    (if pair (cdr pair) default)))

;; Set a key in an alist (functional update)
(define (alist-set key val alist)
  (cons (cons key val)
        (filter (lambda (p) (not (eqv? (car p) key))) alist)))

;;; ─────────────────────────────────────────────
;;; The algebra: on-stream-event
;;; ─────────────────────────────────────────────
;;
;; This is the step function of the fold:
;;   (accumulator, event) → accumulator
;;
;; Each case matches one StreamEvent variant from the Rust code.
;; The accumulator grows monotonically — deltas only add, never remove.

(define (on-stream-event acc event)
  (let ((tag (car event)))
    (cond
      ;; message-start: initialize (we already have our empty accumulator)
      ((eq? tag 'message-start)
       acc)

      ;; content-block-start: append a new block to the blocks list
      ((eq? tag 'content-block-start)
       (let ((block (list-ref event 2)))
         (acc-set 'blocks
                  (append (acc-blocks acc) (list block))
                  acc)))

      ;; text-delta: append text to the block at the given index
      ((eq? tag 'text-delta)
       (let ((idx (delta-index event))
             (text (delta-text event)))
         (acc-set 'blocks
                  (list-update (acc-blocks acc) idx
                    (lambda (b)
                      (make-text-block
                        (string-append (text-block-text b) text))))
                  acc)))

      ;; input-json-delta: accumulate partial input string
      ;; In Rust, tool-use input arrives as JSON fragments.
      ;; We can't parse until all fragments are concatenated.
      ;; Store them in 'partials' keyed by block index.
      ((eq? tag 'input-json-delta)
       (let* ((idx (list-ref event 1))
              (fragment (list-ref event 2))
              (partials (acc-get 'partials acc))
              (existing (alist-ref idx partials ""))
              (updated (alist-set idx (string-append existing fragment) partials)))
         (acc-set 'partials updated acc)))

      ;; thinking-delta: append to the thinking block
      ((eq? tag 'thinking-delta)
       (let ((idx (delta-index event))
             (text (delta-text event)))
         (acc-set 'blocks
                  (list-update (acc-blocks acc) idx
                    (lambda (b)
                      (make-thinking-block
                        (string-append (thinking-block-text b) text)
                        (thinking-block-signature b))))
                  acc)))

      ;; content-block-stop: block is done (no-op for now)
      ((eq? tag 'content-block-stop)
       acc)

      ;; message-delta: captures the stop reason
      ((eq? tag 'message-delta)
       (acc-set 'stop-reason (list-ref event 1) acc))

      ;; message-stop: stream is done (no-op, finish extracts the result)
      ((eq? tag 'message-stop)
       acc)

      ;; Unknown event — ignore (forward compatibility)
      (else acc))))

;;; ─────────────────────────────────────────────
;;; Finish: crystallize the accumulator into a Message
;;; ─────────────────────────────────────────────
;;
;; This is the final step after the fold completes.
;; Resolve any partial JSON fragments into proper tool-use inputs,
;; then wrap everything in an assistant Message.
;;
;; In Rust: StreamAccumulator::finish() → (Message, UsageInfo, Option<String>)

(define (finish-accumulator acc)
  (let* ((blocks (acc-blocks acc))
         (partials (acc-get 'partials acc))
         ;; Resolve partials: for each tool-use block, if there's a
         ;; partial string at its index, parse it and replace the input.
         (resolved (resolve-partials blocks partials 0)))
    (list (make-assistant-msg resolved)
          (acc-stop-reason acc))))

;; Walk blocks, replacing tool-use inputs with parsed partials
(define (resolve-partials blocks partials idx)
  (if (null? blocks)
      '()
      (let* ((block (car blocks))
             (rest (resolve-partials (cdr blocks) partials (+ idx 1))))
        (if (and (tool-use? block)
                 (assv idx partials))
            ;; Parse the accumulated string as a Scheme alist
            ;; (In the real system this would be JSON.parse)
            (let* ((partial-str (cdr (assv idx partials)))
                   (parsed (read (open-input-string partial-str))))
              (cons (make-tool-use (tool-use-id block)
                                  (tool-use-name block)
                                  parsed)
                    rest))
            (cons block rest)))))

;;; ─────────────────────────────────────────────
;;; The fold itself
;;; ─────────────────────────────────────────────
;;
;; This is it. The entire streaming layer in one line:
;;
;;   fold-left on-stream-event empty-accumulator events
;;
;; Then finish. That's the algebra.

(define (fold-stream events)
  (finish-accumulator
    (fold-left on-stream-event (make-empty-accumulator) events)))

;;; ─────────────────────────────────────────────
;;; Stream result accessors
;;; ─────────────────────────────────────────────

(define (stream-result-message result) (car result))
(define (stream-result-stop-reason result) (cadr result))
