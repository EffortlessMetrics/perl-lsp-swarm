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

export function isPotentiallyExpensiveRegex(source: string, flags = ''): boolean {
  const normalizedFlags = normalizeGherkinRegexFlags(flags);
  if (normalizedFlags === null) {
    return true;
  }

  const key = `${normalizedFlags}\u0000${source}`;
  const cached = verdictCache.get(key);
  if (cached !== undefined) {
    return cached;
  }

  const verdict =
    POTENTIALLY_EXPENSIVE_REGEX_RE.test(source) ||
    hasOverlappingQuantifiedAlternation(source) ||
    hasUnboundedWildcard(source) ||
    hasAdjacentVariableRepetition(source, normalizedFlags) ||
    hasVariableWidthAtomChain(source, normalizedFlags);

  if (verdictCache.size >= MAX_VERDICT_CACHE_ENTRIES) {
    verdictCache.clear();
  }
  verdictCache.set(key, verdict);
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
  const key = `${caseInsensitive ? 'i' : ''}\u0000${atom}`;
  const cached = atomDomainCache.get(key);
  if (cached !== undefined) {
    return cached;
  }

  const domain = computeSimpleAtomDomain(atom, caseInsensitive);
  if (atomDomainCache.size >= MAX_ATOM_DOMAIN_CACHE_ENTRIES) {
    atomDomainCache.clear();
  }
  atomDomainCache.set(key, domain);
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

// `bits` is how much freedom this boundary offers a neighbouring atom: zero for
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
}

type SequenceFacts = Omit<ChainAtom, 'end'>;

interface QuantifierFacts {
  end: number;
  bits: number;
  repeat: number | null;
  nullable: boolean;
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
    return { end: lazyEnd(index + 1), bits: UNBOUNDED_ATOM_BITS, repeat: null, nullable: false };
  }
  if (character === '*') {
    return { end: lazyEnd(index + 1), bits: UNBOUNDED_ATOM_BITS, repeat: null, nullable: true };
  }
  if (character === '?') {
    // Present or absent: one bit, not an unbounded run.
    return { end: lazyEnd(index + 1), bits: 1, repeat: null, nullable: true };
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
        };
      }
      const choices = maximum - minimum + 1;
      return {
        end: lazyEnd(closingBrace + 1),
        bits: choices > 1 ? Math.min(Math.log2(choices), UNBOUNDED_ATOM_BITS) : 0,
        repeat: choices > 1 ? null : minimum,
        nullable: minimum === 0,
      };
    }
  }

  return { end: index, bits: 0, repeat: 1, nullable: false };
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
    nullable: false,
    chain: false,
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
      return {
        end: quantifier.end,
        leading: { domain: null, bits: UNBOUNDED_ATOM_BITS },
        trailing: { domain: null, bits: UNBOUNDED_ATOM_BITS },
        union: null,
        fixedWidth: null,
        nullable: quantifier.nullable,
        chain: false,
      };
    }

    const contentStart = readGroupContentStart(source, index, groupEnd);
    if (contentStart === null) {
      return opaqueAtom(quantifier.end);
    }

    const group = analyzeAlternatives(source, contentStart, groupEnd, flags, depth + 1);
    // A repeating group can present any of its characters at either boundary, so
    // the quantifier widens both edges to the group's whole domain and adds its
    // own freedom on top of whatever the group already offers there.
    const repeated = quantifier.bits > 0;
    return {
      end: quantifier.end,
      leading: repeated
        ? { domain: group.union, bits: quantifier.bits + group.leading.bits }
        : group.leading,
      trailing: repeated
        ? { domain: group.union, bits: quantifier.bits + group.trailing.bits }
        : group.trailing,
      union: group.union,
      fixedWidth:
        quantifier.repeat === null || group.fixedWidth === null
          ? null
          : group.fixedWidth * quantifier.repeat,
      nullable: quantifier.nullable || group.nullable,
      chain: group.chain,
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
    union: domain,
    fixedWidth: quantifier.repeat,
    nullable: quantifier.nullable,
    chain: false,
  };
}

function summarizeBranch(atoms: ChainAtom[]): SequenceFacts {
  let chain = atoms.some((atom) => atom.chain);
  let runBits = 0;
  let previousTrailing: EdgeFacts | null = null;

  for (const atom of atoms) {
    const linked =
      previousTrailing !== null &&
      previousTrailing.bits > 0 &&
      atom.leading.bits > 0 &&
      domainsOverlap(previousTrailing.domain, atom.leading.domain);

    // An atom that can match nothing cannot separate the atoms around it.
    if (!linked && atom.nullable && previousTrailing !== null) {
      continue;
    }

    // Ambiguity lives at the seam between two atoms, not inside either one, and
    // a seam is only as free as its narrower side: `(?:(\w+) )?` ends in a
    // required space, so it hands the next atom one bit of freedom however
    // unbounded its interior is.
    runBits =
      linked && previousTrailing !== null
        ? runBits + Math.min(previousTrailing.bits, atom.leading.bits)
        : 0;
    if (runBits > MAX_CHAIN_AMBIGUITY_BITS) {
      chain = true;
      break;
    }
    previousTrailing = atom.trailing;
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
  return {
    leading: {
      domain: unionDomains(leadingParts.map((atom) => atom.leading.domain)),
      bits: Math.max(0, ...leadingParts.map((atom) => atom.leading.bits)),
    },
    trailing: {
      domain: unionDomains(trailingParts.map((atom) => atom.trailing.domain)),
      bits: Math.max(0, ...trailingParts.map((atom) => atom.trailing.bits)),
    },
    union: unionDomains(atoms.map((atom) => atom.union)),
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
