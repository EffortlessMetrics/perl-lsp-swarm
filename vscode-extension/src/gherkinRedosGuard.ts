// Catastrophic backtracking (ReDoS) requires a quantified group that itself
// contains a quantifier, a backreference, a lookaround, or a quantified group
// containing alternation. A single character class followed by one quantifier
// is linear-time and safe — flagging it produced false negatives or ambiguous
// classifications for ordinary step definitions (see #859).
const POTENTIALLY_EXPENSIVE_REGEX_RE =
  /(?:\([^)]*(?:[+*]|\{[0-9]+(?:,[0-9]*)?\})[^)]*\))[+*{]|\\[1-9]|\(\?<[=!]|(\(\?[!=])/;

function hasQuantifiedAlternation(source: string): boolean {
  const groups: boolean[] = [];
  let inCharacterClass = false;

  for (let index = 0; index < source.length; index += 1) {
    const character = source[index];
    if (character === undefined) {
      continue;
    }

    if (character === '\\') {
      index += 1;
      continue;
    }

    if (inCharacterClass) {
      if (character === ']') {
        inCharacterClass = false;
      }
      continue;
    }

    if (character === '[') {
      inCharacterClass = true;
      continue;
    }

    if (character === '(') {
      groups.push(false);
      continue;
    }

    if (character === '|') {
      const groupIndex = groups.length - 1;
      if (groupIndex >= 0) {
        groups[groupIndex] = true;
      }
      continue;
    }

    if (character !== ')') {
      continue;
    }

    const containsAlternation = groups.pop();
    if (containsAlternation === undefined) {
      continue;
    }

    const next = source[index + 1];
    if (containsAlternation && (next === '+' || next === '*' || next === '{')) {
      return true;
    }

    const parentIndex = groups.length - 1;
    if (containsAlternation && parentIndex >= 0) {
      groups[parentIndex] = true;
    }
  }

  return false;
}

export function isPotentiallyExpensiveRegex(source: string): boolean {
  return POTENTIALLY_EXPENSIVE_REGEX_RE.test(source) || hasQuantifiedAlternation(source);
}
