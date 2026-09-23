import { describe, expect, test } from '@rstest/core';

import { MAX_THEME_CSS_CHARS, compileThemeCss } from './state-theme-css';

describe('the theme stylesheet', () => {
    test('every selector is scoped to the panel root, inside at-rules as well', () => {
        const { css, issues } = compileThemeCss(
            '.tt-state-field { color: red }\n'
            + '@media (max-width: 600px) { .tt-state-rail { display: none } }\n'
            + ':root { --accent: gold }\n'
            + '@keyframes pulse { from { opacity: 0 } to { opacity: 1 } }',
        );

        expect(issues).toEqual([]);
        expect(css).toContain('.tt-state-root .tt-state-field');
        expect(css).toContain('.tt-state-root .tt-state-rail');
        // `:root` means the panel's own root here, not the document.
        expect(css).toContain('.tt-state-root {');
        expect(css).toContain('--accent: gold');
        // A keyframe name is a name, not a selector: there is nothing to scope.
        expect(css).toContain('@keyframes pulse');
    });

    test('a selector that already names the root is left as it was written', () => {
        const { css } = compileThemeCss('.tt-state-root .tt-state-field { color: red }');

        expect(css).toContain('.tt-state-root .tt-state-field');
        expect(css).not.toContain('.tt-state-root .tt-state-root');
    });

    test('compiling what was already compiled changes nothing', () => {
        const once = compileThemeCss('.tt-state-field { color: red }').css;

        expect(compileThemeCss(once).issues).toEqual([]);
        expect(compileThemeCss(once).css).toBe(once);
    });

    test('a stylesheet that cannot be read is refused with its position', () => {
        // One truncation can be several complaints at the same place: the parser
        // reports every place it could not read, and a broken rule stops it more
        // than once.
        const { css, issues } = compileThemeCss('.tt-state-field { color: red');

        expect(css).toBe('');
        expect(issues.length).toBeGreaterThan(0);
        for (const issue of issues) {
            expect(issue.path).toBe('theme.css');
            expect(issue.message).toContain('line 1');
        }
        expect(issues.map((issue) => issue.message).join(' ')).toContain("missing '}'");
    });

    test('a rule that would have the panel fetch something is refused by name', () => {
        const { css, issues } = compileThemeCss('@import url(/backgrounds/theme.css);');

        expect(css).toBe('');
        expect(issues[0]?.message).toContain('@import');
    });

    test('a `<` is refused before it can end the style element', () => {
        const { css, issues } = compileThemeCss('.tt-state-root .a { content: "</style>" }');

        expect(css).toBe('');
        expect(issues[0]?.message).toContain('`<`');
    });

    test('a sheet longer than the bound is refused', () => {
        const { css, issues } = compileThemeCss(`.a{color:red}`.repeat(MAX_THEME_CSS_CHARS));

        expect(css).toBe('');
        expect(issues[0]?.message).toContain('longer than');
    });

    test('nothing written is not a problem', () => {
        expect(compileThemeCss('   ')).toEqual({ css: '', issues: [] });
    });
});
