;;;; 01-types-test.scm — Tests for the core data types
;;;;
;;;; These tests define what we WANT the types to do.
;;;; Run this before writing 01-types.scm and watch it fail.
;;;; Then implement until it passes.
;;;;
;;;; Maps to: src-rust/crates/core/src/lib.rs (Role, ContentBlock, Message)
;;;; SICP parallel: Section 2.1 — Building Abstractions with Data

(import (scheme base)
        (scheme write)
        (scheme process-context)
        (scheme load))

(load "00-prelude.scm")
(load "01-types.scm")

(display-section "Content Blocks — the universal expression type")

;; In the Rust code, ContentBlock is an enum with variants:
;;   Text, ToolUse, ToolResult, Thinking, ...
;; Each variant carries different data. In Scheme, we use tagged lists.
;; This is the same idea as SICP's "tagged data" from Section 2.4.

(run-test "text block construction"
  (lambda ()
    (let ((b (make-text-block "hello world")))
      (assert-true (text-block? b) "should be a text block")
      (assert-equal (text-block-text b) "hello world" "text content"))))

(run-test "text block is not a tool-use"
  (lambda ()
    (let ((b (make-text-block "hello")))
      (assert-false (tool-use? b) "text block is not tool-use"))))

(run-test "tool-use block construction"
  (lambda ()
    (let ((b (make-tool-use "id-1" "Bash" '((command . "ls -la")))))
      (assert-true (tool-use? b) "should be a tool-use block")
      (assert-equal (tool-use-id b) "id-1" "tool use id")
      (assert-equal (tool-use-name b) "Bash" "tool name")
      (assert-equal (tool-use-input b) '((command . "ls -la")) "tool input"))))

(run-test "tool-result block construction"
  (lambda ()
    (let ((b (make-tool-result-block "id-1" "file1.txt\nfile2.txt" #f)))
      (assert-true (tool-result? b) "should be a tool-result block")
      (assert-equal (tool-result-block-id b) "id-1" "tool use id")
      (assert-equal (tool-result-block-content b) "file1.txt\nfile2.txt" "result content")
      (assert-false (tool-result-block-error? b) "should not be an error"))))

(run-test "tool-result error flag"
  (lambda ()
    (let ((b (make-tool-result-block "id-2" "file not found" #t)))
      (assert-true (tool-result-block-error? b) "should be an error"))))

(run-test "thinking block construction"
  (lambda ()
    (let ((b (make-thinking-block "let me reason..." "sig-abc")))
      (assert-true (thinking-block? b) "should be a thinking block")
      (assert-equal (thinking-block-text b) "let me reason..." "thinking text")
      (assert-equal (thinking-block-signature b) "sig-abc" "signature"))))

(display-section "Messages — role + content blocks")

;; In Rust: Message { role: Role, content: MessageContent }
;; A message is a turn in the conversation. The role says who spoke.
;; The content is a list of ContentBlocks — not just text!
;; This is critical: a single assistant turn can contain BOTH
;; text AND tool-use blocks.

(run-test "user message from text"
  (lambda ()
    (let ((m (make-user-msg "What files are in this directory?")))
      (assert-equal (message-role m) 'user "role is user")
      (assert-equal (length (message-blocks m)) 1 "one block")
      (assert-true (text-block? (car (message-blocks m))) "block is text"))))

(run-test "assistant message with multiple blocks"
  (lambda ()
    (let ((m (make-assistant-msg
               (list (make-text-block "I'll check for you.")
                     (make-tool-use "id-1" "Bash" '((command . "ls")))))))
      (assert-equal (message-role m) 'assistant "role is assistant")
      (assert-equal (length (message-blocks m)) 2 "two blocks"))))

(run-test "extract tool-use blocks from message"
  (lambda ()
    (let* ((m (make-assistant-msg
                (list (make-text-block "Let me look.")
                      (make-tool-use "id-1" "Bash" '((command . "ls")))
                      (make-tool-use "id-2" "Read" '((file_path . "foo.rs"))))))
           (uses (get-tool-uses m)))
      (assert-equal (length uses) 2 "two tool uses")
      (assert-equal (tool-use-name (car uses)) "Bash" "first tool is Bash")
      (assert-equal (tool-use-name (cadr uses)) "Read" "second tool is Read"))))

(display-section "Environment — the conversation as a list of messages")

;; The environment is just a list of messages. Newest last.
;; This IS Vec<Message> in the Rust code.
;; This IS the environment in SICP's metacircular evaluator.
;; There's nothing fancy here. That's the point.

(run-test "environment is a list of messages"
  (lambda ()
    (let ((env (list (make-user-msg "hello")
                     (make-assistant-msg
                       (list (make-text-block "hi there"))))))
      (assert-equal (length env) 2 "two messages")
      (assert-equal (message-role (car env)) 'user "first is user")
      (assert-equal (message-role (cadr env)) 'assistant "second is assistant"))))

(run-test "environment grows by appending"
  (lambda ()
    (let* ((env (list (make-user-msg "hello")))
           (env (append env (list (make-assistant-msg
                                    (list (make-text-block "hi")))))))
      (assert-equal (length env) 2 "grew to two messages"))))

(test-summary)
