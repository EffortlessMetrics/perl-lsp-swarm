;;; eglot-project-root-driver.el --- Stock Eglot/project.el root probe -*- lexical-binding: t; -*-

;; Issue #11747 observation instrument: open one fixture file without
;; prebinding an expected root, then record exactly what stock project.el
;; and stock Eglot recognized and selected.  This driver invents no root,
;; never equates the process working directory to the project root, never
;; populates any remembered-project or known-root registry, and never
;; reads expectation metadata.  A missing server program is a recorded
;; manual action, never an inferred negative disposition.

(require 'json)
(require 'project)
(require 'eglot)
(require 'seq)
(require 'subr-x)

(defconst perl-lsp-root-probe-schema-version "emacs_eglot_project_root_observations.v1")

(defun perl-lsp-root-probe--required-environment (name)
  "Return required environment variable NAME or signal an error."
  (or (getenv name)
      (error "Missing required Eglot root-probe environment: %s" name)))

(defun perl-lsp-root-probe--library-facts ()
  "Return the located stock Eglot library file's name and digest."
  (let* ((library (locate-library "eglot"))
         (absolute (and library (expand-file-name library))))
    (unless absolute
      (error "loaded Emacs exposes no stock Eglot library"))
    (with-temp-buffer
      (insert-file-contents-literally absolute)
      ;; The absolute library location stays out of the durable record;
      ;; only its digest and basename identify the subject bytes here.
      (list (file-name-nondirectory absolute)
            (secure-hash 'sha256 (buffer-string))))))

(defun perl-lsp-root-probe--error-token (err)
  "Bound refusal token for ERR that never embeds a private path."
  (if (and (consp err) (symbolp (car err)))
      (substring (symbol-name (car err)) 0 (min 64 (length (symbol-name (car err)))))
    "connect_failed"))

(defun perl-lsp-root-probe--initialize-request-root-uri (server)
  "Return this exact server's initialize rootUri as a typed outcome.

The car is `observed' when the initialize request itself supplied the
answer — cdr is the URI string, or :null for a literal JSON null — and
`unavailable' when the instrument could not read that evidence at all,
with a typed refusal token as cdr.  The two are never collapsed: an
unreadable event log is an instrument refusal, not an observation that
stock Eglot sent no root."
  (let* ((events (ignore-errors (jsonrpc-events-buffer server)))
         (text (and events
                    (buffer-live-p events)
                    (with-current-buffer events
                      (buffer-string)))))
    (cond
     ((null text) (cons 'unavailable "initialize_events_unreadable"))
     ;; Only the two alternatives capture, and each holds the answer
     ;; itself: no surrounding quotes and no trailing whitespace can
     ;; reach the receipt as part of the observed root.
     ((not (string-match
            "\"rootUri\"[[:space:]]*:[[:space:]]*\\(?:\"\\([^\"]*\\)\"\\|\\(null\\)\\)"
            text))
      (cons 'unavailable "initialize_root_uri_absent"))
     ((match-beginning 1) (cons 'observed (match-string 1 text)))
     (t (cons 'observed :null)))))

(defun perl-lsp-root-probe--live-server-processes ()
  "Return the Eglot server processes that are live right now.

Uses only the public process list and Eglot's own process-naming
convention, so no private session registry is consulted."
  (seq-filter (lambda (process)
                (and (process-live-p process)
                     (string-prefix-p "EGLOT" (process-name process))))
              (process-list)))

(defun perl-lsp-root-probe--live-server-count (server baseline)
  "Count the live server processes this case is answerable for.

SERVER is the exact connection this case created, when `eglot--connect'
returned one.  BASELINE is the set of Eglot processes already live
before the attempt: those belong to other sessions and can neither
satisfy nor fail this case.  Counting the non-baseline survivors rather
than SERVER alone also catches the leak where `eglot--connect' spawned a
process and then raised, so this case never received the handle — the
old exact-server check recorded zero survivors for exactly that case."
  (let ((surviving (seq-count (lambda (process) (not (memq process baseline)))
                              (perl-lsp-root-probe--live-server-processes)))
        (exact (and server (jsonrpc--process server))))
    ;; A server whose process somehow predates the attempt is still this
    ;; case's own session, so it is never excused by the baseline.
    (if (and exact (process-live-p exact) (memq exact baseline))
        (1+ surviving)
      surviving)))

(defun perl-lsp-root-probe-run ()
  "Run one stock project.el/Eglot root observation and write its receipt."
  (let* ((probe-file (expand-file-name
                      (perl-lsp-root-probe--required-environment
                       "PERL_LSP_EGLOT_ROOT_PROBE_FILE")))
         (receipt-path (expand-file-name
                        (perl-lsp-root-probe--required-environment
                         "PERL_LSP_EGLOT_ROOT_PROBE_RECEIPT")))
         (candidate (getenv "PERL_LSP_EGLOT_ROOT_PROBE_CANDIDATE"))
         (fixture-root (expand-file-name (perl-lsp-root-probe--required-environment "PERL_LSP_EGLOT_ROOT_PROBE_FIXTURE_ROOT")))
         (generation-identity (perl-lsp-root-probe--required-environment "PERL_LSP_EGLOT_ROOT_PROBE_GENERATION"))
         (subject-id (perl-lsp-root-probe--required-environment "PERL_LSP_EGLOT_ROOT_PROBE_SUBJECT"))
         ;; Deliberate: the process working directory stays wherever the
         ;; host put it and is never consulted as root authority.
         (buffer (find-file-noselect probe-file))
         (project (with-current-buffer buffer (project-current nil)))
         (root (and project
                    (with-current-buffer buffer (project-root project))))
         (project-name
          (and project (fboundp 'project-name)
               (with-current-buffer buffer (project-name project))))
         (opened-relative (file-relative-name probe-file fixture-root))
         ;; Captured while BUFFER is certainly live; the cleanup step below
         ;; may close it before the receipt is written.
         (opened-mode (symbol-name (buffer-local-value 'major-mode buffer))))
    (let* ((server nil)
          ;; Captured before any connect attempt: every Eglot process
          ;; already live belongs to another session and is excluded from
          ;; this case's cleanup answer in both directions.
          (baseline (perl-lsp-root-probe--live-server-processes))
          (session-result
           ;; Stock behavior ends here when no server program is supplied:
           ;; there is nothing to contact, so the receipt records the
           ;; manual action instead of inventing a session fact.
           (if candidate
               (condition-case err
                   ;; The connect and the observation are one protected
                   ;; body: a `progn' is required here, because a bare
                   ;; second form would be read as a handler for the
                   ;; condition named by its head rather than as code.
                   (progn
                     (setq server
                           (with-current-buffer buffer
                             (let ((eglot-sync-connect 30)
                                   (eglot-autoreconnect nil)
                                   (eglot-autoshutdown t))
                               ;; Mirror the landed bundled adapter: this
                               ;; is exactly what stock `eglot-contact'
                               ;; runs, so the observed selection stays
                               ;; stock behavior end to end.
                               (eglot--connect
                                (list major-mode)
                                (eglot--current-project)
                                'eglot-lsp-server
                                (list candidate "--stdio")
                                '("perl")))))
                     (if (and server (process-live-p (jsonrpc--process server)))
                         (let* ((root-uri
                                 (perl-lsp-root-probe--initialize-request-root-uri
                                  server))
                                (observed
                                 (if (eq (car root-uri) 'observed)
                                     `((session_established . t)
                                       (initialize_root_uri . ,(cdr root-uri)))
                                   ;; The session is a fact; its initialize
                                   ;; evidence is not.  Refuse the root
                                   ;; rather than let an unreadable event
                                   ;; log serialize as an observed null.
                                   `((session_established . t)
                                     (initialize_root_uri . :null)
                                     (manual_action_required . t)
                                     (refusal_reason . ,(cdr root-uri))))))
                           ;; Observe while alive, then end the session
                           ;; here only; final cleanup verification stays
                           ;; with the single post-run cleanup phase below.
                           (condition-case _shutdown
                               (eglot-shutdown server nil 15 t)
                             (error nil))
                           observed)
                       '((session_established . :false)
                         (manual_action_required . t)
                         (refusal_reason . "no_live_session"))))
                 (error
                  `((session_established . :false)
                    (manual_action_required . t)
                    (refusal_reason . ,(perl-lsp-root-probe--error-token err)))))
             '((session_established . :false)
               (manual_action_required . t)
               (refusal_reason . "candidate_executable_not_supplied"))))
          (library-facts (perl-lsp-root-probe--library-facts))
          (cleanup-live-servers 0)
          (cleanup-buffer-dead (not (buffer-live-p buffer))))
      ;; Post-cleanup verification drives the receipt: cleanup is proven,
      ;; never asserted while optimism was still possible.
      (setq cleanup-live-servers
            (perl-lsp-root-probe--live-server-count server baseline))
      (when (> cleanup-live-servers 0)
        ;; Fail closed before any receipt exists: a torn-down-but-alive
        ;; server process may never back a recorded observation, so the
        ;; host run refuses instead of emitting driver_complete.
        (error "refusing to record: live server process behind cleanup (%d surviving)"
               cleanup-live-servers))
      (when (buffer-live-p buffer)
        (kill-buffer buffer)
        (setq cleanup-buffer-dead (not (buffer-live-p buffer))))
      (perl-lsp-root-probe--record
       receipt-path
       `((case_id . ,(perl-lsp-root-probe--required-environment
                      "PERL_LSP_EGLOT_ROOT_PROBE_CASE"))
         (subject_id . ,subject-id)
         (generation_identity . ,generation-identity)
         (emacs_version . ,(emacs-version))
         (eglot_library . ,(nth 0 library-facts))
         (eglot_sha256 . ,(nth 1 library-facts))
         (opened_file_relative . ,opened-relative)
         (major_mode . ,opened-mode)
         (language_id . "perl")
         (project_recognized . ,(if project t :false))
         (project_el_root_relative . ,(if root
                                          (file-relative-name root fixture-root)
                                        :null))
         (project_el_name . ,(or project-name :null))
         (session_established
          . ,(or (alist-get 'session_established session-result) :false))
         (initialize_root_uri
          . ,(or (alist-get 'initialize_root_uri session-result) :null))
         (manual_action_required
          . ,(or (alist-get 'manual_action_required session-result) :null))
         (refusal_reason
          . ,(or (alist-get 'refusal_reason session-result) :null))
         (process_cleanup_live_servers . ,cleanup-live-servers)
         (cleanup_buffer_closed . ,(if cleanup-buffer-dead t :false))
         (driver_complete . t))))))

(defun perl-lsp-root-probe--record (receipt-path payload-alist)
  "Write one observation receipt to RECEIPT-PATH from PAYLOAD-ALIST.

Every carried fact keeps its native false/null object so the record can
distinguish \"stock answered nothing\" from \"instrument failed to ask\"."
  (with-temp-file receipt-path
    (insert (json-serialize payload-alist))))

(provide 'eglot-project-root-driver)
;;; eglot-project-root-driver.el ends here
