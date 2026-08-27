# Worktree forensic evidence v1

The `worktree-recovery plan` route is a read-only, explicit-input observer for
one repository and one candidate. It does not discover candidates and does not
write backups, apply recovery, repair Git administration, or perform operator
worktree actions.

Stable-read evidence is bounded to the observation interval. On Unix, the
observer compares device/inode identity, metadata, and repeated bytes. On
Windows it uses the repository's stable WinAPI `FileIdInfo` adapter when that
instrument is available. Adapter or filesystem identity failure remains
`Unavailable`; without observed identity the result must not claim
`CLEAN_RECONSTRUCTABLE` or race detection. A Windows `Unavailable` result is
therefore an instrument limitation, not evidence that the candidate is clean.

Recursive manifest observation also fingerprints every directory before reading
its entries and revalidates that identity after enumeration and descent. A
directory replacement, reparse transition, or unavailable directory identity
marks the manifest incomplete and prevents a clean classification. This is a
fail-closed sampled-interval guarantee; it does not claim that a replacement
after the final route revalidation is impossible.

Git output is bounded to 4 MiB of retained stdout/stderr and a 10-second
producer interval. A child that exceeds either bound is terminated and the
observation fails closed. Ignored source-like files are detected through the
bounded status result and the lossless manifest path identity, including files
nested below ignored directories. Non-UTF-8 or quoted status paths are not
converted lossy; they remain unclassifiable.

On Unix, each Git child starts a separate process group and group termination
closes descendant capture pipes after timeout, output overflow, or normal
completion. On Windows, each child is placed in a kill-on-close job object for
the same purpose. Unsupported platforms refuse the bounded process before
launch. Git observation commands explicitly disable fsmonitor and hooks, and
disable optional Git locks, so configured observer-side commands cannot write
to the selected candidate while normal repository conversion semantics remain
in effect. The disabled hook path is the platform device `NUL` on Windows and
`/dev/null` on Unix; platform-specific fixture controls arm both an fsmonitor
argv spy and a normal Git hook before asserting that observation invokes neither.

The no-mutation snapshot proof is intentionally bounded to the captured set:
worktree listing, direct refs, `HEAD` object text, object-count summary, the
candidate pointer, administrative `gitdir`/`commondir`/`HEAD`/`index`/
`config.worktree`, common `.git/config`, and recursive file digests for the
administrative and candidate directories. It does not claim to snapshot
reflogs, packed-ref bytes, every loose or packed object, or other common-dir
state not represented by those observations.

Platform and integration proof are separate claims. Unix-native process-group
containment remains `NOT_PROVEN` until the Unix executable control runs, and
candidate proof against a moving or different base remains `NOT_PROVEN` for
current-main integration until that exact-tree proof runs.

The production process observer is also unavailable in this slice. When all
other evidence is valid, that missing process evidence keeps the classification
`NOT_PROVEN` rather than inferring inactivity from the absence of lock files.

Administrative `gitdir` and `commondir` records, candidate Git identity, and
HEAD/ref consistency are prerequisites for `CLEAN_RECONSTRUCTABLE`. Missing or
conflicting identity remains fail-closed.
