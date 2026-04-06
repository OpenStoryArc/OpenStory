;;;; 03-tools-test.scm — Tests for tool dispatch (apply)
;;;;
;;;; In SICP, `apply` takes a procedure and arguments, and executes it.
;;;; Here, tool dispatch takes a tool name and input, finds the matching
;;;; tool in the registry, and calls its execute function.
;;;;
;;;; This is the boundary where computation crosses into the world:
;;;; reading files, running commands, searching code. The tool receives
;;;; a symbolic request and returns a concrete result.
;;;;
;;;; Maps to: src-rust/crates/tools/src/lib.rs (Tool trait, registry)
;;;; SICP parallel: Section 4.1.1 — The Core of the Evaluator (apply)

(import (scheme base)
        (scheme write)
        (scheme process-context)
        (scheme load))

(load "00-prelude.scm")
(load "01-types.scm")
(load "03-tools.scm")

(display-section "Tool construction")

;; A tool is a named procedure: (name, description, execute-fn)
;; In Rust: trait Tool { fn name(), fn description(), fn execute() }
;; In Scheme: just a list with a lambda. No traits needed.

(run-test "make a tool and inspect it"
  (lambda ()
    (let ((t (make-tool "Echo"
               "Echoes its input back"
               (lambda (input ctx) (make-tool-result (cdr (assq 'text input)) #f)))))
      (assert-equal (tool-name t) "Echo" "tool name")
      (assert-equal (tool-description t) "Echoes its input back" "tool description"))))

(display-section "Tool execution")

(run-test "execute a tool directly"
  (lambda ()
    (let* ((t (make-tool "Echo"
                "Echoes input"
                (lambda (input ctx)
                  (make-tool-result (cdr (assq 'text input)) #f))))
           (result (tool-execute t '((text . "hello")) '())))
      (assert-equal (tool-result-content result) "hello" "echoed content")
      (assert-false (tool-result-error? result) "not an error"))))

(display-section "Tool registry and dispatch")

;; The registry is a list of tools. Dispatch finds the right one by name.
;; This is exactly `assoc` — the simplest possible lookup.
;; In SICP terms: the registry is an environment frame where
;; tool names are bound to their implementations.

(run-test "dispatch finds the right tool"
  (lambda ()
    (let* ((bash (make-tool "Bash" "Run a command"
                   (lambda (input ctx)
                     (make-tool-result "file1.txt\nfile2.txt" #f))))
           (read-tool (make-tool "Read" "Read a file"
                        (lambda (input ctx)
                          (make-tool-result "fn main() {}" #f))))
           (registry (make-tool-registry (list bash read-tool)))
           (result (dispatch-tool registry "Bash" '((command . "ls")) '())))
      (assert-equal (tool-result-content result)
                    "file1.txt\nfile2.txt"
                    "dispatched to Bash tool"))))

(run-test "dispatch unknown tool returns error"
  (lambda ()
    (let* ((registry (make-tool-registry '()))
           (result (dispatch-tool registry "FakeToolXyz" '() '())))
      (assert-true (tool-result-error? result) "should be an error"))))

(display-section "Mock tools — simulated filesystem and shell")

;; These mock tools let us test the full agent loop later
;; without needing a real filesystem or shell.
;; The mock IS the specification — it defines what the tools
;; should return for known inputs.

(run-test "mock-bash executes a command"
  (lambda ()
    (let ((result (tool-execute mock-bash-tool
                    '((command . "ls src/")) '())))
      (assert-false (tool-result-error? result) "not an error")
      ;; Should return something (the mock's canned output)
      (assert-true (string? (tool-result-content result))
                   "returns a string"))))

(run-test "mock-read reads a file"
  (lambda ()
    (let ((result (tool-execute mock-read-tool
                    '((file_path . "src/main.rs")) '())))
      (assert-false (tool-result-error? result) "not an error")
      (assert-true (string? (tool-result-content result))
                   "returns file content"))))

(run-test "mock-read on missing file returns error"
  (lambda ()
    (let ((result (tool-execute mock-read-tool
                    '((file_path . "nonexistent.xyz")) '())))
      (assert-true (tool-result-error? result) "should be an error"))))

(run-test "mock-grep searches for a pattern"
  (lambda ()
    (let ((result (tool-execute mock-grep-tool
                    '((pattern . "TODO")) '())))
      (assert-false (tool-result-error? result) "not an error")
      (assert-true (string? (tool-result-content result))
                   "returns grep output"))))

(display-section "Full mock registry")

(run-test "default-mock-registry dispatches all tools"
  (lambda ()
    (let ((r default-mock-registry))
      ;; Should be able to dispatch to each tool without error
      (assert-false
        (tool-result-error? (dispatch-tool r "Bash" '((command . "ls")) '()))
        "Bash works")
      (assert-false
        (tool-result-error? (dispatch-tool r "Read" '((file_path . "src/main.rs")) '()))
        "Read works")
      (assert-false
        (tool-result-error? (dispatch-tool r "Grep" '((pattern . "fn")) '()))
        "Grep works"))))

(test-summary)
