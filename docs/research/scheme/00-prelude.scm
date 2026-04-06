;;;; 00-prelude.scm — Test framework and shared utilities
;;;;
;;;; In the tradition of SICP, we build our tools from nothing.
;;;; This file provides just enough scaffolding to write tests
;;;; that read as executable specifications.

(import (scheme base)
        (scheme write)
        (scheme read)
        (scheme process-context)
        (scheme file)
        (scheme load))

;;; ─────────────────────────────────────────────
;;; Test framework (15 lines that do everything)
;;; ─────────────────────────────────────────────

(define *tests-passed* 0)
(define *tests-failed* 0)

(define (run-test name thunk)
  (guard (exn
          (#t (set! *tests-failed* (+ *tests-failed* 1))
              (display "  FAIL: ") (display name)
              (display " — ")
              ;; error objects carry a message; other exceptions just get printed
              (if (error-object? exn)
                  (display (error-object-message exn))
                  (write exn))
              (newline)))
    (thunk)
    (set! *tests-passed* (+ *tests-passed* 1))
    (display "  pass: ") (display name) (newline)))

(define (assert-equal actual expected msg)
  (when (not (equal? actual expected))
    (error (string-append msg
             "\n    expected: " (write-to-string expected)
             "\n    got:      " (write-to-string actual)))))

(define (assert-true val msg)
  (when (not val) (error msg)))

(define (assert-false val msg)
  (when val (error (string-append msg " — expected #f, got truthy"))))

(define (test-summary)
  (newline)
  (display (string-append
    (number->string *tests-passed*) " passed, "
    (number->string *tests-failed*) " failed"))
  (newline)
  (if (> *tests-failed* 0) (exit 1)))

;;; ─────────────────────────────────────────────
;;; Display helpers
;;; ─────────────────────────────────────────────

(define (display-section title)
  (newline)
  (display (string-append "── " title " ──"))
  (newline))

(define (write-to-string obj)
  (let ((port (open-output-string)))
    (write obj port)
    (get-output-string port)))

;;; ─────────────────────────────────────────────
;;; Portable fold-left
;;; (R7RS has it in (scheme list), but we define
;;; our own so there are zero imports needed.)
;;; ─────────────────────────────────────────────

(define (fold-left f init lst)
  (if (null? lst)
      init
      (fold-left f (f init (car lst)) (cdr lst))))

(define (filter pred lst)
  (cond ((null? lst) '())
        ((pred (car lst)) (cons (car lst) (filter pred (cdr lst))))
        (else (filter pred (cdr lst)))))
