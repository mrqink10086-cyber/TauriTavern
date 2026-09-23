/**
 * The panel theme stylesheet, as the editor stores it.
 *
 * The panel has three style levels: the built-in default style, then this sheet,
 * then (later) an HTML template. This module owns the second one, and both of
 * its jobs happen at **save** time rather than at render time:
 *
 * - **Validation.** The sheet is parsed once, so a typo is refused while the
 *   user is still looking at it — with a line and a column — instead of a chat
 *   whose panel silently lost half its styling.
 * - **Scoping.** Every selector is prefixed with the panel root, so a theme can
 *   only reach the panel. Without it, a `body { display: none }` in a panel
 *   theme would take the whole application down with it.
 *
 * The compiled text is what gets stored, which is what keeps rendering free of
 * parsing: the renderer assigns one string to one `<style>` element.
 *
 * What is *not* checked here is where a picture may come from. A `url()` source
 * is the domain's rule (`validate_theme_css` refuses anything but a host path),
 * exactly as it is for a panel picture: one rule, one place to read it.
 */

import { CssTypes, parse, stringify, type CssAtRuleAST } from '@adobe/css-tools';

/** The class a stored theme selector has to carry, and the panel's root. */
export const STATE_PANEL_ROOT_CLASS = 'tt-state-root';

/** Mirrors `MAX_THEME_CSS_CHARS` in `state_panel.rs`. */
export const MAX_THEME_CSS_CHARS = 16_384;

/**
 * The at-rules a theme may use.
 *
 * Everything else is refused rather than passed through: `@import` fetches a
 * document, `@font-face` an asset, and `@layer`/`@namespace`/`@page` change how
 * the sheet is resolved rather than what the panel shows. A theme expresses the
 * difference between itself and the default style; it is not a second document
 * that gets to describe the application.
 */
const THEME_AT_RULES: readonly string[] = Object.freeze([
    CssTypes.rule,
    CssTypes.comment,
    CssTypes.media,
    CssTypes.supports,
    CssTypes.keyframes,
]);

/** What a compiled theme is: the text to store, and what is wrong with it. */
export type CompiledThemeCss = {
    /** The sheet as it is stored, scoped. Empty when the source cannot be used. */
    css: string;
    issues: ThemeCssIssue[];
};

export type ThemeCssIssue = {
    path: string;
    message: string;
};

/**
 * `html`, `body` and `:root` mean the panel root here, not the document.
 *
 * A theme that defines its variables on `:root` means "the panel's variables";
 * scoping that to `.tt-state-root :root` would match nothing and read as a theme
 * that does not work.
 */
const ROOT_ALIAS = /^(?:html|body|:root)\b/u;

/** A selector that already names the root is left exactly as it was written. */
const ROOT_REFERENCE = /\.tt-state-root(?![\w-])/u;

/**
 * Compile the theme for storage.
 *
 * An unusable source compiles to an empty sheet: storing half of a broken theme
 * is worse than storing none of it, and the editor refuses to save while an
 * issue is listed.
 */
export function compileThemeCss(source: string): CompiledThemeCss {
    const text = String(source ?? '').trim();
    if (!text) {
        return { css: '', issues: [] };
    }

    const issues: ThemeCssIssue[] = [];
    if ([...text].length > MAX_THEME_CSS_CHARS) {
        issues.push({
            path: 'theme.css',
            message: `the stylesheet is longer than ${MAX_THEME_CSS_CHARS} characters`,
        });
        return { css: '', issues };
    }
    if (text.includes('<')) {
        // The text is stored inside a `<style>` element: that character is how
        // it would stop being one.
        issues.push({ path: 'theme.css', message: 'a stylesheet cannot contain `<`' });
    }

    let ast: ReturnType<typeof parse>;
    try {
        // `silent` reports what it could not read instead of throwing, and every
        // entry carries the line and column the user has to look at.
        ast = parse(text, { silent: true });
    } catch (error) {
        // `silent` turns syntax errors into entries; anything thrown here is the
        // parser declining the input outright, which is still not a stylesheet.
        issues.push({
            path: 'theme.css',
            message: error instanceof Error ? error.message : String(error),
        });
        return { css: '', issues };
    }

    for (const error of ast.stylesheet.parsingErrors ?? []) {
        issues.push({
            path: 'theme.css',
            message: `line ${error.line}, column ${error.column}: ${error.reason}`,
        });
    }

    const rules = scopeRules(ast.stylesheet.rules, issues);

    if (issues.length > 0) {
        return { css: '', issues };
    }
    return {
        css: stringify({ type: CssTypes.stylesheet, stylesheet: { rules } }),
        issues,
    };
}

/**
 * Prefix every selector with the panel root, and refuse what a theme may not use.
 *
 * Rules are walked rather than text-edited: a selector inside a `@media` block
 * has to be scoped exactly like a top-level one, and a rule the theme is not
 * allowed to use has to be refused by name rather than as a parse error.
 */
function scopeRules(rules: readonly CssAtRuleAST[], issues: ThemeCssIssue[]): CssAtRuleAST[] {
    return rules.map((rule) => {
        if (!THEME_AT_RULES.includes(rule.type)) {
            issues.push({
                path: 'theme.css',
                message: `\`@${rule.type}\` cannot be used in a panel theme`,
            });
            return rule;
        }
        if (rule.type === CssTypes.rule) {
            return { ...rule, selectors: rule.selectors.map(scopeSelector) };
        }
        if (rule.type === CssTypes.media || rule.type === CssTypes.supports) {
            return { ...rule, rules: scopeRules(rule.rules ?? [], issues) };
        }
        // Comments and `@keyframes`: keyframe selectors are not selectors and
        // its name is a name, so there is nothing here to scope.
        return rule;
    });
}

function scopeSelector(selector: string): string {
    const trimmed = selector.trim();
    if (!trimmed) {
        return trimmed;
    }
    if (ROOT_ALIAS.test(trimmed)) {
        return trimmed.replace(ROOT_ALIAS, `.${STATE_PANEL_ROOT_CLASS}`);
    }
    if (ROOT_REFERENCE.test(trimmed)) {
        return trimmed;
    }
    return `.${STATE_PANEL_ROOT_CLASS} ${trimmed}`;
}
