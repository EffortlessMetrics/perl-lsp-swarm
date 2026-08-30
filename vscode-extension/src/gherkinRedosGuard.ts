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

export function isPotentiallyExpensiveRegex(source: string, flags = ''): boolean {
  const normalizedFlags = normalizeGherkinRegexFlags(flags);
  if (normalizedFlags === null) {
    return true;
  }

  return (
    POTENTIALLY_EXPENSIVE_REGEX_RE.test(source) ||
    hasOverlappingQuantifiedAlternation(source) ||
    hasUnboundedWildcard(source) ||
    hasAdjacentVariableRepetition(source, normalizedFlags)
  );
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

function atomsMayOverlap(left: string, right: string, flags: string): boolean {
  if (left === right || (flags.includes('i') && left.toLowerCase() === right.toLowerCase())) {
    return true;
  }

  if (!hasBoundedAtomShape(left) || !hasBoundedAtomShape(right)) {
    // Complemented, Unicode, or otherwise unknown atoms stay fail-closed. Only
    // a pair whose supported character domains are proven disjoint may pass.
    return true;
  }

  try {
    const atomFlags = flags.includes('i') ? 'i' : '';
    const leftAtom = new RegExp(`^(?:${left})$`, atomFlags);
    const rightAtom = new RegExp(`^(?:${right})$`, atomFlags);
    for (let code = 0; code <= 0x7f; code += 1) {
      const witness = String.fromCharCode(code);
      if (leftAtom.test(witness) && rightAtom.test(witness)) {
        return true;
      }
    }
    return false;
  } catch {
    return true;
  }
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
  for (const flag of flags.toLowerCase()) {
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
