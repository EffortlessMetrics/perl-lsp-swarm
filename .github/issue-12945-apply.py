#!/usr/bin/env python3
from pathlib import Path
from textwrap import dedent, indent

source = Path("crates/perl-lsp-rs-core/src/config/mod.rs")
text = source.read_text(encoding="utf-8")

def block(raw: str, spaces: int = 0) -> str:
    value = dedent(raw)
    if value.startswith("\n"):
        value = value[1:]
    if not value.endswith("\n"):
        value += "\n"
    return indent(value, " " * spaces)

def replace_once(label: str, old: str, new: str) -> None:
    global text
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected exactly one source match, found {count}")
    text = text.replace(old, new, 1)

replace_once(
    "probe attempt field",
    block(
        '''
        /// Cached system @INC probe outcome (populated lazily when use_system_inc is true).
        system_inc_cache: Option<SystemIncProbeOutcome>,

        /// Perl interpreter used for startup `@INC` probing.
        ''',
        4,
    ),
    block(
        '''
        /// Cached system @INC probe outcome (populated lazily when use_system_inc is true).
        system_inc_cache: Option<SystemIncProbeOutcome>,

        /// Number of startup `@INC` probe attempts since the last invalidation.
        ///
        /// Only a timeout may consume the second and final attempt. Every other
        /// outcome settles the cache after the first probe.
        system_inc_probe_attempts: u8,

        /// Perl interpreter used for startup `@INC` probing.
        ''',
        4,
    ),
)

replace_once(
    "default attempt budget",
    block(
        '''
        use_system_inc: false,
        system_inc_cache: None,
        perl_path: None,
        ''',
        12,
    ),
    block(
        '''
        use_system_inc: false,
        system_inc_cache: None,
        system_inc_probe_attempts: 0,
        perl_path: None,
        ''',
        12,
    ),
)

replace_once(
    "useSystemInc invalidation",
    block(
        '''
        if use_inc != self.use_system_inc {
            self.system_inc_cache = None;
        }
        ''',
        16,
    ),
    block(
        '''
        if use_inc != self.use_system_inc {
            self.invalidate_system_inc_probe();
        }
        ''',
        16,
    ),
)

replace_once(
    "usePerl5lib invalidation",
    block(
        '''
        if use_p5l != self.use_perl5lib {
            self.system_inc_cache = None;
        }
        ''',
        16,
    ),
    block(
        '''
        if use_p5l != self.use_perl5lib {
            self.invalidate_system_inc_probe();
        }
        ''',
        16,
    ),
)

replace_once(
    "probe state machine",
    block(
        '''
        fn ensure_system_inc_probe(&mut self) {
            if self.system_inc_cache.is_some() {
                return;
            }

            // Snapshot the fields needed by the oracle constructor before the
            // mutable borrow below.
            let perl_args = self.perl_args.clone();
            let result = Self::fetch_perl_inc(self, &perl_args);
            self.system_inc_cache = Some(result);
        }
        ''',
        4,
    ),
    block(
        '''
        fn invalidate_system_inc_probe(&mut self) {
            self.system_inc_cache = None;
            self.system_inc_probe_attempts = 0;
        }

        fn ensure_system_inc_probe(&mut self) {
            self.ensure_system_inc_probe_with(Self::fetch_perl_inc);
        }

        fn ensure_system_inc_probe_with(
            &mut self,
            probe: impl FnOnce(&WorkspaceConfig, &[String]) -> SystemIncProbeOutcome,
        ) {
            if self.system_inc_probe_attempts >= SYSTEM_INC_PROBE_MAX_ATTEMPTS {
                return;
            }

            match self.system_inc_cache.as_ref() {
                None | Some(SystemIncProbeOutcome::TimedOut) => {}
                Some(_) => return,
            }

            // Snapshot the fields needed by the oracle constructor before the
            // mutable borrow below.
            let perl_args = self.perl_args.clone();
            let result = probe(self, &perl_args);
            self.system_inc_probe_attempts =
                self.system_inc_probe_attempts.saturating_add(1);
            self.system_inc_cache = Some(result);
        }
        ''',
        4,
    ),
)

