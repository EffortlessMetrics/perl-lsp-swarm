# Reviewing

Shared contract for every review lens. Each lens agent implements this; none of them
summarizes another.

## Each lens owns and posts its own review

There is no cumulative review. A synthesis step exists to compress many findings into one
human's attention, and this is an agent surface — the reader can hold five full reviews
at once, so compressing them loses the anchors, the falsifiers, and the angles that came
back clean while adding nothing. It also puts a summarizer between the reviewer and the
record, restating findings it did not gather.

So post your own review, to the PR, under your own name. Do not hand findings to a lane
root to relay, and do not restate another lens's findings as your own.

The lane root keeps two jobs and loses a third: it chooses which lenses a PR warrants,
and it dispositions what comes back. It no longer rewrites your review.

## Merge is gated on a ledger, not a verdict

```text
every material dimension has a posted review, or an explicit NOT_APPLICABLE
every posted finding has an evidence-backed disposition
```

Both halves are queries against PR review threads, which is why this replaces a prose
judgment nobody could check. Your posted review is a row in that ledger. A lens that was
dispatched and never posted is a visible hole rather than a silence that reads as clean —
so if you cannot complete your review, post what you have and name what you did not
reach.

## Comment shape

Prefer one comprehensive comment over a stream of small ones. Comprehensiveness is about
the scope of a single judgment, not about waiting to accumulate several: post your review
when *your* review is done. Do not dribble a comment per finding as you go, and do not sit
on a finished review to bundle it with something else.

- **inline review comments** for localized findings, anchored at the line they concern.
  That anchoring is the whole value; do not describe a location in prose when you can
  attach to it;
- **one PR comment** for the lens as a whole — scope, method, what you examined, what you
  did not, and your findings;
- **issue comments, not issue bodies**, for research and history. The body is the claim;
  the comments are the record.

Open with your lens and your stage, so a later reader can tell what ran:

```markdown
## <lens> review — <PR head sha>
Stage: <where this sits in the pipeline>
Scope: <what you examined>
Not examined: <what you deliberately or unavoidably did not>
```

## Budget

API quota is spent by polling, not by posting. The core limit is 5,000 requests per hour
with tighter secondary limits on content creation, so a fixed-interval watcher exhausts it
and a comprehensive comment costs one call.

Never poll on a timer. Read live state when you start and when a named wake event occurs.
Do not re-read unchanged checks, re-list threads you already have, or confirm state you
did not change.

## Evidence rules

- a clean review is valid. Do not manufacture findings to show you looked;
- your own agreement is not evidence, and neither is another agent reaching your
  conclusion from the same source. Independence needs a different source, oracle, method,
  threat model, or environment;
- cite what you read, with `file:line` or run identity, and quote anything load-bearing;
- read labels on your evidence. A verification report describing a candidate branch is not
  describing `main`, and a committed metrics artifact is a snapshot rather than live state;
- where live GitHub policy matters, discover it. Classic branch protection and rulesets
  are independent and additive, so reading one gives a confidently wrong answer;
- report severity honestly. A finding you cannot support costs the lane root more than
  silence.

## Relevant skills

Consume these when your review reaches the situation they own, rather than inventing a
substitute procedure:

| Situation | Skill |
| --- | --- |
| challenging proof discrimination or a vacuous oracle | `review-tests` |
| implementation, ownership, reachability, complexity, rollback | `review-candidate` |
| a finding has been repaired and needs disposition | `address-review-comments` |
| the claim or its owner turns out to be wrong | `prepare-issue` |

## You cannot edit

No lens holds `Edit` or `Write`. A reviewer that repairs what it finds and reports clean
destroys the evidence it was commissioned to produce. Report the defect; the writer fixes
it.

You do hold `Bash`, because posting requires `gh`. Using it to reach the working tree is
out of scope even though nothing stops you.
