// Pure display logic for the panel and menu. This file must not import GNOME libraries,
// so it can be unit tested outside GNOME Shell (see extension-tests/).

/** Returns the parsed report from backend stdout, or null if the run failed or reported an error. */
export function reportFromOutput(successful, stdout) {
    if (!successful || !stdout)
        return null;
    const data = JSON.parse(stdout.trim());
    return data.error ? null : data;
}

export function isClaude(data) {
    return data.provider === 'claude';
}

/** Display name for the popup header, with a provider-specific fallback. */
export function userName(data) {
    return data.user?.name || (isClaude(data) ? 'Claude User' : 'Antigravity User');
}

/** Plan badge text, with a provider-specific fallback. */
export function planName(data) {
    return data.plan?.name || (isClaude(data) ? 'Claude' : 'Antigravity Quota');
}

export function providerIconName(provider) {
    return provider === 'claude' ? 'claude.svg' : 'antigravity.svg';
}

/**
 * The model whose percentage the top bar shows. Claude's limits all apply at once, so the
 * tightest one matters; for Antigravity it is the model matching the user's keyword.
 */
export function pickPrimaryModel(data, primaryKeyword) {
    const models = data.models || [];
    const keyword = (primaryKeyword || '').toLowerCase();
    const primary = isClaude(data)
        ? models.reduce((low, m) => (!low || m.remaining_percentage < low.remaining_percentage ? m : low), null)
        : models.find(m => m.name.toLowerCase().includes(keyword));
    return primary || models[0] || null;
}

/** Style class for the top bar label, or null when low-quota warnings are off. */
export function statusClass(model, warnLow, threshold) {
    if (!model || !warnLow)
        return null;
    if (model.remaining_percentage <= threshold)
        return 'antigravity-status-critical';
    if (model.remaining_percentage <= threshold + 20)
        return 'antigravity-status-warn';
    return 'antigravity-status-good';
}

function pct(model) {
    return `${model.remaining_percentage.toFixed(0)}%`;
}

/**
 * Top bar text for a display format. Returns null when the label should be hidden
 * ('icon-only') or left unchanged (no usable model in 'dual').
 */
export function panelText(data, format, primaryModel) {
    const models = data.models || [];

    if (format === 'icon-only')
        return null;

    if (format === 'percentage')
        return primaryModel ? pct(primaryModel) : 'N/A';

    if (format === 'model-and-percentage') {
        if (!primaryModel)
            return 'N/A';
        const name = primaryModel.name;
        const shortName = name.includes('Flash') ? 'Flash'
            : name.includes('Pro') ? 'Pro'
            : name.includes('Claude') ? 'Claude'
            : name.split(' ')[0];
        return `${shortName}: ${pct(primaryModel)}`;
    }

    if (format === 'dual') {
        if (isClaude(data) && models.length >= 2)
            return `${pct(models[0])} | ${pct(models[1])}`;
        const flash = models.find(m => m.name.includes('Flash') || m.name.includes('Gemini'));
        const claude = models.find(m => m.name.includes('Claude') || m.name.includes('Sonnet') || m.name.includes('GPT'));
        if (flash && claude)
            return `⚡ ${pct(flash)} | 🧠 ${pct(claude)}`;
        return primaryModel ? pct(primaryModel) : null;
    }

    return null;
}

export function barFillClass(remainingPercentage) {
    if (remainingPercentage > 50)
        return 'antigravity-bar-fill-good';
    if (remainingPercentage > 20)
        return 'antigravity-bar-fill-warn';
    return 'antigravity-bar-fill-critical';
}

/** Width of a bar fill in pixels; at least 3 so an empty bar stays visible. */
export function barFillWidth(remainingPercentage, trackWidth) {
    return Math.max(3, Math.round((remainingPercentage / 100.0) * trackWidth));
}

/** The reset line under a model card, or null when there is no reset information. */
export function resetText(model) {
    if (model.time_until_reset)
        return `Resets in ${model.time_until_reset} (${model.reset_time || ''})`;
    if (model.reset_time)
        return `Resets: ${model.reset_time}`;
    return null;
}
