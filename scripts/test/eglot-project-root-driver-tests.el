;;; eglot-project-root-driver-tests.el --- Root probe receipt tests -*- lexical-binding: t; -*-

;; Run with Emacs -Q --batch -l this-file -f ert-run-tests-batch-and-exit.
;; These are instrument tests, not observations of the hosted root matrix.
(require 'ert)
(load (expand-file-name "eglot-project-root-driver.el"
                        (file-name-directory (or load-file-name buffer-file-name)))
      nil nil)

(ert-deftest perl-lsp-root-probe-no-candidate-writes-typed-refusal ()
  "Exercise the real driver and serializer without contacting a server."
  (let* ((directory (make-temp-file "eglot-root-receipt-" t))
         (source (expand-file-name "probe.pl" directory))
         (receipt (expand-file-name "receipt.json" directory))
         (process-environment (copy-sequence process-environment))
         (project-list-file (expand-file-name "projects" directory)))
    (unwind-protect
        (progn
          (with-temp-file source (insert "my $value = 1;\n"))
          (dolist (binding `(("FILE" . ,source)
                             ("RECEIPT" . ,receipt)
                             ("FIXTURE_ROOT" . ,directory)
                             ("GENERATION" . "unit-test-generation")
                             ("SUBJECT" . "unit-test-subject")
                             ("CASE" . "standalone_file")))
            (setenv (concat "PERL_LSP_EGLOT_ROOT_PROBE_" (car binding))
                    (cdr binding)))
          (setenv "PERL_LSP_EGLOT_ROOT_PROBE_CANDIDATE" nil)
          (perl-lsp-root-probe-run)
          (let ((record (with-temp-buffer
                          (insert-file-contents receipt)
                          (json-parse-buffer :object-type 'alist))))
            (should (eq (alist-get 'manual_action_required record) t))
            (should (eq (alist-get 'session_established record) :false))
            (should (eq (alist-get 'initialize_root_uri record) :null))
            (should (equal (alist-get 'refusal_reason record)
                           "candidate_executable_not_supplied"))
            (should (eq (alist-get 'cleanup_buffer_closed record) t))
            (should (= (alist-get 'process_cleanup_live_servers record) 0))
            (should (eq (alist-get 'driver_complete record) t))
            (should-not (get-file-buffer source))))
      (when-let ((buffer (get-file-buffer source)))
        (with-current-buffer buffer (set-buffer-modified-p nil))
        (kill-buffer buffer))
      (delete-directory directory t))))

(ert-deftest perl-lsp-root-probe-native-json-booleans-stay-distinct ()
  "Pin the serializer's actual true, false and null representations."
  (let ((receipt (make-temp-file "eglot-root-json-")))
    (unwind-protect
        (progn
          (perl-lsp-root-probe--record
           receipt '((manual_action_required . t)
                     (session_established . :false)
                     (initialize_root_uri . :null)))
          (let ((record (with-temp-buffer
                          (insert-file-contents receipt)
                          (json-parse-buffer :object-type 'alist))))
            (should (eq (alist-get 'manual_action_required record) t))
            (should (eq (alist-get 'session_established record) :false))
            (should (eq (alist-get 'initialize_root_uri record) :null)))
          (should-error (json-serialize '((manual_action_required . :true)))
                        :type 'wrong-type-argument))
      (delete-file receipt))))

;;; eglot-project-root-driver-tests.el ends here
