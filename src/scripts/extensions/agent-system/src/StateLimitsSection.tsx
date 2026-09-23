/**
 * How much one value may cost, and what it is counted in.
 *
 * The number was a constant before this: 512 characters for every scene, which
 * is roughly 105 tokens of English and 320 to 640 of Chinese. So the unit is
 * part of the sentence, not a detail — and a scene that counts in tokens has to
 * say which vocabulary counts them, because a token is not a token across
 * models. The default follows the chat's model, which needs no configuration.
 */

import {
    stateLimitsOf,
    TOKENIZER_FILE_PREFIX,
    type StateDeclaration,
} from './state-config-model';
import type { StateConfigController } from './state-config-controller';
import type { Tr } from './AgentSystemPanelContract';

/**
 * The vocabularies the app ships, mirroring `tt-adapter-tokenization`'s table.
 *
 * A name the backend does not know is refused with a message that says so, so
 * this list only has to be the convenient half of the answer: the field accepts
 * any name, including a file the user supplies.
 */
const SHIPPED_FAMILIES = [
    { name: 'qwen3.8', label: 'Qwen3.8' },
    { name: 'deepseek-v4.1', label: 'DeepSeek V4.1' },
    { name: 'glm', label: 'GLM' },
    { name: 'gemma4', label: 'Gemma 4 / Gemini' },
    { name: 'qwen3-embedding', label: 'Qwen3 Embedding' },
] as const;

/** What the vocabulary select shows when the scene follows the chat's model. */
const FOLLOW_MODEL = '';

export function StateLimitsSection({
    draft,
    controller,
    tr,
}: {
    draft: StateDeclaration;
    controller: StateConfigController;
    tr: Tr;
}) {
    const limits = stateLimitsOf(draft);
    const countsTokens = limits.unit === 'tokens';
    const namedFile = limits.tokenizer.startsWith(TOKENIZER_FILE_PREFIX);

    // One numeric field, because three of them differ only in what they count.
    const ceiling = (
        key: 'value' | 'valuesPerField' | 'fieldsPerUpdate',
        label: string,
    ) => (
        <label className="ttas-field" key={key}>
            <span>{label}</span>
            <input
                className="text_pole"
                type="number"
                min={0}
                step={1}
                value={limits[key]}
                onChange={(event) => {
                    // An emptied box means "no ceiling", not "a NaN ceiling":
                    // the backend reads 0 that way.
                    const parsed = Number.parseInt(event.target.value, 10);
                    controller.updateLimits({ [key]: Number.isFinite(parsed) ? Math.max(0, parsed) : 0 });
                }}
            />
        </label>
    );

    return (
        <section className="ttas-state-section">
            <h4>{tr('stateLimitsTitle')}</h4>
            <p className="ttas-field-hint">{tr('stateLimitsLead')}</p>

            <div className="ttas-state-row-inputs">
                {ceiling('value', tr('stateLimitsValue'))}
                {ceiling('valuesPerField', tr('stateLimitsValuesPerField'))}
                {ceiling('fieldsPerUpdate', tr('stateLimitsFieldsPerUpdate'))}
                <label className="ttas-field">
                    <span>{tr('stateLimitsUnit')}</span>
                    <select
                        className="text_pole"
                        value={limits.unit}
                        onChange={(event) =>
                            controller.updateLimits({
                                unit: event.target.value === 'tokens' ? 'tokens' : 'chars',
                            })
                        }
                    >
                        <option value="chars">{tr('stateLimitsUnitChars')}</option>
                        <option value="tokens">{tr('stateLimitsUnitTokens')}</option>
                    </select>
                </label>
            </div>

            {countsTokens && (
                <div className="ttas-state-row-inputs">
                    <label className="ttas-field">
                        <span>{tr('stateLimitsTokenizer')}</span>
                        <select
                            className="text_pole"
                            value={namedFile ? TOKENIZER_FILE_PREFIX : limits.tokenizer}
                            onChange={(event) =>
                                controller.updateLimits({
                                    tokenizer:
                                        event.target.value === TOKENIZER_FILE_PREFIX
                                            ? TOKENIZER_FILE_PREFIX
                                            : event.target.value,
                                })
                            }
                        >
                            <option value={FOLLOW_MODEL}>{tr('stateLimitsTokenizerModel')}</option>
                            {SHIPPED_FAMILIES.map((family) => (
                                <option key={family.name} value={family.name}>
                                    {family.label}
                                </option>
                            ))}
                            <option value={TOKENIZER_FILE_PREFIX}>{tr('stateLimitsTokenizerFile')}</option>
                        </select>
                    </label>
                    {namedFile && (
                        <>
                            <label className="ttas-field">
                                <span>{tr('stateLimitsTokenizerPath')}</span>
                                <input
                                    className="text_pole"
                                    value={limits.tokenizer.slice(TOKENIZER_FILE_PREFIX.length)}
                                    placeholder="/path/to/tokenizer.json"
                                    onChange={(event) =>
                                        controller.updateLimits({
                                            tokenizer: `${TOKENIZER_FILE_PREFIX}${event.target.value}`,
                                        })
                                    }
                                />
                            </label>
                            <button
                                type="button"
                                className="menu_button"
                                onClick={() => void controller.chooseVocabularyFile()}
                            >
                                <i className="fa-solid fa-folder-open"></i>
                                <span>{tr('stateLimitsTokenizerBrowse')}</span>
                            </button>
                        </>
                    )}
                </div>
            )}

            <p className="ttas-field-hint">
                {countsTokens ? tr('stateLimitsTokensHint') : tr('stateLimitsCharsHint')}
            </p>
        </section>
    );
}
