// Catastrophic backtracking (ReDoS) requires a quantified group that itself
// contains a quantifier, a backreference, a lookaround, or a quantified group
// containing alternation. A single character class followed by one quantifier
// is linear-time and safe — flagging it produced false negatives or ambiguous
// classifications for ordinary step definitions (see #859). Quantified
// wildcards remain intentionally unsupported because their accepted language
// overlaps every following atom and makes matching cost difficult to bound.
const POTENTIALLY_EXPENSIVE_REGEX_RE =
  /(?:\([^)]*(?:[+*]|\{[0-9]+(?:,[0-9]*)?\})[^)]*\))[+*{]|\\[1-9]|\\k<|\(\?<[=!]|(\(\?[!=])/;

export const MAX_MATCH_REGEX_LENGTH = 256;
export const MAX_MATCH_STEP_TEXT_LENGTH = 512;
export const MAX_MATCH_ATTEMPTS = 20_000;

export interface GherkinMatchBudget {
  tryConsume(): boolean;
}

type BranchFirst = string | 'unknown' | null;

interface GroupFrame {
  branchFirst: BranchFirst;
  branchFirsts: BranchFirst[];
  hasAlternation: boolean;
  nestedOverlap: boolean;
  prefixPending: boolean;
  prefixMode: boolean | 'angle';
}

function branchesOverlap(branchFirsts: BranchFirst[]): boolean {
  const firstChars = new Set<string>();
  for (const first of branchFirsts) {
    if (first === null || first === 'unknown' || firstChars.has(first)) {
      return true;
    }
    firstChars.add(first);
  }
  return false;
}

function unboundedQuantifierEnd(source: string, index: number): number | null {
  const character = source[index];
  if (character === '+' || character === '*') {
    return index + 1;
  }
  if (character !== '{') {
    return null;
  }

  const closingBrace = source.indexOf('}', index + 1);
  return closingBrace > index && /^\{\d+,\}$/.test(source.slice(index, closingBrace + 1))
    ? closingBrace + 1
    : null;
}

function hasUnboundedQuantifier(source: string, index: number): boolean {
  return unboundedQuantifierEnd(source, index) !== null;
}

function hasOverlappingQuantifiedAlternation(source: string): boolean {
  const groups: GroupFrame[] = [];

  for (let index = 0; index < source.length; index += 1) {
    const character = source[index];
    if (character === undefined) {
      continue;
    }

    const currentGroup = groups.at(-1);
    if (currentGroup?.prefixPending) {
      currentGroup.prefixPending = false;
      if (character === '?') {
        currentGroup.prefixMode = true;
        continue;
      }
    }

    if (currentGroup?.prefixMode) {
      if (currentGroup.prefixMode === 'angle') {
        if (character === '>') {
          currentGroup.prefixMode = false;
        }
        continue;
      }
      if (character === '<') {
        currentGroup.prefixMode = 'angle';
      } else if (character === ':' || character === '=' || character === '!') {
        currentGroup.prefixMode = false;
      }
      continue;
    }

    if (character === '\\') {
      index += 1;
      if (currentGroup?.branchFirst === null) {
        currentGroup.branchFirst = 'unknown';
      }
      continue;
    }

    if (character === '[') {
      if (currentGroup?.branchFirst === null) {
        currentGroup.branchFirst = 'unknown';
      }
      for (index += 1; index < source.length; index += 1) {
        const classCharacter = source[index];
        if (classCharacter === '\\') {
          index += 1;
        } else if (classCharacter === ']') {
          break;
        }
      }
      continue;
    }

    if (character === '(') {
      if (currentGroup?.branchFirst === null) {
        currentGroup.branchFirst = 'unknown';
      }
      groups.push({
        branchFirst: null,
        branchFirsts: [],
        hasAlternation: false,
        nestedOverlap: false,
        prefixPending: true,
        prefixMode: false,
      });
      continue;
    }

    if (character === '|') {
      if (currentGroup) {
        currentGroup.branchFirsts.push(currentGroup.branchFirst);
        currentGroup.branchFirst = null;
        currentGroup.hasAlternation = true;
      }
      continue;
    }

    if (character === ')') {
      const group = groups.pop();
      if (group === undefined) {
        continue;
      }

      const quantified = hasUnboundedQuantifier(source, index + 1);
      if (group.hasAlternation) {
        group.branchFirsts.push(group.branchFirst);
        const overlap = branchesOverlap(group.branchFirsts);
        group.nestedOverlap ||= overlap;
      }
      if (quantified && group.nestedOverlap) {
        return true;
      }

      const parentGroup = groups.at(-1);
      if (parentGroup) {
        parentGroup.nestedOverlap ||= group.nestedOverlap;
      }
      continue;
    }

    if (currentGroup?.branchFirst === null && character !== '^' && character !== '$') {
      currentGroup.branchFirst = character === '.' ? 'unknown' : character;
    }
  }

  return false;
}

// The verdict depends only on the pattern and its flags, never on the step text
// it will be matched against, but the match path asks once per
// (definition, step) pair — up to `MAX_MATCH_ATTEMPTS` times per request across
// a workspace's fixed set of step definitions. Memoising the decision keeps the
// chain scan off the repeated path entirely. Bounded and clearable for the same
// reason as the atom cache below: this is a pure memo of a pure function.
const MAX_VERDICT_CACHE_ENTRIES = 512;
const verdictCache = new Map<string, boolean>();

// An entry cap alone does not bound memory, because every key carries the
// pattern text itself. `isSafeGherkinStepMatch` already refuses anything longer
// than `MAX_MATCH_REGEX_LENGTH`, so no production call can present a larger
// one — but that ceiling belonged to the caller, not to the cache, and a future
// caller reaching the guard directly would retain unbounded keys. Admitting
// only keys the ceiling already covers makes the bound a property of the cache
// itself: at most `MAX_VERDICT_CACHE_ENTRIES` and
// `MAX_ATOM_DOMAIN_CACHE_ENTRIES` keys of at most `MAX_MATCH_REGEX_LENGTH`
// characters each.
//
// An oversized pattern is still answered exactly as before. It is simply never
// retained, so a verdict can never depend on cache state.
function isRetainableCacheKey(text: string): boolean {
  return text.length <= MAX_MATCH_REGEX_LENGTH;
}

/**
 * Entry counts for the guard's two memos. Diagnostic only: nothing reads these
 * to make a policy decision, and both memos are pure, so the counts cannot
 * affect a verdict.
 */
export function gherkinRedosGuardCacheStats(): {
  verdictEntries: number;
  atomDomainEntries: number;
} {
  return { verdictEntries: verdictCache.size, atomDomainEntries: atomDomainCache.size };
}

export function isPotentiallyExpensiveRegex(source: string, flags = ''): boolean {
  const normalizedFlags = normalizeGherkinRegexFlags(flags);
  if (normalizedFlags === null) {
    return true;
  }

  const retainable = isRetainableCacheKey(source);
  const key = `${normalizedFlags}\u0000${source}`;
  if (retainable) {
    const cached = verdictCache.get(key);
    if (cached !== undefined) {
      return cached;
    }
  }

  const verdict =
    POTENTIALLY_EXPENSIVE_REGEX_RE.test(source) ||
    hasOverlappingQuantifiedAlternation(source) ||
    hasUnboundedWildcard(source) ||
    hasAdjacentVariableRepetition(source, normalizedFlags) ||
    hasVariableWidthAtomChain(source, normalizedFlags);

  if (retainable) {
    if (verdictCache.size >= MAX_VERDICT_CACHE_ENTRIES) {
      verdictCache.clear();
    }
    verdictCache.set(key, verdict);
  }
  return verdict;
}

function hasUnboundedWildcard(source: string): boolean {
  for (let index = 0; index < source.length;) {
    const atom = readRegexAtom(source, index);
    if (atom === null) {
      index += 1;
      continue;
    }
    if (atom.source === '.' && unboundedQuantifierEnd(source, atom.end) !== null) {
      return true;
    }
    index = atom.end;
  }
  return false;
}

function hasAdjacentVariableRepetition(source: string, flags: string): boolean {
  for (let index = 0; index < source.length;) {
    const left = readRegexAtom(source, index);
    if (left === null) {
      index += 1;
      continue;
    }
    index = left.end;

    const rightStart = unboundedQuantifierEnd(source, left.end);
    if (rightStart === null) {
      continue;
    }
    const right = readRegexAtom(source, rightStart);
    if (
      right !== null &&
      unboundedQuantifierEnd(source, right.end) !== null &&
      atomsMayOverlap(left.source, right.source, flags)
    ) {
      return true;
    }
  }
  return false;
}

interface RegexAtom {
  source: string;
  end: number;
}

function readRegexAtom(source: string, index: number): RegexAtom | null {
  const character = source[index];
  if (character === undefined || '()|^$+*?{}'.includes(character)) {
    return null;
  }
  if (character === '\\') {
    return source[index + 1] === undefined
      ? null
      : { source: source.slice(index, index + 2), end: index + 2 };
  }
  if (character !== '[') {
    return { source: character, end: index + 1 };
  }

  for (let cursor = index + 1; cursor < source.length; cursor += 1) {
    if (source[cursor] === '\\') {
      cursor += 1;
    } else if (source[cursor] === ']') {
      return { source: source.slice(index, cursor + 1), end: cursor + 1 };
    }
  }
  return null;
}

// Establishing one atom's domain costs a `RegExp` construction plus 128
// `test()` calls, and the chain scan asks for every atom in the pattern rather
// than only the rare adjacent unbounded pair the older rule looked at. The
// guard runs on the match hot path — up to `MAX_MATCH_ATTEMPTS` times per
// request — and step definitions across a workspace reuse the same few atoms
// (`\w`, `[^"]`, a literal), so the results are memoised. Measured over a
// 20,000-call budget on a realistic pattern mix: 890 ms uncached, 27 ms cached,
// against 26 ms before the chain rule existed.
//
// The cache is bounded because an adversarial workspace can present unboundedly
// many distinct atoms. It is a pure memo of a pure function, so dropping it
// wholesale is always safe.
const MAX_ATOM_DOMAIN_CACHE_ENTRIES = 1024;
const atomDomainCache = new Map<string, Set<string> | null>();

// The single character-domain authority. `null` means the atom's domain could
// not be established, and every consumer treats that as "may overlap" so a
// complemented, Unicode, or otherwise unsupported atom stays fail-closed.
function simpleAtomDomain(atom: string, flags: string): Set<string> | null {
  const caseInsensitive = flags.includes('i');
  const retainable = isRetainableCacheKey(atom);
  const key = `${caseInsensitive ? 'i' : ''}\u0000${atom}`;
  if (retainable) {
    const cached = atomDomainCache.get(key);
    if (cached !== undefined) {
      return cached;
    }
  }

  const domain = computeSimpleAtomDomain(atom, caseInsensitive);
  if (retainable) {
    if (atomDomainCache.size >= MAX_ATOM_DOMAIN_CACHE_ENTRIES) {
      atomDomainCache.clear();
    }
    atomDomainCache.set(key, domain);
  }
  return domain;
}

function computeSimpleAtomDomain(atom: string, caseInsensitive: boolean): Set<string> | null {
  if (!hasBoundedAtomShape(atom)) {
    return null;
  }

  try {
    const matcher = new RegExp(`^(?:${atom})$`, caseInsensitive ? 'i' : '');
    const domain = new Set<string>();
    for (let code = 0; code <= 0x7f; code += 1) {
      const witness = String.fromCharCode(code);
      if (matcher.test(witness)) {
        domain.add(witness);
      }
    }
    return domain;
  } catch {
    return null;
  }
}

function domainsOverlap(left: Set<string> | null, right: Set<string> | null): boolean {
  if (left === null || right === null) {
    return true;
  }
  for (const character of left) {
    if (right.has(character)) {
      return true;
    }
  }
  return false;
}

function atomsMayOverlap(left: string, right: string, flags: string): boolean {
  if (left === right || (flags.includes('i') && left.toLowerCase() === right.toLowerCase())) {
    return true;
  }

  return domainsOverlap(simpleAtomDomain(left, flags), simpleAtomDomain(right, flags));
}

// #9806. `readRegexAtom` deliberately stops at `(`, so the adjacency scan above
// cannot see a group, and `hasOverlappingQuantifiedAlternation` only fires when
// the ambiguous group carries a quantifier. Chains of adjacent variable-width
// groups therefore reached `RegExp.test()` unclassified. Measured against 511
// `a`s and a final `!`, inside this module's own 256/512 bounds:
// `^(a+)(a+)(a+)(a+)b$` (19 characters) 26.3 s, `^(?:a+)(?:a+)(?:a+)(?:a+)b$`
// 26.3 s, and `^(a|aa)` x40 + `b$` (243 characters) 89.7 s.
//
// This scan reads a group as an atom and propagates its variable-width and
// character-domain facts across `)`. Two atoms only compete when the left one
// can end variably and the right one can begin variably on a shared character,
// so a required separator — including one inside a group, as in `(\w+ )` —
// still breaks the chain.
//
// A chain is scored by how many ways the engine can split one run of input
// across it, in bits, rather than by counting atoms. Counting atoms conflates
// two families whose costs differ by orders of magnitude at the same length:
//
//   N adjacent    (a+)xN      (a|aa)xN    (?:ab )?xN
//   3             13 ms       0.13 ms     0.06 ms
//   4             636 ms      0.09 ms     0.08 ms
//   20            timeout     53 ms       35 ms
//   25            timeout     1.8 s       1.1 s
//
// An unbounded atom (`+`, `*`, `{n,}`) can absorb anywhere from nothing to the
// whole step text, so its edges are worth log2(MAX_MATCH_STEP_TEXT_LENGTH)
// bits. A bounded one (`?`, `{n,m}`, or a group whose branches merely differ in
// width) is worth only log2 of its alternative count — one bit for `(?:the )?`.
// The chain's cost is the sum over its seams, each worth the lesser of the two
// edges that meet there, which is the exponent on the work the engine does.
//
// One unbounded seam is the budget. `^(a+)(a+)b$` spends exactly that, is
// quadratic, measures 1.2 ms against 511 `a`s, and is the shape ordinary step
// definitions use; a second unbounded seam doubles the exponent and is refused.
// Bounded optionality only reaches the same total at around nineteen atoms,
// which the measurements above put well inside the safe region and far beyond
// anything a real step definition carries.
//
// Scoring by atom count instead rejects the ordinary cucumber idiom of stacked
// optional phrases — `^(?:the )?(?:new )?(?:admin )?user "([^"]+)" exists$`
// resolves in 0.078 ms — which is exactly the #859 over-rejection #6158 exists
// to avoid.
const UNBOUNDED_ATOM_BITS = Math.log2(MAX_MATCH_STEP_TEXT_LENGTH);
const MAX_CHAIN_AMBIGUITY_BITS = 2 * UNBOUNDED_ATOM_BITS - 1;
const MAX_GROUP_ANALYSIS_DEPTH = 32;
// A nullable atom splits the branch in two; equivalent trailing boundary states
// are merged after each atom so the analysis remains bounded without score-only
// pruning. `bits` is how much freedom this boundary offers a neighbouring atom: zero for
// a fixed edge, which cannot compete and therefore separates a chain.
interface EdgeFacts {
  domain: Set<string> | null;
  bits: number;
}

interface ChainAtom {
  end: number;
  leading: EdgeFacts;
  trailing: EdgeFacts;
  union: Set<string> | null;
  fixedWidth: number | null;
  nullable: boolean;
  chain: boolean;
  // Freedom that can actually reach each boundary, in bits. An edge can be
  // fixed while the interior still varies — `(aa*a)` begins and ends on a
  // literal `a` — and that interior is what lets consecutive copies
  // redistribute input. But variability sealed off by a required separator
  // cannot reach the boundary at all: in `(a+b+a+)` the `b+` isolates the
  // leading `a+` from the trailing one, so only one `a+` worth of freedom
  // meets each seam. Counting every atom instead refuses `^((a+b+a+)){2}$`,
  // which resolves in 0.1 ms.
  leadingRunBits: number;
  trailingRunBits: number;
}

type SequenceFacts = Omit<ChainAtom, 'end'>;

interface QuantifierFacts {
  end: number;
  bits: number;
  repeat: number | null;
  nullable: boolean;
  // Largest number of times the atom may repeat; `null` when unbounded. A
  // repeat of R places R - 1 seams inside the atom itself.
  maxRepeat: number | null;
}

function unionDomains(domains: (Set<string> | null)[]): Set<string> | null {
  const union = new Set<string>();
  for (const domain of domains) {
    if (domain === null) {
      return null;
    }
    for (const character of domain) {
      union.add(character);
    }
  }
  return union;
}

function readQuantifier(source: string, index: number): QuantifierFacts {
  const character = source[index];
  const lazyEnd = (end: number): number => (source[end] === '?' ? end + 1 : end);

  if (character === '+') {
    return {
      end: lazyEnd(index + 1),
      bits: UNBOUNDED_ATOM_BITS,
      repeat: null,
      nullable: false,
      maxRepeat: null,
    };
  }
  if (character === '*') {
    return {
      end: lazyEnd(index + 1),
      bits: UNBOUNDED_ATOM_BITS,
      repeat: null,
      nullable: true,
      maxRepeat: null,
    };
  }
  if (character === '?') {
    // Present or absent: one bit, not an unbounded run.
    return { end: lazyEnd(index + 1), bits: 1, repeat: null, nullable: true, maxRepeat: 1 };
  }
  if (character === '{') {
    const closingBrace = source.indexOf('}', index + 1);
    const body = closingBrace > index ? source.slice(index + 1, closingBrace) : null;
    const bounds = body === null ? null : /^(\d+)(?:,(\d*))?$/.exec(body);
    if (bounds !== null) {
      const minimum = Number(bounds[1]);
      const maximum =
        bounds[2] === undefined ? minimum : bounds[2] === '' ? null : Number(bounds[2]);
      if (maximum === null) {
        return {
          end: lazyEnd(closingBrace + 1),
          bits: UNBOUNDED_ATOM_BITS,
          repeat: null,
          nullable: minimum === 0,
          maxRepeat: null,
        };
      }
      const choices = maximum - minimum + 1;
      return {
        end: lazyEnd(closingBrace + 1),
        bits: choices > 1 ? Math.min(Math.log2(choices), UNBOUNDED_ATOM_BITS) : 0,
        repeat: choices > 1 ? null : minimum,
        nullable: minimum === 0,
        maxRepeat: maximum,
      };
    }
  }

  return { end: index, bits: 0, repeat: 1, nullable: false, maxRepeat: 1 };
}

// A position the parser cannot classify. It neither varies nor overlaps, so it
// separates its neighbours the way a required literal does.
function opaqueAtom(end: number): ChainAtom {
  return {
    end,
    leading: { domain: null, bits: 0 },
    trailing: { domain: null, bits: 0 },
    union: null,
    fixedWidth: null,
    // An unanalysable construct may match nothing — a zero-width modifier
    // group, for instance — so it cannot be relied on to separate the atoms
    // around it. Treating it as nullable keeps a chain alive across it rather
    // than letting an unknown reset the run, which would fail open.
    nullable: true,
    chain: false,
    leadingRunBits: 0,
    trailingRunBits: 0,
  };
}

function findGroupEnd(source: string, index: number): number | null {
  let depth = 0;
  for (let cursor = index; cursor < source.length; cursor += 1) {
    const character = source[cursor];
    if (character === '\\') {
      cursor += 1;
    } else if (character === '[') {
      for (cursor += 1; cursor < source.length; cursor += 1) {
        if (source[cursor] === '\\') {
          cursor += 1;
        } else if (source[cursor] === ']') {
          break;
        }
      }
    } else if (character === '(') {
      depth += 1;
    } else if (character === ')') {
      depth -= 1;
      if (depth === 0) {
        return cursor;
      }
    }
  }
  return null;
}

function readGroupContentStart(source: string, index: number, groupEnd: number): number | null {
  if (source[index + 1] !== '?') {
    return index + 1;
  }
  if (source[index + 2] === ':') {
    return index + 3;
  }
  if (source[index + 2] === '<' && source[index + 3] !== '=' && source[index + 3] !== '!') {
    const nameEnd = source.indexOf('>', index + 3);
    return nameEnd > index && nameEnd < groupEnd ? nameEnd + 1 : null;
  }
  // Lookarounds and any other `(?…)` construct. `POTENTIALLY_EXPENSIVE_REGEX_RE`
  // already rejects the lookaround forms before this scan runs; anything else
  // stays unanalysed rather than being guessed at.
  return null;
}

function readChainAtom(
  source: string,
  index: number,
  flags: string,
  depth: number,
): ChainAtom | null {
  const character = source[index];
  if (character === undefined) {
    return null;
  }

  if (character === '(') {
    const groupEnd = findGroupEnd(source, index);
    if (groupEnd === null) {
      return null;
    }
    const quantifier = readQuantifier(source, groupEnd + 1);
    if (depth >= MAX_GROUP_ANALYSIS_DEPTH) {
      // Fail closed: an unanalysed group varies over an unknown domain, so it
      // links to whatever sits beside it.
      // Fail closed. Wrapping a chain in enough groups to exhaust the
      // analysis budget does not make it cheaper to match — 33 wrappers around
      // `(a+)(a+)(a+)(a+)b` is 83 characters and takes over two seconds at 120
      // — so an unanalysed group reports a chain rather than swallowing one.
      return {
        end: quantifier.end,
        leading: { domain: null, bits: UNBOUNDED_ATOM_BITS },
        trailing: { domain: null, bits: UNBOUNDED_ATOM_BITS },
        union: null,
        fixedWidth: null,
        nullable: quantifier.nullable,
        chain: true,
        leadingRunBits: UNBOUNDED_ATOM_BITS,
        trailingRunBits: UNBOUNDED_ATOM_BITS,
      };
    }

    const contentStart = readGroupContentStart(source, index, groupEnd);
    if (contentStart === null) {
      return opaqueAtom(quantifier.end);
    }

    const group = analyzeAlternatives(source, contentStart, groupEnd, flags, depth + 1);
    // Repeating a group places a seam between each pair of consecutive copies,
    // exactly as writing those copies out would. `((a+))+` is eight characters
    // and takes 81 s on thirty `a`s, because the older denylist regex cannot
    // see a quantifier across the extra `)` and this scan reads the whole
    // construct as one atom. Charging the group against itself catches it, and
    // catches `(\)?a+)+`, `((a?)){20}` and `((a+|a)){3,7}` with it.
    //
    // A group whose own boundary is fixed seams with nothing however often it
    // repeats, which is why `(\w+ )+` and `(cat|dog)+` stay accepted.
    // Between two different atoms the seam is the narrower edge, because both
    // sides must be able to move. A group repeated against itself is not that
    // case: the copies are identical, so whatever freedom exists at the
    // boundary is available on both sides of it. `(a+a{2})` ends in a fixed
    // `a{2}`, but the `a+` before it still slides the boundary — and that tail
    // is made of the same character the next copy consumes, so it blocks
    // nothing. Measured: `^((a+a{2})){5}$` takes 5.4 s at 120 characters and
    // does not finish at 200. Disjoint edges are what actually block a seam,
    // which is why `(\w+ )+` and `((a+b+)){5}` stay accepted.
    // What blocks a self-seam is a boundary the neighbouring copy cannot
    // cross, so disjoint edges — `(\w+ )+`, `((a+b+)){5}` — are safe however
    // often the group repeats. When the edges do overlap, the cost is whatever
    // the interior can redistribute, not what the edge atoms alone vary by:
    // `((aa*a))+` begins and ends on a fixed `a` yet exceeds nine seconds on
    // forty characters.
    // Both sides of a self-seam must be able to move, so the seam is worth the
    // lesser of the freedom reaching each boundary.
    const selfSeamBits = domainsOverlap(group.trailing.domain, group.leading.domain)
      ? Math.min(group.trailingRunBits, group.leadingRunBits)
      : 0;
    const selfChain =
      selfSeamBits > 0 &&
      (quantifier.maxRepeat === null ||
        (quantifier.maxRepeat - 1) * selfSeamBits > MAX_CHAIN_AMBIGUITY_BITS);
    // A repeating group can present any of its characters at either boundary, so
    // the quantifier widens both edges to the group's whole domain and adds its
    // own freedom on top of whatever the group already offers there.
    // Widening a boundary to the group's whole domain is only justified when the
    // group can appear more than once: the second copy's first character then
    // really can sit against the first copy's last. An optional group appears at
    // most once, so its real edges still apply — widening them invents overlaps
    // that are not reachable, and `^(?:x+a+)?(?:b+a+)?(?:c+a+)?$` (0.1 ms, every
    // real boundary disjoint) would be refused for a seam it cannot form.
    // Optionality still contributes its own bit either way.
    const repeats = quantifier.maxRepeat === null || quantifier.maxRepeat > 1;
    return {
      end: quantifier.end,
      leading: repeats
        ? { domain: group.union, bits: quantifier.bits + group.leading.bits }
        : { domain: group.leading.domain, bits: quantifier.bits + group.leading.bits },
      trailing: repeats
        ? { domain: group.union, bits: quantifier.bits + group.trailing.bits }
        : { domain: group.trailing.domain, bits: quantifier.bits + group.trailing.bits },
      union: group.union,
      fixedWidth:
        quantifier.repeat === null || group.fixedWidth === null
          ? null
          : group.fixedWidth * quantifier.repeat,
      nullable: quantifier.nullable || group.nullable,
      chain: group.chain || selfChain,
      leadingRunBits: quantifier.bits + group.leadingRunBits,
      trailingRunBits: quantifier.bits + group.trailingRunBits,
    };
  }

  const atom = readRegexAtom(source, index);
  if (atom === null) {
    return null;
  }

  const quantifier = readQuantifier(source, atom.end);
  const domain = simpleAtomDomain(atom.source, flags);
  const edge: EdgeFacts = { domain, bits: quantifier.bits };
  return {
    end: quantifier.end,
    leading: edge,
    trailing: edge,
    leadingRunBits: quantifier.bits,
    trailingRunBits: quantifier.bits,
    union: domain,
    fixedWidth: quantifier.repeat,
    nullable: quantifier.nullable,
    chain: false,
  };
}

// Walk in from one end, accumulating freedom while each atom can still reach
// the boundary. A required atom over a disjoint domain seals the rest off.
function connectedRunBits(
  ordered: ChainAtom[],
  boundaryDomain: Set<string> | null,
  side: 'leading' | 'trailing',
): number {
  let total = 0;
  for (const atom of ordered) {
    if (!domainsOverlap(boundaryDomain, atom.union)) {
      // Only a *required* atom seals the run. A nullable one can vanish and
      // leave its neighbours meeting directly, so it cannot be a wall — and a
      // zero-width group has an empty domain, which is disjoint from
      // everything. Breaking on those made `(?:)` a fake separator:
      // `^((a+))+b$` is rejected, yet `^((a+(?:)))+b$` — sixteen characters,
      // exponential, 118 ms at n=24 — was accepted.
      if (!atom.nullable) {
        break;
      }
      continue;
    }
    total += side === 'leading' ? atom.leadingRunBits : atom.trailingRunBits;
  }
  return total;
}

interface BranchPath {
  runBits: number;
  trailing: EdgeFacts | null;
}

function sameDomain(left: Set<string> | null, right: Set<string> | null): boolean {
  if (left === null || right === null) {
    return left === right;
  }
  if (left.size !== right.size) {
    return false;
  }
  for (const character of left) {
    if (!right.has(character)) {
      return false;
    }
  }
  return true;
}

function sameTrailing(left: EdgeFacts | null, right: EdgeFacts | null): boolean {
  return (
    (left === null && right === null) ||
    (left !== null &&
      right !== null &&
      left.bits === right.bits &&
      sameDomain(left.domain, right.domain))
  );
}

// For a fixed trailing boundary, only the highest accumulated score matters:
// every future atom sees the same trailing domain and bits, and therefore the
// higher-scoring path dominates the lower one. Unlike the former top-eight
// heuristic, this preserves every distinct future-relevant boundary state while
// keeping at most one path per state (bounded by the atoms in this source).
function retainBestBoundaryStates(paths: BranchPath[]): BranchPath[] {
  const retained: BranchPath[] = [];
  for (const path of paths) {
    const existing = retained.find((candidate) => sameTrailing(candidate.trailing, path.trailing));
    if (existing === undefined) {
      retained.push(path);
    } else if (path.runBits > existing.runBits) {
      existing.runBits = path.runBits;
    }
  }
  return retained;
}

function summarizeBranch(atoms: ChainAtom[]): SequenceFacts {
  let chain = atoms.some((atom) => atom.chain);
  // A nullable atom splits the branch into two mutually exclusive paths — one
  // where it matches and one where it vanishes — and a chain reachable on
  // neither path is not a chain. `^([ab]+)(a?)(b+)(b+)c$` is only dangerous on
  // the absent path, where three unbounded repetitions meet (213 ms at the
  // ceiling); `^(a+)(?:a+b+)?(a+)c$` has one seam on each path and is dangerous
  // on neither (2.7 ms). Merging the two into one edge would rule the first
  // safe or the second unsafe, so each path carries its own score.
  let paths: BranchPath[] = [{ runBits: 0, trailing: null }];

  for (const atom of atoms) {
    const next: BranchPath[] = [];
    for (const path of paths) {
      const previousTrailing = path.trailing;
      const linked =
        previousTrailing !== null &&
        previousTrailing.bits > 0 &&
        atom.leading.bits > 0 &&
        domainsOverlap(previousTrailing.domain, atom.leading.domain);

      // Ambiguity lives at the seam between two atoms, not inside either one,
      // and a seam is only as free as its narrower side: `(?:(\w+) )?` ends in
      // a required space, so it hands the next atom one bit of freedom however
      // unbounded its interior is.
      const runBits =
        linked && previousTrailing !== null
          ? path.runBits + Math.min(previousTrailing.bits, atom.leading.bits)
          : 0;
      if (runBits > MAX_CHAIN_AMBIGUITY_BITS) {
        chain = true;
      }
      next.push({ runBits, trailing: atom.trailing });
      // The path in which this atom is absent keeps the state that preceded it,
      // so its neighbours are seen meeting directly.
      if (atom.nullable && previousTrailing !== null) {
        next.push(path);
      }
    }
    if (chain) {
      break;
    }
    // Nullable atoms can produce many paths, but paths with the same trailing
    // domain and boundary score have identical futures. Keep only the highest
    // score for each such state; never discard a state merely because another
    // state has a larger score.
    paths = retainBestBoundaryStates(next);
  }

  const leadingParts: ChainAtom[] = [];
  for (const atom of atoms) {
    leadingParts.push(atom);
    if (!atom.nullable) {
      break;
    }
  }
  const trailingParts: ChainAtom[] = [];
  for (let index = atoms.length - 1; index >= 0; index -= 1) {
    const atom = atoms[index];
    if (atom === undefined) {
      break;
    }
    trailingParts.push(atom);
    if (!atom.nullable) {
      break;
    }
  }

  const widths = atoms.map((atom) => atom.fixedWidth);
  const leadingDomain = unionDomains(leadingParts.map((atom) => atom.leading.domain));
  const trailingDomain = unionDomains(trailingParts.map((atom) => atom.trailing.domain));
  return {
    leading: {
      domain: leadingDomain,
      bits: Math.max(0, ...leadingParts.map((atom) => atom.leading.bits)),
    },
    trailing: {
      domain: trailingDomain,
      bits: Math.max(0, ...trailingParts.map((atom) => atom.trailing.bits)),
    },
    union: unionDomains(atoms.map((atom) => atom.union)),
    leadingRunBits: connectedRunBits(atoms, leadingDomain, 'leading'),
    trailingRunBits: connectedRunBits([...atoms].reverse(), trailingDomain, 'trailing'),
    fixedWidth: widths.includes(null)
      ? null
      : widths.reduce((total: number, width) => total + (width ?? 0), 0),
    nullable: atoms.every((atom) => atom.nullable),
    chain,
  };
}

function summarizeAlternatives(branches: SequenceFacts[]): SequenceFacts {
  const widths = branches.map((branch) => branch.fixedWidth);
  // Branches of different lengths let the group end at more than one offset, so
  // its boundaries vary even with no quantifier anywhere — the `(a|aa)` shape.
  // The choice between them is worth log2(branch count) bits, so an ambiguous
  // two-branch group costs one bit, the same as `?`, which is what the
  // measurements say it is worth.
  const alternationBits =
    branches.length > 1 && (widths.includes(null) || new Set(widths).size > 1)
      ? Math.log2(branches.length)
      : 0;

  return {
    leading: {
      domain: unionDomains(branches.map((branch) => branch.leading.domain)),
      bits: alternationBits + Math.max(0, ...branches.map((branch) => branch.leading.bits)),
    },
    trailing: {
      domain: unionDomains(branches.map((branch) => branch.trailing.domain)),
      bits: alternationBits + Math.max(0, ...branches.map((branch) => branch.trailing.bits)),
    },
    union: unionDomains(branches.map((branch) => branch.union)),
    leadingRunBits:
      alternationBits + Math.max(0, ...branches.map((branch) => branch.leadingRunBits)),
    trailingRunBits:
      alternationBits + Math.max(0, ...branches.map((branch) => branch.trailingRunBits)),
    fixedWidth: alternationBits > 0 ? null : (widths[0] ?? null),
    nullable: branches.some((branch) => branch.nullable),
    chain: branches.some((branch) => branch.chain),
  };
}

function analyzeAlternatives(
  source: string,
  start: number,
  end: number,
  flags: string,
  depth: number,
): SequenceFacts {
  const branches: SequenceFacts[] = [];
  let atoms: ChainAtom[] = [];
  let index = start;

  while (index < end) {
    const character = source[index];
    if (character === '|') {
      branches.push(summarizeBranch(atoms));
      atoms = [];
      index += 1;
      continue;
    }
    // Anchors are zero-width and cannot separate their neighbours.
    if (character === '^' || character === '$') {
      index += 1;
      continue;
    }
    // Neither can a word-boundary assertion, which `readRegexAtom` would
    // otherwise hand back as an ordinary fixed atom. Treating `\B` as a
    // separator made `^a+\Ba+\Ba+\Ba+b$` — the same shape as
    // `^(a+)(a+)(a+)(a+)b$`, and 26.8 s on the same input — look separated.
    if (character === '\\' && (source[index + 1] === 'b' || source[index + 1] === 'B')) {
      index += 2;
      continue;
    }

    const atom = readChainAtom(source, index, flags, depth);
    if (atom === null || atom.end <= index) {
      atoms.push(opaqueAtom(index + 1));
      index += 1;
      continue;
    }
    atoms.push(atom);
    index = atom.end;
  }

  branches.push(summarizeBranch(atoms));
  return summarizeAlternatives(branches);
}

function hasVariableWidthAtomChain(source: string, flags: string): boolean {
  return analyzeAlternatives(source, 0, source.length, flags, 0).chain;
}

function hasBoundedAtomShape(atom: string): boolean {
  if (atom.startsWith('[') && atom.endsWith(']')) {
    const content = atom.slice(1, -1);
    return (
      !content.startsWith('^') &&
      /^[\x00-\x7f]*$/.test(content) &&
      /^(?:\\[dws]|\\[^A-Za-z0-9]|[^\\])*$/.test(content)
    );
  }
  return /^[\x00-\x7f]$/.test(atom) || /^\\(?:[dws]|[^A-Za-z0-9])$/.test(atom);
}

export function normalizeGherkinRegexFlags(flags: string): string | null {
  let normalized = '';
  for (const flag of flags) {
    if (flag !== 'i' && flag !== 'm' && flag !== 's') {
      // Perl flags such as x have no equivalent in this JavaScript matching
      // path. Silently dropping them can turn a non-match into a false match.
      return null;
    }
    if (!normalized.includes(flag)) {
      normalized += flag;
    }
  }
  return normalized;
}

export function isSafeGherkinStepMatch(source: string, stepText: string, flags = ''): boolean {
  if (source.length > MAX_MATCH_REGEX_LENGTH || stepText.length > MAX_MATCH_STEP_TEXT_LENGTH) {
    return false;
  }

  return !isPotentiallyExpensiveRegex(source, flags);
}

export function createGherkinMatchBudget(): GherkinMatchBudget {
  let attempts = 0;

  return {
    tryConsume(): boolean {
      if (attempts >= MAX_MATCH_ATTEMPTS) {
        return false;
      }

      attempts += 1;
      return true;
    },
  };
}
