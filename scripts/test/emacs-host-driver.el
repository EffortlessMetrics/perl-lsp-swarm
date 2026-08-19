;;; emacs-host-driver.el --- Hermetic perl-lsp host driver protocol -*- lexical-binding: t; -*-

;; This file owns only the common event and failure protocol.  Exact Eglot and
;; lsp-mode adapters are loaded after it and must define
;; `perl-lsp-test-client-run'.

(require 'json)

(defconst perl-lsp-test-driver-schema-version "emacs_host_driver.v1")
(defvar perl-lsp-test--event-sequence 0)

(defun perl-lsp-test--required-environment (name)
  "Return required environment variable NAME or signal an error."
  (or (getenv name)
      (error "Missing required perl-lsp host environment: %s" name)))

(defun perl-lsp-test--safe-detail-value (value)
  "Reject VALUE when it could expose an absolute or private path."
  (let ((text (format "%s" value)))
    (when (or (string-prefix-p "/" text)
              (string-prefix-p "~" text)
              (string-match-p "^[[:alpha:]]:[/\\\\]" text)
              (string-match-p "://" text)
              (string-match-p "\\(?:^\\|/\\)\\.\\.\\(?:/\\|$\\)" text))
      (error "Unsafe driver detail value"))
    text))

(defun perl-lsp-test-emit (event &optional details)
  "Append one ordered EVENT with safe DETAILS to the driver JSONL stream."
  (setq perl-lsp-test--event-sequence (1+ perl-lsp-test--event-sequence))
  (let* ((event-file
          (perl-lsp-test--required-environment "PERL_LSP_EMACS_EVENT_FILE"))
         (safe-details
          (mapcar
           (lambda (entry)
             (cons (car entry)
                   (perl-lsp-test--safe-detail-value (cdr entry))))
           details))
         (payload
          `((schema_version . ,perl-lsp-test-driver-schema-version)
            (sequence . ,perl-lsp-test--event-sequence)
            (event . ,event)
            (details . ,safe-details))))
    (with-temp-buffer
      (insert (json-serialize payload))
      (insert "\n")
      (write-region (point-min) (point-max) event-file t 'silent))))

(defun perl-lsp-test-run ()
  "Run the exact checked client adapter and emit a bounded failure event."
  (perl-lsp-test-emit
   "host_started"
   `((subject . "emacs")
     (client_kind . ,(perl-lsp-test--required-environment
                       "PERL_LSP_EMACS_CLIENT_KIND"))))
  (condition-case err
      (progn
        (unless (fboundp 'perl-lsp-test-client-run)
          (error "Loaded adapter does not define perl-lsp-test-client-run"))
        (funcall #'perl-lsp-test-client-run))
    (error
     (perl-lsp-test-emit
      "driver_failed"
      `((reason . ,(format "%S" (car err)))))
     (kill-emacs 2))))

(provide 'emacs-host-driver)
;;; emacs-host-driver.el ends here
