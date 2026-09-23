/**
 * Tool description overrides in their two shapes.
 *
 * Stored, an override holds only the text that differs from the tool's own;
 * normalized, it holds only non-empty text, so a cleared field is stored as
 * absent rather than as an empty instruction. A malformed one stops the save:
 * it is a form the panel cannot render, not something the model can explain.
 */

type ToolDescriptions = NonNullable<TauriTavernAgentProfileDefinition['tools']['toolDescriptions']>;

function isPlainObject(value: unknown): value is Record<string, unknown> {
    return value !== null && typeof value === 'object' && !Array.isArray(value);
}

export function normalizeToolDescriptions(value: unknown): ToolDescriptions {
    if (value == null) {
        return {};
    }
    if (!isPlainObject(value)) {
        throw new Error('tools.toolDescriptions must be an object');
    }

    const normalized: ToolDescriptions = {};
    for (const [toolName, override] of Object.entries(value)) {
        if (!isPlainObject(override)) {
            throw new Error(`tools.toolDescriptions.${toolName} must be an object`);
        }

        const description = override.description;
        if (description !== undefined && typeof description !== 'string') {
            throw new Error(`tools.toolDescriptions.${toolName}.description must be a string`);
        }
        const properties: Record<string, string> = {};
        if (override.properties != null) {
            if (!isPlainObject(override.properties)) {
                throw new Error(`tools.toolDescriptions.${toolName}.properties must be an object`);
            }
            for (const [property, propertyDescription] of Object.entries(override.properties)) {
                if (typeof propertyDescription !== 'string') {
                    throw new Error(`tools.toolDescriptions.${toolName}.properties.${property} must be a string`);
                }
                if (propertyDescription.trim()) {
                    properties[property] = propertyDescription;
                }
            }
        }

        const normalizedOverride: TauriTavernToolDescriptionOverride = {};
        if (typeof description === 'string' && description.trim()) {
            normalizedOverride.description = description;
        }
        if (Object.keys(properties).length > 0) {
            normalizedOverride.properties = properties;
        }
        if (normalizedOverride.description || normalizedOverride.properties) {
            normalized[toolName] = normalizedOverride;
        }
    }

    return normalized;
}