replace_once(
    "typed outcome documentation",
    block(
        '''
        /// `SYSTEM_INC_PROBE_TIMEOUT`. The typed result is cached so callers can
        /// distinguish a transient timeout from a spawn failure, nonzero exit,
        /// unavailable oracle, or a successful empty output without changing the
        /// fail-closed behaviour of [`Self::get_system_inc`].
        ''',
        4,
    ),
    block(
        '''
        /// `SYSTEM_INC_PROBE_TIMEOUT`. Settled results are cached after one probe.
        /// A timeout fails closed for that lookup but permits one later caller to retry;
        /// after a second timeout, the result is terminal until configuration invalidates
        /// the probe state. The typed outcome keeps every failure class observable without
        /// changing the fail-closed behaviour of [`Self::get_system_inc`].
        ''',
        4,
    ),
)

replace_once(
    "path getter documentation",
    block(
        '''
        /// distinguish the failure class. The user can re-trigger probing by
        /// toggling `useSystemInc`, which invalidates the cache.
        ''',
        4,
    ),
    block(
        '''
        /// distinguish the failure class. A timeout may be retried once by a later
        /// lookup. Changing `useSystemInc` or `usePerl5lib` invalidates both the cached
        /// outcome and the retry budget.
        ''',
        4,
    ),
)

replace_once(
    "timeout warning",
    block(
        r'''
        "startup @INC probe timed out; caching empty result. \
         Set perl.workspace.useSystemInc=false to disable probing, \
         or pin a faster perl interpreter."
        ''',
        20,
    ),
    block(
        r'''
        "startup @INC probe timed out; failing closed for this lookup. \
         The configuration cache permits at most one later retry. \
         Set perl.workspace.useSystemInc=false to disable probing, \
         or pin a faster perl interpreter."
        ''',
        20,
    ),
)

replace_once(
    "bounded attempt constants",
    block(
        '''
        /// Bounded interpreter startup `@INC` probe.
        ///
        /// The probe is intentionally a separate constant from
        /// `WorkspaceConfig::resolution_timeout_ms` (50 ms default). 50 ms is well
        /// under Perl interpreter startup on most platforms — a perlbrew shim,
        /// remote filesystem, or even a cold cache can comfortably exceed it.
        /// 1000 ms is short enough that a stalled probe does not noticeably block
        /// the LSP and long enough that healthy probes succeed reliably.
        #[cfg(not(target_arch = "wasm32"))]
        pub(crate) const SYSTEM_INC_PROBE_TIMEOUT: Duration = Duration::from_secs(1);
        ''',
    ),
    block(
        '''
        /// Maximum startup `@INC` probe attempts between configuration invalidations.
        ///
        /// The first timeout fails closed for its caller. Exactly one later caller may
        /// retry; every non-timeout outcome settles immediately.
        const SYSTEM_INC_PROBE_MAX_ATTEMPTS: u8 = 2;

        /// Per-attempt bound for interpreter startup `@INC` probing.
        ///
        /// This remains separate from `WorkspaceConfig::resolution_timeout_ms` (50 ms
        /// by default), which is too short for interpreter startup. Each attempt gets
        /// one second, and the retry state machine permits at most two attempts between
        /// configuration invalidations.
        #[cfg(not(target_arch = "wasm32"))]
        pub(crate) const SYSTEM_INC_PROBE_TIMEOUT: Duration = Duration::from_secs(1);
        ''',
    ),
)

