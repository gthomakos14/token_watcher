// Unit tests for extension/format.js. Run with: node --test extension-tests/
import { test } from 'node:test';
import assert from 'node:assert/strict';
import * as F from '../extension/format.js';

const model = (name, remaining, extra = {}) => ({ name, remaining_percentage: remaining, ...extra });
const antigravity = models => ({ provider: 'antigravity', models });
const claude = models => ({ provider: 'claude', models });

const AG_MODELS = [model('Gemini (Flash & Pro)', 72), model('Claude & GPT (Sonnet, Opus, GPT-OSS)', 100)];
const CLAUDE_MODELS = [model('Session', 93), model('Week (all models)', 40)];

test('reportFromOutput accepts only successful, error-free JSON', () => {
    assert.deepEqual(F.reportFromOutput(true, ' {"models":[]}\n'), { models: [] });
    assert.equal(F.reportFromOutput(false, '{"models":[]}'), null);
    assert.equal(F.reportFromOutput(true, ''), null);
    assert.equal(F.reportFromOutput(true, null), null);
    assert.equal(F.reportFromOutput(true, '{"error":"agy failed"}'), null);
    assert.throws(() => F.reportFromOutput(true, 'not json'), SyntaxError);
});

test('provider helpers', () => {
    assert.equal(F.isClaude(claude([])), true);
    assert.equal(F.isClaude(antigravity([])), false);
    assert.equal(F.isClaude({}), false, 'reports from before Claude support are Antigravity');
    assert.equal(F.providerIconName('claude'), 'claude.svg');
    assert.equal(F.providerIconName('antigravity'), 'antigravity.svg');
    assert.equal(F.providerIconName(undefined), 'antigravity.svg');
});

test('switching providers', () => {
    assert.equal(F.otherProvider('claude'), 'antigravity');
    assert.equal(F.otherProvider('antigravity'), 'claude');
    assert.equal(F.otherProvider(undefined), 'claude', 'reports from before Claude support are Antigravity');
    assert.equal(F.switchLabel('claude'), 'Antigravity');
    assert.equal(F.switchLabel('antigravity'), 'Claude');
});

test('user and plan names fall back per provider', () => {
    assert.equal(F.userName({ provider: 'claude', user: { name: 'ada' } }), 'ada');
    assert.equal(F.userName({ provider: 'claude', user: { name: '' } }), 'Claude User');
    assert.equal(F.userName({ provider: 'antigravity', user: {} }), 'Antigravity User');
    assert.equal(F.userName({}), 'Antigravity User', 'reports from before Claude support are Antigravity');

    assert.equal(F.planName({ provider: 'claude', plan: { name: 'Claude Pro' } }), 'Claude Pro');
    assert.equal(F.planName({ provider: 'claude', plan: { name: '' } }), 'Claude');
    assert.equal(F.planName({ provider: 'antigravity', plan: {} }), 'Antigravity Quota');
});

test('Claude primary model is the tightest limit, ignoring the keyword', () => {
    assert.equal(F.pickPrimaryModel(claude(CLAUDE_MODELS), 'session').name, 'Week (all models)');
});

test('Antigravity primary model matches the keyword case-insensitively', () => {
    assert.equal(F.pickPrimaryModel(antigravity(AG_MODELS), 'SONNET').name, 'Claude & GPT (Sonnet, Opus, GPT-OSS)');
    assert.equal(F.pickPrimaryModel(antigravity(AG_MODELS), 'flash').name, 'Gemini (Flash & Pro)');
});

test('primary model falls back to the first model, or null', () => {
    assert.equal(F.pickPrimaryModel(antigravity(AG_MODELS), 'nothing matches').name, 'Gemini (Flash & Pro)');
    assert.equal(F.pickPrimaryModel(antigravity([]), 'flash'), null);
    assert.equal(F.pickPrimaryModel(claude([]), ''), null);
    assert.equal(F.pickPrimaryModel({}, 'flash'), null);
});

test('statusClass thresholds', () => {
    const at = pct => F.statusClass(model('m', pct), true, 20);
    assert.equal(at(20), 'antigravity-status-critical');
    assert.equal(at(5), 'antigravity-status-critical');
    assert.equal(at(21), 'antigravity-status-warn');
    assert.equal(at(40), 'antigravity-status-warn');
    assert.equal(at(41), 'antigravity-status-good');
    assert.equal(F.statusClass(model('m', 1), false, 20), null, 'warnings off');
    assert.equal(F.statusClass(null, true, 20), null, 'no model');
});

