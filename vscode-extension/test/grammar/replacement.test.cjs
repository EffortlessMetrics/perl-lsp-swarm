const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { before, test } = require('node:test');
const textmate = require('vscode-textmate');
const oniguruma = require('vscode-oniguruma');

const extension = path.resolve(__dirname, '../..');
const scope = 'string.regexp.replace.perl';
let grammar;
before(async () => {
    await oniguruma.loadWASM(
        fs.readFileSync(require.resolve('vscode-oniguruma/release/onig.wasm')),
    );
    const manifest = JSON.parse(fs.readFileSync(path.join(extension, 'package.json'), 'utf8'));
    const registration = manifest.contributes.grammars.find((entry) => entry.language === 'perl');
    const registry = new textmate.Registry({
        onigLib: Promise.resolve({
            createOnigScanner: (patterns) => new oniguruma.OnigScanner(patterns),
            createOnigString: (value) => new oniguruma.OnigString(value),
        }),
        loadGrammar: async (requested) =>
            requested === registration.scopeName
                ? textmate.parseRawGrammar(
                      fs.readFileSync(path.join(extension, registration.path), 'utf8'),
                      registration.path,
                  )
                : null,
    });
    grammar = await registry.loadGrammar(registration.scopeName);
});

function scopesAt(tokens, offset) {
    const token = tokens.find((token) => token.startIndex <= offset && offset < token.endIndex);
    assert.ok(token, `token at ${offset}`);
    return token.scopes;
}

const expressions = [
    's/pattern/replacement/gir',
    's///r',
    's/foo//ee',
    's//bar/g',
    's{pattern}{replacement}gir',
    's{}{}ee',
    's{a{2}}{b{c}}r',
    's(pattern)(replacement)r',
    's<pattern><replacement>ee',
    'tr/a-z/A-Z/cdr',
    'tr[a-z][A-Z]r',
    'y/a-z/A-Z/r',
    'y[a-z][A-Z]cd',
    String.raw`s/a\/b/c\/d/ee`,
    String.raw`tr/a\/b/c\/d/r`,
    String.raw`s{a\}b}{c\}d}r`,
    String.raw`tr[a\]b][c\]d]r`,
    's!foo!bar!r',
    's#foo#bar#ee',
    's{foo}#bar#r',
    's{foo} {bar}r',
    's{foo}/bar/r',
    's{foo}[bar]r',
    'tr[a-z]{A-Z}r',
    's/foo/$value @array &literal my print/ee',
];
for (const expression of expressions) {
    test(`both halves and flags: ${expression}`, () => {
        const line = `${expression}; my $after = 1;`;
        const result = grammar.tokenizeLine(line);
        for (let offset = 0; offset < expression.length; offset++) {
            const scopes = scopesAt(result.tokens, offset);
            assert.ok(
                scopes.includes(scope),
                `replacement scope at ${offset} (${line[offset]}): ${scopes}`,
            );
            if (offset >= (expression.startsWith('tr') ? 2 : 1)) {
                assert.ok(
                    !scopes.some((scope) => /^(variable\.|support\.function\.)/.test(scope)),
                    scopes.join(' '),
                );
            }
        }
        assert.ok(scopesAt(result.tokens, 0).includes('keyword.other.perl'));
        assert.ok(!scopesAt(result.tokens, expression.length).includes(scope), 'semicolon is code');
        assert.ok(
            scopesAt(result.tokens, line.indexOf('my', expression.length)).includes(
                'keyword.other.perl',
            ),
        );
        assert.ok(
            scopesAt(result.tokens, line.indexOf('$after')).includes('variable.other.scalar.perl'),
        );
    });
}
for (const expression of [
    String.raw`s/a\/b/c\/d/ee`,
    String.raw`s{a\}b}{c\}d}r`,
    String.raw`tr[a\]b][c\]d]r`,
]) {
    test(`escapes remain escapes: ${expression}`, () => {
        const result = grammar.tokenizeLine(expression);
        for (
            let offset = expression.indexOf('\\');
            offset >= 0;
            offset = expression.indexOf('\\', offset + 2)
        ) {
            assert.ok(scopesAt(result.tokens, offset).includes('constant.character.escape.perl'));
        }
    });
}
for (const [malformed, closure] of [
    ['s/foo/bar', '/r'],
    ['s/foo', '/bar/r'],
    ['s{foo}{bar', '}r'],
    ['tr[a-z][A-Z', ']r'],
]) {
    test(`unterminated input stays lexical until its closing delimiter: ${malformed}`, () => {
        const first = grammar.tokenizeLine(malformed);
        assert.ok(scopesAt(first.tokens, malformed.length - 1).includes(scope));
        const next = `${closure}; my $after = 1;`;
        const recovered = grammar.tokenizeLine(next, first.ruleStack);
        assert.ok(scopesAt(recovered.tokens, 0).includes(scope));
        assert.ok(!scopesAt(recovered.tokens, closure.length).includes(scope));
        assert.ok(
            scopesAt(recovered.tokens, next.indexOf('$after')).includes(
                'variable.other.scalar.perl',
            ),
        );
    });
}
test('a missing paired replacement opener does not swallow following code', () => {
    const line = 's{foo} my $after = 1;';
    const result = grammar.tokenizeLine(line);
    assert.ok(
        scopesAt(result.tokens, line.indexOf('$after')).includes('variable.other.scalar.perl'),
    );
});
test('matching and following substitutions retain their own boundaries', () => {
    const line = 's/foo/bar/g; s/baz/qux/r; m/next/i; my $after = 1;';
    const result = grammar.tokenizeLine(line);
    assert.ok(scopesAt(result.tokens, line.indexOf('qux')).includes(scope));
    const matchScopes = scopesAt(result.tokens, line.indexOf('next'));
    assert.ok(matchScopes.includes('string.regexp.perl'));
    assert.ok(!matchScopes.includes(scope));
    assert.ok(
        scopesAt(result.tokens, line.indexOf('$after')).includes('variable.other.scalar.perl'),
    );
});

