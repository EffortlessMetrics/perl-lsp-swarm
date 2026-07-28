import * as fs from 'fs';
import * as path from 'path';

/**
 * The README's "Commands" section is the Marketplace-facing list of what the
 * extension can do. It drifted to 15 of 35 contributed commands, which left
 * "Perl: Run Health Check" -- the command a user needs when a first run goes
 * wrong -- undocumented, and it carried a title ("Perl: Restart Language
 * Server") that did not match the palette entry.
 *
 * These tests pin the README to `contributes.commands` so the two cannot drift
 * apart again silently.
 */
describe('README command coverage', () => {
    const extensionRoot = path.resolve(__dirname, '..', '..');
    const packageJson = JSON.parse(
        fs.readFileSync(path.join(extensionRoot, 'package.json'), 'utf8'),
    ) as {
        contributes: { commands: Array<{ command: string; title: string; category?: string }> };
    };
    const readme = fs.readFileSync(path.join(extensionRoot, 'README.md'), 'utf8');

    const paletteLabel = (cmd: { title: string; category?: string }): string =>
        cmd.category ? `${cmd.category}: ${cmd.title}` : cmd.title;

    it('documents every contributed command', () => {
        const missing = packageJson.contributes.commands
            .map(paletteLabel)
            .filter((label) => !readme.includes(label));

        expect(missing).toEqual([]);
    });

    it('uses the exact palette label for each documented command', () => {
        // A label the user cannot find by typing it into the palette is worse
        // than no label, so compare against the rendered `category: title`
        // string rather than a substring of it.
        for (const cmd of packageJson.contributes.commands) {
            const label = paletteLabel(cmd);
            expect(readme).toContain(`**${label}**`);
        }
    });

    it('does not advertise commands the extension does not contribute', () => {
        const contributed = new Set(packageJson.contributes.commands.map(paletteLabel));

        // Bold entries in the Commands section only -- the rest of the README
        // legitimately bolds prose.
        const commandsSection = readme.slice(
            readme.indexOf('## Commands'),
            readme.indexOf('## Compatibility'),
        );
        expect(commandsSection.length).toBeGreaterThan(0);

        const advertised = [...commandsSection.matchAll(/^\| \*\*(.+?)\*\* \|/gm)]
            .map((m) => m[1])
            .filter((label): label is string => label !== undefined);
        expect(advertised.length).toBeGreaterThan(0);

        const unknown = advertised.filter((label) => !contributed.has(label));
        expect(unknown).toEqual([]);
    });
});