test('percentage format', () => {
    assert.equal(F.panelText(antigravity(AG_MODELS), 'percentage', model('m', 72.6)), '73%');
    assert.equal(F.panelText(antigravity([]), 'percentage', null), 'N/A');
});

test('model-and-percentage format shortens names', () => {
    const text = name => F.panelText(antigravity([]), 'model-and-percentage', model(name, 50));
    assert.equal(text('Gemini 3 Flash'), 'Flash: 50%');
    assert.equal(text('Gemini 3 Pro'), 'Pro: 50%');
    assert.equal(text('Claude Sonnet'), 'Claude: 50%');
    assert.equal(text('Week (all models)'), 'Week: 50%');
    assert.equal(F.panelText(antigravity([]), 'model-and-percentage', null), 'N/A');
});

test('dual format for Antigravity shows both families', () => {
    assert.equal(F.panelText(antigravity(AG_MODELS), 'dual', AG_MODELS[0]), '⚡ 72% | 🧠 100%');
});

test('dual format for Claude shows the first two limits unlabeled', () => {
    assert.equal(F.panelText(claude(CLAUDE_MODELS), 'dual', CLAUDE_MODELS[1]), '93% | 40%');
});

test('dual format falls back to the primary model, then leaves the label alone', () => {
    const single = [model('Session', 93)];
    assert.equal(F.panelText(claude(single), 'dual', single[0]), '93%');
    assert.equal(F.panelText(antigravity([model('Gemini', 10)]), 'dual', model('Gemini', 10)), '10%');
    assert.equal(F.panelText(antigravity([]), 'dual', null), null);
});

test('icon-only and unknown formats produce no text', () => {
    assert.equal(F.panelText(antigravity(AG_MODELS), 'icon-only', AG_MODELS[0]), null);
    assert.equal(F.panelText(antigravity(AG_MODELS), 'something-new', AG_MODELS[0]), null);
});

test('bar fill colors', () => {
    assert.equal(F.barFillClass(51), 'antigravity-bar-fill-good');
    assert.equal(F.barFillClass(50), 'antigravity-bar-fill-warn');
    assert.equal(F.barFillClass(21), 'antigravity-bar-fill-warn');
    assert.equal(F.barFillClass(20), 'antigravity-bar-fill-critical');
    assert.equal(F.barFillClass(0), 'antigravity-bar-fill-critical');
});

test('bar fill width is proportional with a 3px minimum', () => {
    assert.equal(F.barFillWidth(50, 200), 100);
    assert.equal(F.barFillWidth(100, 200), 200);
    assert.equal(F.barFillWidth(72.4, 100), 72);
    assert.equal(F.barFillWidth(0, 200), 3);
    assert.equal(F.barFillWidth(1, 100), 3);
});

test('reset text', () => {
    assert.equal(
        F.resetText(model('m', 1, { time_until_reset: '2h 5m', reset_time: '2026-11-03 13:00:00' })),
        'Resets in 2h 5m (2026-11-03 13:00:00)');
    assert.equal(F.resetText(model('m', 1, { time_until_reset: '2h 5m' })), 'Resets in 2h 5m ()');
    assert.equal(F.resetText(model('m', 1, { reset_time: 'soon' })), 'Resets: soon');
    assert.equal(F.resetText(model('m', 1)), null);
});

test('real backend output renders end to end', () => {
    // Shape of `token-watcher --json` for Claude.
    const stdout = JSON.stringify({
        provider: 'claude',
        user: { name: 'glenn', email: 'a@b.c', profile_url: null },
        plan: { id: 'pro', name: 'Claude Pro', description: null },
        selected_model_id: null,
        selected_model_name: null,
        models: [
            { name: 'Session', id: 1, remaining_percentage: 87, used_percentage: 13, reset_timestamp: 1, reset_time: 'x', time_until_reset: '3h 25m', badge: null, is_selected: false },
            { name: 'Week (all models)', id: 2, remaining_percentage: 94, used_percentage: 6, reset_timestamp: 2, reset_time: 'y', time_until_reset: '1d 13h', badge: null, is_selected: false },
        ],
        fetched_at: 0,
    });
    const data = F.reportFromOutput(true, stdout);
    const primary = F.pickPrimaryModel(data, 'flash');
    assert.equal(F.panelText(data, 'percentage', primary), '87%');
    assert.equal(F.statusClass(primary, true, 20), 'antigravity-status-good');
});
