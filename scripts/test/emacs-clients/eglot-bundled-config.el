;;; eglot-bundled-config.el --- Checked configuration for the bundled-Eglot subject -*- lexical-binding: t; -*-

;; Bound by hash into the run plan and loaded by the adapter before the
;; connection is made.  It owns client behavior settings only; it must never
;; touch package state, registration tables, or fixture content.

(setq eglot-sync-connect 30)
(setq eglot-autoreconnect nil)
(setq eglot-autoshutdown nil)
(setq eglot-events-buffer-config '(500000 . 4))

;;; eglot-bundled-config.el ends here