for (const [firstLine, secondLine] of [
    ['s{foo}', '{bar}r'],
    ['tr[a-z]', '[A-Z]r'],
    ['s{foo}', '/bar/ee'],
    ['s{foo}', '  [bar]r'],
]) {
    test(`newline between paired halves: ${firstLine}`, () => {
        const first = grammar.tokenizeLine(firstLine);
        const line = `${secondLine}; my $after = 1;`;
        const second = grammar.tokenizeLine(line, first.ruleStack);
        for (let offset = 0; offset < secondLine.length; offset++) {
            assert.ok(scopesAt(second.tokens, offset).includes(scope));
        }
        assert.ok(!scopesAt(second.tokens, secondLine.length).includes(scope));
        assert.ok(
            scopesAt(second.tokens, line.indexOf('$after')).includes('variable.other.scalar.perl'),
        );
    });
}

test('comments between paired halves preserve replacement and following code', () => {
    let stack;
    for (const line of [
        's{foo}',
        ' # comment!',
        '',
        ' # second comment.',
        ' {bar}r; my $after = 1;',
    ]) {
        const result = grammar.tokenizeLine(line, stack);
        stack = result.ruleStack;
        if (line.includes('bar')) {
            assert.ok(scopesAt(result.tokens, line.indexOf('bar')).includes(scope));
            assert.ok(scopesAt(result.tokens, line.indexOf('r;')).includes(scope));
            assert.ok(
                scopesAt(result.tokens, line.indexOf('$after')).includes(
                    'variable.other.scalar.perl',
                ),
            );
        }
    }
});
for (const expression of ['s{foo}{bar}r', 's{foo}/bar/ee']) {
    test(`a completed paired replacement does not capture next-line delimiters: ${expression}`, () => {
        const first = grammar.tokenizeLine(expression);
        for (const line of [' / 2;', ' } my $after = 1;']) {
            const next = grammar.tokenizeLine(line, first.ruleStack);
            assert.ok(!scopesAt(next.tokens, 1).includes(scope));
        }
    });
}

for (const [line, subStart, subEnd, closeAt, marker] of [
    ['if ($x =~ s{a}{b}) { my $after = 1; }', 10, 17, 17, '$after'],
    ['if ($x =~ s{a}{b}){ my $after = 1; }', 10, 17, 17, '$after'],
    ['if ($x =~ s{^\\s+}{}) { my $after = 1; }', 10, 19, 19, '$after'],
    ['if ($x =~ tr[a-z][A-Z]) { my $after = 1; }', 10, 22, 22, '$after'],
    ['if ($x =~ s<a>{b}) { my $after = 1; }', 10, 17, 17, '$after'],
    ['my @out = map { s{a}{b} } @in;', 16, 23, 24, '@out'],
    ['$x =~ s{a}{b}; my $after = 1;', 6, 13, 13, '$after'],
]) {
    test(`an unflagged paired substitution does not swallow a following closer or block: ${line}`, () => {
        const result = grammar.tokenizeLine(line);
        for (let offset = subStart; offset < subEnd; offset++) {
            assert.ok(scopesAt(result.tokens, offset).includes(scope), `substitution at ${offset}: ${line[offset]}`);
        }
        assert.ok(
            !scopesAt(result.tokens, closeAt).includes(scope),
            `closer ${JSON.stringify(line[closeAt])} at ${closeAt} is code`,
        );
        const markerAt = line.indexOf(marker);
        const markerScopes = scopesAt(result.tokens, markerAt);
        if (marker.startsWith('$')) {
            assert.ok(markerScopes.includes('variable.other.scalar.perl'), `${marker} stays a variable`);
        } else {
            assert.ok(!markerScopes.includes(scope), `${marker} stays code`);
        }
    });
}
test('a flagged paired substitution still binds whitespace-separated brace replacement', () => {
    const line = '$x =~ s{foo} {bar}r; my $after = 1;';
    const result = grammar.tokenizeLine(line);
    assert.ok(scopesAt(result.tokens, line.indexOf('{bar}')).includes(scope));
    assert.ok(!scopesAt(result.tokens, line.indexOf('; my')).includes(scope));
});
