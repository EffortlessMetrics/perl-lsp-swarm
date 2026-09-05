;;; eglot-released.el --- External-Eglot adapter for the perl-lsp host driver -*- lexical-binding: t; -*-

;; Loaded by the shared runner right after scripts/test/emacs-host-driver.el.
;; It owns the external Eglot client subjects: standalone Eglot as released
;; on GNU ELPA (the legacy pinned 1.23 row and the manifest-bound 1.24 row)
;; and the pinned upstream-source Eglot extracted from the declared
;; emacs.git tree (#8776).  The declared inputs arrive through the run plan
;; environment; this adapter never installs, refreshes, or consults package
;; archives or ambient package state, and it proves that the Eglot it loads
;; is the declared client file, not the Emacs build's bundled copy and not
;; an ambient cache entry.
;;
;; The declared package archive reaches this adapter exactly for released
;; subjects — the run-plan builder refuses a package input for the
;; upstream-source subject before launch — so its presence selects the
;; released identity emission (with the archive digest as package
;; identity) and its absence selects the package-free upstream-source
;; identity.  The journey itself (registration, connection, candidate
;; binding, capability capture, shutdown, evidence exports) is one shared
;; path; the two states differ only in the client_loaded identity evidence
;; they emit.

;; NOTE: `eglot' is deliberately NOT required at the top of this file.  A
;; top-level require of it would load the Emacs build's bundled copy before
;; the declared client directory is on `load-path', and the in-body
;; require below would then be a satisfied no-op — the run would execute
;; bundled Eglot while claiming an external subject.  The only require
;; happens after the load-path is owned by the declared file.
(require 'cl-lib)
(require 'json)
(require 'lisp-mnt)

(defconst perl-lsp-test-released-readiness-deadline 30
  "Seconds the adapter waits for the synchronous Eglot connect.")

(defconst perl-lsp-test-released-shutdown-deadline 10
  "Seconds the adapter waits for the server process to die after shutdown.")

(defun perl-lsp-test-released-env (name)
  "Return required host environment variable NAME or signal an error."
  (or (getenv name)
      (error "external Eglot adapter missing environment: %s" name)))

(defun perl-lsp-test-released-file-digest (file)
  "Return the sha256 hex digest of FILE's raw bytes.

`insert-file-contents-literally' is mandatory here: the decoded variant
performs character decoding and line-ending translation, so its buffer hash
would diverge from the raw-byte digest the run plan verifies — silently for
text files, always for the binary package archive."
  (with-temp-buffer
    (insert-file-contents-literally file)
    (secure-hash 'sha256 (buffer-string))))

(defun perl-lsp-test-released-library-facts (library)
  "Return (VERSION SHA256-HEX) for the declared external LIBRARY file.

Both external states require the version header: `released' and
`upstream-source' are mandatory identity fields, so a library whose header
cannot be read fails the run instead of degrading to a digest-only claim.
The readability guard lives here, before the literal insert: the caller's
`let*' binding evaluates this function before any body guard could run, so
an unreadable declared file must fail with this declared error rather than
an Emacs-generic read error."
  (unless (file-readable-p library)
    (error "declared external Eglot library is not readable"))
  (let ((version (with-temp-buffer
                   (insert-file-contents-literally library)
                   (lm-version))))
    (unless (and (stringp version) (not (string= version "")))
      (error "external Eglot library carries no version header"))
    (list version (perl-lsp-test-released-file-digest library))))

(defun perl-lsp-test-released-json-normalize (value)
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
    (cl-map 'vector #'perl-lsp-test-released-json-normalize value))
   ((hash-table-p value)
    (let ((copy (make-hash-table :test #'equal)))
      (maphash
       (lambda (key item)
         (puthash (if (stringp key) key (format "%s" key))
                  (perl-lsp-test-released-json-normalize item)
                  copy))
       value)
      copy))
   ((and (listp value) (cl-every #'consp value) (listp (cdr (last value))))
    (mapcar
     (lambda (pair)
       (cons (perl-lsp-test-released-json-normalize (car pair))
             (perl-lsp-test-released-json-normalize (cdr pair))))
     value))
   ((listp value)
    (mapcar #'perl-lsp-test-released-json-normalize value))
   (t (format "%s" value))))

(defun perl-lsp-test-released-write-snapshot (server snapshot-file)
  "Write SERVER's initialize capabilities to SNAPSHOT-FILE."
  (let ((capabilities (eglot--capabilities server)))
    (unless capabilities
      (error "external Eglot server reported no initialize capabilities"))
    (with-temp-file snapshot-file
      (insert
       (condition-case err
           (json-serialize
            (perl-lsp-test-released-json-normalize capabilities))
         (error
          (error "external Eglot capability snapshot serialization failed: %S"
                 err)))))))

(defun perl-lsp-test-released-export-buffer (buffer file)
  "Write BUFFER's contents to FILE; an absent buffer writes an empty file."
  (with-temp-file file
    (when (buffer-live-p buffer)
      (insert (with-current-buffer buffer (buffer-string))))))

(defun perl-lsp-test-released-wait-for-dead-process (process deadline)
  "Wait until PROCESS is not live, or signal after DEADLINE seconds."
  (let ((limit (+ (float-time) deadline)))
    (while (and (process-live-p process) (< (float-time) limit))
      (accept-process-output nil 0.1))
    (when (process-live-p process)
      (error "external Eglot server process survived shutdown"))))

(defun perl-lsp-test-released-observed-program (server)
  "Return the program the live SERVER process was actually started as."
  (let* ((process (jsonrpc--process server))
         (command (and (process-live-p process) (process-command process))))
    (unless (and command (stringp (car command)))
      (error "external Eglot server process exposes no program identity"))
    (car command)))

(defun perl-lsp-test-client-run ()
  "Drive one external-Eglot lifecycle journey against the exact candidate.

The declared package archive selects the released identity; its absence
selects the package-free upstream-source identity.  Everything after
`client_loaded' is one shared journey for both states."
  (let* ((candidate (perl-lsp-test-released-env "PERL_LSP_EMACS_CANDIDATE"))
         (fixture-root (perl-lsp-test-released-env "PERL_LSP_EMACS_FIXTURE_ROOT"))
         (configuration (perl-lsp-test-released-env "PERL_LSP_EMACS_CONFIGURATION"))
         (snapshot-file (perl-lsp-test-released-env "PERL_LSP_EMACS_CAPABILITY_SNAPSHOT"))
         (client-log (perl-lsp-test-released-env "PERL_LSP_EMACS_CLIENT_LOG"))
         (stderr-file (perl-lsp-test-released-env "PERL_LSP_EMACS_SERVER_STDERR"))
         (library (perl-lsp-test-released-env "PERL_LSP_EMACS_CLIENT_SOURCE"))
         ;; Optional by state, not by oversight: the run-plan builder
         ;; refuses a package input for the upstream-source subject before
         ;; launch, so a live binding here means the released identity.
         (package-file (getenv "PERL_LSP_EMACS_CLIENT_PACKAGE")))
    ;; The declared library's identity facts (version header and raw-byte
    ;; digest) are computed here, after the bindings and before anything is
    ;; loaded: the facts function guards the file's readability itself —
    ;; the first place the file is touched — so an unreadable declared
    ;; library fails with the declared error, never an Emacs-generic read
    ;; error from a `let*' binding that ran before a guard could.
    ;; The declared package file is part of a released subject's identity;
    ;; its digest is emitted as runtime evidence alongside the library.  An
    ;; upstream-source subject carries no package identity at all, so the
    ;; requirement is conditional on the input's presence.
    (when package-file
      (unless (file-readable-p package-file)
        (error "released Eglot subject requires the declared package file")))
    (let ((facts (perl-lsp-test-released-library-facts library)))
    ;; The declared file's directory is pushed to the front of `load-path'
    ;; and the resolution is then proven: `locate-library' must return
    ;; exactly the declared file.  If the Emacs build's bundled Eglot, an
    ;; ambient package directory, or a stale cache entry answered instead,
    ;; this equality fails and the run fails closed.
    (add-to-list 'load-path (file-name-directory library))
    (require 'eglot)
    (let ((resolved (locate-library "eglot")))
      (unless resolved
        (error "external Eglot library did not resolve after require"))
      (unless (string-equal (file-truename resolved)
                            (file-truename library))
        (error "external Eglot did not resolve to the declared client file")))
    (perl-lsp-test-emit
     "client_loaded"
     (if package-file
         `((source_state . "released")
           (version . ,(nth 0 facts))
           (source_sha256 . ,(nth 1 facts))
           (package_sha256 . ,(perl-lsp-test-released-file-digest package-file)))
       `((source_state . "upstream_source")
         (version . ,(nth 0 facts))
         (source_sha256 . ,(nth 1 facts)))))
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
                       perl-lsp-test-released-readiness-deadline)
                      (eglot-autoreconnect nil)
                      (eglot-autoshutdown nil))
                  ;; `eglot--connect' does not default its class argument;
                  ;; nil would break `make-instance', so the external
                  ;; subjects pin the stock class explicitly.
                  (eglot--connect (list major-mode)
                                  (eglot--current-project)
                                  'eglot-lsp-server contact '("perl")))))
          (unless (and server (eglot-current-server))
            (error "external Eglot connect did not manage the fixture buffer"))
          ;; Exact-candidate binding: the observed program of the live
          ;; server process must be the declared candidate, byte for byte.
          (unless (string-equal (perl-lsp-test-released-observed-program server)
                                candidate)
            (error "external Eglot selected a non-candidate server program"))
          (perl-lsp-test-emit
           "registration_selected"
           `((registration . "manual_row")
             (program . ,(file-name-nondirectory candidate))))
          (perl-lsp-test-released-write-snapshot server snapshot-file)
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
                                perl-lsp-test-released-shutdown-deadline t)
                (perl-lsp-test-released-wait-for-dead-process
                 (jsonrpc--process server)
                 perl-lsp-test-released-shutdown-deadline)
                (perl-lsp-test-emit "shutdown_completed" nil))
            ;; Client log and server stderr stay separate artifacts, and
            ;; both exports run even when the shutdown path above failed so
            ;; the driver's failure event carries the captured evidence.
            (perl-lsp-test-released-export-buffer
             (jsonrpc-events-buffer server) client-log)
            (perl-lsp-test-released-export-buffer
             (jsonrpc-stderr-buffer server) stderr-file))))))))

(provide 'eglot-released)
;;; eglot-released.el ends here
