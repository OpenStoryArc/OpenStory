;;;; 03-tools.scm — Tool dispatch (apply)
;;;;
;;;; In SICP Section 4.1.1, `apply` is one half of the evaluator:
;;;;
;;;;   (define (apply procedure arguments)
;;;;     (cond ((primitive-procedure? procedure)
;;;;            (apply-primitive-procedure procedure arguments))
;;;;           ((compound-procedure? procedure)
;;;;            (eval-sequence (procedure-body procedure) ...))
;;;;           ...))
;;;;
;;;; Tool dispatch is the same thing. The model produces a ToolUse block
;;;; (an expression whose operator is the tool name). We look up the tool
;;;; in a registry (an environment), and call its execute function.
;;;;
;;;; The tool name is the operator.
;;;; The tool input is the operand.
;;;; The context is the environment.
;;;; The result is the value.
;;;;
;;;; Maps to: src-rust/crates/tools/src/lib.rs (Tool trait, ToolResult)
;;;; SICP parallel: Section 4.1.1 — apply

;;; ─────────────────────────────────────────────
;;; Tool abstraction
;;; ─────────────────────────────────────────────
;;
;; In Rust: trait Tool { fn name(&self), fn description(&self),
;;          fn input_schema(&self), fn execute(&self, input, ctx) }
;;
;; In Scheme: a tool is (list 'tool name description execute-fn).
;; A procedure with a name tag. That's all a "trait object" is
;; when you strip away the vtable.

;; Tool execution result — simpler than a ContentBlock::ToolResult
;; because it doesn't carry the tool-use-id yet. That gets attached
;; later when the query loop wraps it into a ToolResult content block.
;; In Rust: struct ToolResult { content: String, is_error: bool }

(define (make-tool-result content is-error)
  (list 'tool-exec-result content is-error))

(define (tool-result-content r) (list-ref r 1))
(define (tool-result-error? r)  (list-ref r 2))

(define (make-tool name description execute-fn)
  (list 'tool name description execute-fn))

(define (tool-name t)        (list-ref t 1))
(define (tool-description t) (list-ref t 2))
(define (tool-execute-fn t)  (list-ref t 3))

;; Execute a tool. This is `apply` for a primitive procedure.
(define (tool-execute tool input ctx)
  ((tool-execute-fn tool) input ctx))

;;; ─────────────────────────────────────────────
;;; Tool registry
;;; ─────────────────────────────────────────────
;;
;; A registry is just a list of tools.
;; Dispatch is just `find` — look up by name.
;;
;; In SICP, the environment is a list of frames, each frame
;; a list of bindings. Our registry is one frame where
;; tool names are bound to tool implementations.

(define (make-tool-registry tools) tools)

(define (dispatch-tool registry name input ctx)
  (let ((tool (find-tool registry name)))
    (if tool
        (tool-execute tool input ctx)
        (make-tool-result
          (string-append "Unknown tool: " name)
          #t))))

(define (find-tool registry name)
  (cond ((null? registry) #f)
        ((string=? (tool-name (car registry)) name)
         (car registry))
        (else (find-tool (cdr registry) name))))

;;; ─────────────────────────────────────────────
;;; Mock filesystem
;;; ─────────────────────────────────────────────
;;
;; A simulated filesystem for testing. The mock tools operate
;; on this instead of the real filesystem. It's an alist of
;; path → content.

(define mock-filesystem
  '(("src/main.rs" .
     "fn main() {\n    // TODO: handle errors properly\n    println!(\"hello\");\n}\n")
    ("src/lib.rs" .
     "pub mod query;\npub mod tools;\n// TODO: add streaming support\n")
    ("src/query.rs" .
     "pub fn run_query_loop() {\n    // TODO: implement compaction\n    loop { break; }\n}\n")
    ("Cargo.toml" .
     "[package]\nname = \"claurst\"\nversion = \"1.0.0\"\n")))

;;; ─────────────────────────────────────────────
;;; Mock tools
;;; ─────────────────────────────────────────────
;;
;; These simulate what the real tools do.
;; Each is a function: (input, ctx) → tool-result
;;
;; The mock IS the specification. It defines the contract
;; that the agent loop depends on.

(define mock-bash-tool
  (make-tool "Bash"
    "Execute a shell command"
    (lambda (input ctx)
      (let ((cmd (cdr (assq 'command input))))
        (cond
          ;; Simulate `ls` by listing mock filesystem paths
          ((or (string=? cmd "ls") (string=? cmd "ls src/"))
           (make-tool-result
             (fold-left (lambda (acc pair)
                          (string-append acc (car pair) "\n"))
                        "" mock-filesystem)
             #f))
          ;; Simulate `grep` as a simple search
          ((string-prefix? "grep " cmd)
           (make-tool-result "src/main.rs:2: // TODO: handle errors" #f))
          ;; Unknown command
          (else
           (make-tool-result (string-append "executed: " cmd) #f)))))))

(define mock-read-tool
  (make-tool "Read"
    "Read a file from the filesystem"
    (lambda (input ctx)
      (let* ((path (cdr (assq 'file_path input)))
             (entry (assoc path mock-filesystem)))
        (if entry
            (make-tool-result (cdr entry) #f)
            (make-tool-result
              (string-append "File not found: " path)
              #t))))))

(define mock-grep-tool
  (make-tool "Grep"
    "Search for a pattern in files"
    (lambda (input ctx)
      (let ((pattern (cdr (assq 'pattern input))))
        ;; Search mock filesystem for lines matching the pattern
        (let ((matches (fold-left
                         (lambda (acc pair)
                           (let ((path (car pair))
                                 (content (cdr pair)))
                             (if (string-contains content pattern)
                                 (string-append acc path ": [match]\n")
                                 acc)))
                         "" mock-filesystem)))
          (if (string=? matches "")
              (make-tool-result "No matches found." #f)
              (make-tool-result matches #f)))))))

;;; ─────────────────────────────────────────────
;;; Default registry with all mock tools
;;; ─────────────────────────────────────────────

(define default-mock-registry
  (make-tool-registry (list mock-bash-tool mock-read-tool mock-grep-tool)))

;;; ─────────────────────────────────────────────
;;; Helpers (used by mock tools)
;;; ─────────────────────────────────────────────

(define (string-prefix? prefix str)
  (and (>= (string-length str) (string-length prefix))
       (string=? (substring str 0 (string-length prefix)) prefix)))

(define (string-contains haystack needle)
  (let ((hlen (string-length haystack))
        (nlen (string-length needle)))
    (let loop ((i 0))
      (cond ((> (+ i nlen) hlen) #f)
            ((string=? (substring haystack i (+ i nlen)) needle) #t)
            (else (loop (+ i 1)))))))
