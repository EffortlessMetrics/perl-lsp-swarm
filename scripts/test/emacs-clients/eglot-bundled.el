;;; eglot-bundled.el --- Bundled-Eglot adapter for the perl-lsp host driver -*- lexical-binding: t; -*-

;; Loaded by the shared runner right after scripts/test/emacs-host-driver.el.
;; It owns exactly one client subject: the Eglot bundled inside the exact
;; Emacs build under test.  The adapter never installs, refreshes, or loads
;; packages, and it replaces `eglot-server-programs' with one manual row so
;; no ambient registration can answer instead of the exact candidate.

(require 'cl-lib)
(require 'eglot)
(require 'json)
(require 'lisp-mnt)

(defconst perl-lsp-test-bundled-readiness-deadline 30
  "Seconds the adapter waits for the synchronous Eglot connect.")

(defconst perl-lsp-test-bundled-shutdown-deadline 10
  "Seconds the adapter waits for the server process to die after shutdown.")

(defun perl-lsp-test-bundled-env (name)
  "Return required host environment variable NAME or signal an error."
  (or (getenv name)
      (error "bundled Eglot adapter missing environment: %s" name)))

(defun perl-lsp-test-bundled-emacs-root ()
  "Return the installation root of the running Emacs build."
  (file-name-as-directory
   (file-name-directory (directory-file-name invocation-directory))))

(defun perl-lsp-test-bundled-library ()
  "Return the Eglot library this Emacs actually resolves, or signal."
  (let ((library (locate-library "eglot")))
    (unless library
      (error "this Emacs build does not resolve an Eglot library"))
    library))

(defun perl-lsp-test-bundled-library-facts (library)
  "Return (VERSION SHA256-HEX) for the loaded bundled LIBRARY file.

VERSION is the string `version_unavailable' when the resolved library is a
byte-compiled or compressed form: installed builds commonly load
`eglot.elc' while shipping the source only as `eglot.el.gz', and a bytecode
file carries no reliable header to read.  The digest stays mandatory in
every form: it is the exact-file identity the run plan cross-checks, so the
file is read literally — the decoded `insert-file-contents' performs
character decoding and line-ending translation, which would silently change
the digest relative to the raw bytes the plan verifies."
  (with-temp-buffer
    (insert-file-contents-literally library)
    (let ((digest (secure-hash 'sha256 (buffer-string))))
      (let ((version (condition-case nil
                         (let ((header (lm-version)))
                           (if (and (stringp header) (not (string= header "")))
                               header
                             "version_unavailable"))
                       (error "version_unavailable"))))
        (list version digest)))))

(defun perl-lsp-test-bundled-json-normalize (value)
  "Normalize decoded JSON VALUE for `json-serialize'."
  (cond
   ((eq value :json-false) :false)
   ((eq value :json-null) :null)
   ((stringp value) value)
   ((numberp value) value)
   ((eq value t) t)
   ((keywordp value) (substring (symbol-name value) 1))
   ((symbolp value) (symbol-name value))
   ((vectorp value)
    (cl-map 'vector #'perl-lsp-test-bundled-json-normalize value))
   ((hash-table-p value)
    (let ((copy (make-hash-table :test #'equal)))
      (maphash
       (lambda (key item)
         (puthash (if (stringp key) key (format "%s" key))
                  (perl-lsp-test-bundled-json-normalize item)
                  copy))
       value)
      copy))
   ((and (listp value) (cl-every #'consp value) (listp (cdr (last value))))
    (mapcar
     (lambda (pair)
       (cons (perl-lsp-test-bundled-json-normalize (car pair))
             (perl-lsp-test-bundled-json-normalize (cdr pair))))
     value))
   ((listp value)
    (mapcar #'perl-lsp-test-bundled-json-normalize value))
   (t (format "%s" value))))

(defun perl-lsp-test-bundled-write-snapshot (server snapshot-file)
  "Write SERVER's initialize capabilities to SNAPSHOT-FILE."
  (let ((capabilities (eglot--capabilities server)))
    (unless capabilities
      (error "bundled Eglot server reported no initialize capabilities"))
    (with-temp-file snapshot-file
      (insert
       (condition-case err
           (json-serialize
            (perl-lsp-test-bundled-json-normalize capabilities))
         (error
          (error "bundled Eglot capability snapshot serialization failed: %S"
                 err)))))))

(defun perl-lsp-test-bundled-export-buffer (buffer file)
  "Write BUFFER's contents to FILE; an absent buffer writes an empty file."
  (with-temp-file file
    (when (buffer-live-p buffer)
      (insert (with-current-buffer buffer (buffer-string))))))

(defun perl-lsp-test-bundled-wait-for-dead-process (process deadline)
  "Wait until PROCESS is not live, or signal after DEADLINE seconds."
  (let ((limit (+ (float-time) deadline)))
    (while (and (process-live-p process) (< (float-time) limit))
      (accept-process-output nil 0.1))
    (when (process-live-p process)
      (error "bundled Eglot server process survived shutdown"))))

(defun perl-lsp-test-bundled-observed-program (server)
  "Return the program the live SERVER process was actually started as."
  (let* ((process (jsonrpc--process server))
         (command (and (process-live-p process) (process-command process))))
    (unless (and command (stringp (car command)))
      (error "bundled Eglot server process exposes no program identity"))
    (car command)))

(defun perl-lsp-test-client-run ()
  "Drive one bundled-Eglot lifecycle journey against the exact candidate."
  (let* ((candidate (perl-lsp-test-bundled-env "PERL_LSP_EMACS_CANDIDATE"))
         (fixture-root (perl-lsp-test-bundled-env "PERL_LSP_EMACS_FIXTURE_ROOT"))
         (configuration (perl-lsp-test-bundled-env "PERL_LSP_EMACS_CONFIGURATION"))
         (snapshot-file (perl-lsp-test-bundled-env "PERL_LSP_EMACS_CAPABILITY_SNAPSHOT"))
         (client-log (perl-lsp-test-bundled-env "PERL_LSP_EMACS_CLIENT_LOG"))
         (stderr-file (perl-lsp-test-bundled-env "PERL_LSP_EMACS_SERVER_STDERR"))
         (library (perl-lsp-test-bundled-library))
         (emacs-root (perl-lsp-test-bundled-emacs-root))
         (facts (perl-lsp-test-bundled-library-facts library)))
    ;; Bundled proof: the resolved library must live inside the running
    ;; build.  A package from an ambient archive or cache resolves outside
    ;; the installation root and fails here instead of silently answering.
    (unless (string-prefix-p emacs-root library)
      (error "resolved Eglot library is not inside the running Emacs build"))
    (perl-lsp-test-emit
     "client_loaded"
     `((source_state . "bundled")
       (version . ,(nth 0 facts))
       (source_sha256 . ,(nth 1 facts))))
    ;; The checked configuration is a real run input: it is loaded before
    ;; the connection so client behavior settings come from the plan, not
    ;; from ambient state.
    (load configuration nil t)
    (let* ((contact (list candidate "--stdio"))
           (probe-file (expand-file-name "script/probe.pl" fixture-root))
           (buffer (find-file-noselect probe-file)))
      ;; Registration: the manual candidate row replaces the whole table,
      ;; so no ambient `eglot-server-programs' entry can be consulted.
      (setq eglot-server-programs
            `((perl-mode . ,contact) (cperl-mode . ,contact)))
      (with-current-buffer buffer
        (let* ((server
                (let ((eglot-sync-connect
                       perl-lsp-test-bundled-readiness-deadline)
                      (eglot-autoreconnect nil)
                      (eglot-autoshutdown nil))
                  ;; `eglot--connect' does not default its class argument;
                  ;; nil would break `make-instance', so the bundled subject
                  ;; pins the stock class explicitly.
                  (eglot--connect (list major-mode)
                                  (eglot--current-project)
                                  'eglot-lsp-server contact '("perl")))))
          (unless (and server (eglot-current-server))
            (error "bundled Eglot connect did not manage the fixture buffer"))
          ;; Exact-candidate binding: the observed program of the live
          ;; server process must be the declared candidate, byte for byte.
          (unless (string-equal (perl-lsp-test-bundled-observed-program server)
                                candidate)
            (error "bundled Eglot selected a non-candidate server program"))
          (perl-lsp-test-emit
           "registration_selected"
           `((registration . "manual_row")
             (program . ,(file-name-nondirectory candidate))))
          (perl-lsp-test-bundled-write-snapshot server snapshot-file)
          (perl-lsp-test-emit "initialize_observed" nil)
          (perl-lsp-test-emit
           "workspace_ready"
           `((server_count . ,(format "%d"
                                      (length (hash-table-values
                                               eglot--servers-by-project))))))
          (perl-lsp-test-emit
           "buffer_opened"
           `((mode . ,(symbol-name major-mode))))
          (unwind-protect
              (progn
                (perl-lsp-test-emit "shutdown_started" nil)
                ;; The optional order is (SERVER _INTERACTIVE TIMEOUT
                ;; PRESERVE-BUFFERS): the events and stderr buffers must
                ;; survive so the exports below carry the captured evidence.
                (eglot-shutdown server nil
                                perl-lsp-test-bundled-shutdown-deadline t)
                (perl-lsp-test-bundled-wait-for-dead-process
                 (jsonrpc--process server)
                 perl-lsp-test-bundled-shutdown-deadline)
                (perl-lsp-test-emit "shutdown_completed" nil))
            ;; Client log and server stderr stay separate artifacts, and
            ;; both exports run even when the shutdown path above failed so
            ;; the driver's failure event carries the captured evidence.
            (perl-lsp-test-bundled-export-buffer
             (jsonrpc-events-buffer server) client-log)
            (perl-lsp-test-bundled-export-buffer
             (jsonrpc-stderr-buffer server) stderr-file)))))))

(provide 'eglot-bundled)
;;; eglot-bundled.el ends here