parse_test = block(
    '''
    #[test]
    fn parse_perl_inc_output_dedupes_and_drops_dot() {
        let parsed =
            WorkspaceConfig::parse_perl_inc_output("lib\\n.\\nlib\\n/usr/lib/perl5\\n/usr/lib/perl5\\n");
        assert_eq!(parsed, vec![PathBuf::from("lib"), PathBuf::from("/usr/lib/perl5")]);
    }
    ''',
    4,
)
retry_tests = block(
    '''
    #[test]
    fn system_inc_timeout_retries_once_and_recovers() {
        let calls = std::cell::Cell::new(0);
        let mut config =
            WorkspaceConfig { use_system_inc: true, ..WorkspaceConfig::default() };

        config.ensure_system_inc_probe_with(|_, _| {
            calls.set(calls.get() + 1);
            SystemIncProbeOutcome::TimedOut
        });
        assert_eq!(config.system_inc_cache, Some(SystemIncProbeOutcome::TimedOut));
        assert_eq!(config.system_inc_probe_attempts, 1);

        let recovered = vec![PathBuf::from("/recovered/system-inc")];
        config.ensure_system_inc_probe_with(|_, _| {
            calls.set(calls.get() + 1);
            SystemIncProbeOutcome::Paths(recovered.clone())
        });
        config.ensure_system_inc_probe_with(|_, _| {
            panic!("a settled successful outcome must not launch a third probe")
        });

        assert_eq!(calls.get(), 2);
        assert_eq!(
            config.system_inc_cache,
            Some(SystemIncProbeOutcome::Paths(recovered))
        );
        assert_eq!(config.system_inc_probe_attempts, 2);
    }

    #[test]
    fn system_inc_timeout_stops_after_two_attempts() {
        let calls = std::cell::Cell::new(0);
        let mut config =
            WorkspaceConfig { use_system_inc: true, ..WorkspaceConfig::default() };

        for _ in 0..5 {
            config.ensure_system_inc_probe_with(|_, _| {
                calls.set(calls.get() + 1);
                SystemIncProbeOutcome::TimedOut
            });
        }

        assert_eq!(calls.get(), 2, "timeout retry budget must be exactly two attempts");
        assert_eq!(config.system_inc_probe_attempts, 2);
        assert_eq!(config.system_inc_cache, Some(SystemIncProbeOutcome::TimedOut));
    }

    #[test]
    fn system_inc_non_timeout_outcome_is_memoized() {
        let calls = std::cell::Cell::new(0);
        let mut config =
            WorkspaceConfig { use_system_inc: true, ..WorkspaceConfig::default() };

        config.ensure_system_inc_probe_with(|_, _| {
            calls.set(calls.get() + 1);
            SystemIncProbeOutcome::IoFailed
        });
        config.ensure_system_inc_probe_with(|_, _| {
            calls.set(calls.get() + 1);
            SystemIncProbeOutcome::Paths(vec![PathBuf::from("/must-not-run")])
        });

        assert_eq!(calls.get(), 1, "only TimedOut may consume the retry attempt");
        assert_eq!(config.system_inc_probe_attempts, 1);
        assert_eq!(config.system_inc_cache, Some(SystemIncProbeOutcome::IoFailed));
    }

    #[test]
    fn system_inc_config_invalidation_restores_full_retry_budget() {
        let calls = std::cell::Cell::new(0);
        let mut config =
            WorkspaceConfig { use_system_inc: true, ..WorkspaceConfig::default() };

        let exhaust = |config: &mut WorkspaceConfig| {
            for _ in 0..3 {
                config.ensure_system_inc_probe_with(|_, _| {
                    calls.set(calls.get() + 1);
                    SystemIncProbeOutcome::TimedOut
                });
            }
        };

        exhaust(&mut config);
        assert_eq!(calls.get(), 2);

        config.update_from_value(&serde_json::json!({
            "workspace": { "useSystemInc": false }
        }));
        config.update_from_value(&serde_json::json!({
            "workspace": { "useSystemInc": true }
        }));
        assert!(config.system_inc_cache.is_none());
        assert_eq!(config.system_inc_probe_attempts, 0);
        exhaust(&mut config);
        assert_eq!(calls.get(), 4, "useSystemInc must restore two attempts");

        config.update_from_value(&serde_json::json!({
            "workspace": { "usePerl5lib": false }
        }));
        assert!(config.system_inc_cache.is_none());
        assert_eq!(config.system_inc_probe_attempts, 0);
        exhaust(&mut config);
        assert_eq!(calls.get(), 6, "usePerl5lib must restore two attempts");
    }
    ''',
    4,
)
replace_once(
    "deterministic retry tests",
    parse_test,
    parse_test + "\n" + retry_tests,
)

replace_once(
    "slow interpreter cache comment",
    block(
        '''
        // Cached empty result — second call does not respawn perl.
        let start2 = Instant::now();
        ''',
        8,
    ),
    block(
        '''
        // Both bounded attempts have now been consumed, so the next lookup is
        // terminal-cache-only and must not respawn perl.
        let start2 = Instant::now();
        ''',
        8,
    ),
)

source.write_text(text, encoding="utf-8")
