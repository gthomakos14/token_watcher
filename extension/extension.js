import St from 'gi://St';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
import Clutter from 'gi://Clutter';
import Pango from 'gi://Pango';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';
import { Extension, gettext as _ } from 'resource:///org/gnome/shell/extensions/extension.js';

import * as Format from './format.js';

const AntigravityTokenIndicator = GObject.registerClass(
class AntigravityTokenIndicator extends PanelMenu.Button {
    _init(extension) {
        super._init(0.0, 'Token Watcher');
        this._extension = extension;
        this._settings = extension.getSettings();
        this._timeoutId = null;
        this._debounceId = null;
        this._fileMonitor = null;
        this._monitorId = null;
        this._historyMonitor = null;
        this._historyMonitorId = null;
        this._claudeHistoryMonitor = null;
        this._claudeHistoryMonitorId = null;
        this._iconProvider = null;
        this._latestData = null;
        // Provider chosen with the switch button; null follows the most recently prompted one.
        // Cleared by new prompt activity so the display goes back to following it.
        this._providerOverride = null;
        // Incremented per fetch so results from before a provider switch are dropped.
        this._fetchSeq = 0;
        // Tracks { track, signalId } pairs so we can disconnect before destroy_all_children()
        this._trackSignals = [];

        this._buildPanelButton();
        this._buildMenuContainer();

        // Connect settings
        this._settingsChangedId = this._settings.connect('changed', () => {
            if (this._latestData) {
                this._renderPanel(this._latestData);
                this._renderMenu(this._latestData);
            }
            this._resetTimer();
        });

        // Setup file monitor on state.vscdb
        this._setupFileMonitor();

        // Initial fetch
        this._fetchStatus();

        // Setup timer
        this._resetTimer();
    }

    _getBinaryPath() {
        // Priority 1: bundled in extension bin/
        const bundled = GLib.build_filenamev([this._extension.path, 'bin', 'token-watcher']);
        if (GLib.file_test(bundled, GLib.FileTest.IS_EXECUTABLE)) {
            return bundled;
        }

        // Priority 2: release build in dev workspace
        const devRelease = GLib.build_filenamev([this._extension.path, '..', 'target', 'release', 'token-watcher']);
        if (GLib.file_test(devRelease, GLib.FileTest.IS_EXECUTABLE)) {
            return devRelease;
        }

        // Priority 3: debug build in dev workspace
        const devDebug = GLib.build_filenamev([this._extension.path, '..', 'target', 'debug', 'token-watcher']);
        if (GLib.file_test(devDebug, GLib.FileTest.IS_EXECUTABLE)) {
            return devDebug;
        }

        // Priority 4: PATH
        const inPath = GLib.find_program_in_path('token-watcher');
        if (inPath) {
            return inPath;
        }

        return bundled;
    }

    _buildPanelButton() {
        this._panelBox = new St.BoxLayout({
            style_class: 'antigravity-panel-box',
            y_align: Clutter.ActorAlign.CENTER,
        });

        // Icon
        const iconPath = GLib.build_filenamev([this._extension.path, 'icons', 'antigravity.svg']);
        const iconFile = Gio.File.new_for_path(iconPath);
        if (iconFile.query_exists(null)) {
            const gicon = new Gio.FileIcon({ file: iconFile });
            this._icon = new St.Icon({
                gicon: gicon,
                style_class: 'antigravity-panel-icon',
            });
        } else {
            this._icon = new St.Icon({
                icon_name: 'utilities-system-monitor-symbolic',
                style_class: 'antigravity-panel-icon',
            });
        }
        this._panelBox.add_child(this._icon);

        // Label
        this._label = new St.Label({
            text: '...',
            style_class: 'antigravity-panel-label',
            y_align: Clutter.ActorAlign.CENTER,
        });
        this._panelBox.add_child(this._label);

        this.add_child(this._panelBox);
    }

    _buildMenuContainer() {
        this.menu.box.add_style_class_name('antigravity-menu-container');

        // Header (User, Tier)
        this._headerSection = new PopupMenu.PopupBaseMenuItem({
            reactive: false,
            can_focus: false,
        });
        this._headerBox = new St.BoxLayout({
            vertical: true,
            style_class: 'antigravity-header-box',
            x_expand: true,
        });
        this._userNameLabel = new St.Label({
            text: _('Loading…'),
            style_class: 'antigravity-user-name',
        });
        this._userEmailLabel = new St.Label({
            text: '',
            style_class: 'antigravity-user-email',
        });
        this._planBadgeLabel = new St.Label({
            text: '',
            style_class: 'antigravity-plan-badge',
        });
        this._activeModelLabel = new St.Label({
            text: '',
            style_class: 'antigravity-active-model-badge',
            visible: false,
        });

        this._headerBox.add_child(this._userNameLabel);
        this._headerBox.add_child(this._userEmailLabel);
        this._headerBox.add_child(this._planBadgeLabel);
        this._headerBox.add_child(this._activeModelLabel);
        this._headerSection.add_child(this._headerBox);
        this.menu.addMenuItem(this._headerSection);

        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());

        // Models section container
        this._modelsSection = new PopupMenu.PopupBaseMenuItem({
            reactive: false,
            can_focus: false,
        });
        this._modelsBox = new St.BoxLayout({
            vertical: true,
            style_class: 'antigravity-models-box',
            x_expand: true,
        });
        this._modelsSection.add_child(this._modelsBox);
        this.menu.addMenuItem(this._modelsSection);

        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());

        // Actions row
        const actionsItem = new PopupMenu.PopupBaseMenuItem({
            reactive: true,
            activate: false,
            hover: false,
            can_focus: false,
        });
        const actionsBox = new St.BoxLayout({
            style_class: 'antigravity-actions-box',
            x_expand: true,
        });

        // Refresh button
        const refreshBtn = new St.Button({
            label: _('Refresh'),
            style_class: 'button antigravity-action-btn',
            x_expand: true,
            reactive: true,
            can_focus: true,
            track_hover: true,
        });
        refreshBtn.connect('clicked', async () => {
            refreshBtn.reactive = false;
            refreshBtn.label = _('Refreshing…');
            try {
                await this._fetchStatus(true);
            } catch (e) {
                console.error('[Antigravity Token Watcher] Refresh failed:', e);
            } finally {
                refreshBtn.label = _('Refresh');
                refreshBtn.reactive = true;
            }
        });
        actionsBox.add_child(refreshBtn);

        // Switch between Antigravity and Claude
        const switchBtn = this._switchBtn = new St.Button({
            label: _('Claude'),
            style_class: 'button antigravity-action-btn',
            x_expand: true,
            reactive: true,
            can_focus: true,
            track_hover: true,
        });
        switchBtn.connect('clicked', () => {
            const current = this._providerOverride ?? this._latestData?.provider;
            this._providerOverride = Format.otherProvider(current);
            switchBtn.label = _(Format.switchLabel(this._providerOverride));
            this._fetchStatus(false);
        });
        actionsBox.add_child(switchBtn);

        // Open Antigravity IDE button
        const openIdeBtn = this._openIdeBtn = new St.Button({
            label: _('Open IDE'),
            style_class: 'button antigravity-action-btn',
            x_expand: true,
            reactive: true,
            can_focus: true,
            track_hover: true,
        });
        openIdeBtn.connect('clicked', () => {
            this.menu.close();
            try {
                const app = Gio.AppInfo.create_from_commandline('antigravity', 'Antigravity', Gio.AppInfoCreateFlags.NONE);
                app.launch([], null);
            } catch (e) {
                console.error('[Antigravity Token Watcher] Failed to launch IDE:', e);
            }
        });
        actionsBox.add_child(openIdeBtn);

        // Settings button
        const prefsBtn = new St.Button({
            label: _('Settings'),
            style_class: 'button antigravity-action-btn',
            x_expand: true,
            reactive: true,
            can_focus: true,
            track_hover: true,
        });
        prefsBtn.connect('clicked', () => {
            this.menu.close();
            this._extension.openPreferences();
        });
        actionsBox.add_child(prefsBtn);

        actionsItem.add_child(actionsBox);
        this.menu.addMenuItem(actionsItem);
    }

    _setupFileMonitor() {
        try {
            const home = GLib.get_home_dir();

            // 1. CLI activity monitor (~/.gemini/antigravity-cli/history.jsonl)
            const historyPath = GLib.build_filenamev([home, '.gemini', 'antigravity-cli', 'history.jsonl']);
            const historyFile = Gio.File.new_for_path(historyPath);

            if (historyFile.query_exists(null)) {
                this._historyMonitor = historyFile.monitor_file(Gio.FileMonitorFlags.NONE, null);
                this._historyMonitorId = this._historyMonitor.connect('changed', (monitor, file, otherFile, eventType) => {
                    if (eventType === Gio.FileMonitorEvent.CHANGES_DONE_HINT ||
                        eventType === Gio.FileMonitorEvent.CHANGED) {
                        this._providerOverride = null;
                        this._debouncedRefresh(true);
                    }
                });
            }

            // Claude prompt monitor (~/.claude/history.jsonl). This only needs to switch the
            // display, so it uses the cache rather than polling /usage on every prompt.
            const claudeHistoryPath = GLib.build_filenamev([home, '.claude', 'history.jsonl']);
            const claudeHistoryFile = Gio.File.new_for_path(claudeHistoryPath);

            this._claudeHistoryMonitor = claudeHistoryFile.monitor_file(Gio.FileMonitorFlags.NONE, null);
            this._claudeHistoryMonitorId = this._claudeHistoryMonitor.connect('changed', (monitor, file, otherFile, eventType) => {
                if (eventType === Gio.FileMonitorEvent.CHANGES_DONE_HINT ||
                    eventType === Gio.FileMonitorEvent.CHANGED) {
                    this._providerOverride = null;
                    this._debouncedRefresh(false);
                }
            });

            // 2. IDE database monitor (~/.config/Antigravity/.../state.vscdb)
            const dbPath = GLib.build_filenamev([home, '.config', 'Antigravity', 'User', 'globalStorage', 'state.vscdb']);
            const dbFile = Gio.File.new_for_path(dbPath);

            if (dbFile.query_exists(null)) {
                this._fileMonitor = dbFile.monitor_file(Gio.FileMonitorFlags.NONE, null);
                this._monitorId = this._fileMonitor.connect('changed', (monitor, file, otherFile, eventType) => {
                    if (eventType === Gio.FileMonitorEvent.CHANGES_DONE_HINT ||
                        eventType === Gio.FileMonitorEvent.CHANGED) {
                        this._debouncedRefresh(false);
                    }
                });
            }
        } catch (e) {
            console.error('[Antigravity Token Watcher] Error setting up file monitor:', e);
        }
    }

    _debouncedRefresh(force = false) {
        if (this._debounceId) {
            GLib.source_remove(this._debounceId);
        }
        this._debounceId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 1000, () => {
            this._debounceId = null;
            this._fetchStatus(force);
            return GLib.SOURCE_REMOVE;
        });
    }

    _resetTimer() {
        if (this._timeoutId) {
            GLib.source_remove(this._timeoutId);
            this._timeoutId = null;
        }

        const interval = Math.max(5, this._settings.get_int('refresh-interval'));
        this._timeoutId = GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, interval, () => {
            this._fetchStatus(false);
            return GLib.SOURCE_CONTINUE;
        });
    }

    _fetchStatus(forceRefresh = false) {
        return new Promise(resolve => {
            const bin = this._getBinaryPath();
            const seq = ++this._fetchSeq;
            try {
                const argv = [bin, '--json'];
                if (forceRefresh) {
                    argv.push('--refresh');
                }
                if (this._providerOverride) {
                    argv.push('--provider', this._providerOverride);
                }
                const proc = new Gio.Subprocess({
                    argv,
                    flags: Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_PIPE,
                });
                proc.init(null);

                proc.communicate_utf8_async(null, null, (obj, res) => {
                    try {
                        const [, stdout, stderr] = proc.communicate_utf8_finish(res);
                        const data = Format.reportFromOutput(proc.get_successful(), stdout);
                        if (data && seq !== this._fetchSeq) {
                            // A newer fetch (e.g. after a provider switch) supersedes this one.
                            resolve(data);
                            return;
                        }
                        if (data) {
                            this._latestData = data;
                            this._renderPanel(data);
                            this._renderMenu(data);
                            resolve(data);
                            return;
                        }
                        if (stderr) {
                            console.error('[Antigravity Token Watcher] stderr:', stderr);
                        }
                    } catch (e) {
                        console.error('[Antigravity Token Watcher] Parse error:', e);
                    }
                    resolve(null);
                });
            } catch (e) {
                console.error('[Antigravity Token Watcher] Execution error:', e);
                resolve(null);
            }
        });
    }

    _updateProviderIcon(provider) {
        if (provider === this._iconProvider) {
            return;
        }
        this._iconProvider = provider;

        const iconName = Format.providerIconName(provider);
        const iconPath = GLib.build_filenamev([this._extension.path, 'icons', iconName]);
        const iconFile = Gio.File.new_for_path(iconPath);
        if (iconFile.query_exists(null)) {
            this._icon.gicon = new Gio.FileIcon({ file: iconFile });
        }
    }

    _renderPanel(data) {
        this._updateProviderIcon(data.provider);

        const format = this._settings.get_string('display-format');
        const warnLow = this._settings.get_boolean('warn-low-quota');
        const threshold = this._settings.get_int('low-quota-threshold');

        if (format === 'icon-only') {
            this._label.visible = false;
            return;
        }
        this._label.visible = true;

        const primaryModel = Format.pickPrimaryModel(data, this._settings.get_string('primary-model'));

        // Color coding
        this._label.remove_style_class_name('antigravity-status-good');
        this._label.remove_style_class_name('antigravity-status-warn');
        this._label.remove_style_class_name('antigravity-status-critical');
        const status = Format.statusClass(primaryModel, warnLow, threshold);
        if (status) {
            this._label.add_style_class_name(status);
        }

        const text = Format.panelText(data, format, primaryModel);
        if (text !== null) {
            this._label.set_text(text);
        }
    }

    _renderMenu(data) {
        this._openIdeBtn.visible = !Format.isClaude(data);
        this._switchBtn.label = _(Format.switchLabel(data.provider));

        // User Header
        if (data.user) {
            this._userNameLabel.set_text(Format.userName(data));
            this._userEmailLabel.set_text(data.user.email || '');
            this._userEmailLabel.visible = !!data.user.email;
        }
        if (data.plan) {
            this._planBadgeLabel.set_text(Format.planName(data));
            this._planBadgeLabel.visible = true;
        } else {
            this._planBadgeLabel.visible = false;
        }
        if (data.selected_model_name) {
            this._activeModelLabel.set_text(`Active Model: ${data.selected_model_name}`);
            this._activeModelLabel.visible = true;
        } else {
            this._activeModelLabel.visible = false;
        }

        // Models list
        // Disconnect stale allocation signal handlers before wiping the old widgets.
        // Without this, destroyed tracks keep firing notify::allocation callbacks on
        // detached actors, spamming gnome-shell logs and causing the refresh to hang.
        for (const { track, signalId } of this._trackSignals)
            track.disconnect(signalId);
        this._trackSignals = [];

        this._modelsBox.destroy_all_children();
        const models = data.models || [];
        const showBadge = this._settings.get_boolean('show-badge');

        for (const m of models) {
            const card = new St.BoxLayout({
                vertical: true,
                style_class: 'antigravity-model-card' + (m.is_selected ? ' antigravity-model-card-active' : ''),
                x_expand: true,
            });

            // Top Row: Title, Badge, Percentage
            const topRow = new St.BoxLayout({
                style_class: 'antigravity-model-row',
                x_expand: true,
            });

            const titleLabel = new St.Label({
                text: m.name,
                style_class: 'antigravity-model-title',
                x_expand: true,
                y_align: Clutter.ActorAlign.CENTER,
            });
            topRow.add_child(titleLabel);

            if (showBadge && m.badge) {
                const badgeLabel = new St.Label({
                    text: m.badge,
                    style_class: 'antigravity-model-tag',
                    y_align: Clutter.ActorAlign.CENTER,
                });
                topRow.add_child(badgeLabel);
            }

            const pctLabel = new St.Label({
                text: `${m.remaining_percentage.toFixed(1)}%`,
                style_class: 'antigravity-model-pct',
                y_align: Clutter.ActorAlign.CENTER,
            });
            topRow.add_child(pctLabel);
            card.add_child(topRow);

            // Progress Bar Track
            const track = new St.Widget({
                style_class: 'antigravity-bar-track',
                x_expand: true,
            });

            const fill = new St.Widget({
                style_class: `antigravity-bar-fill ${Format.barFillClass(m.remaining_percentage)}`,
            });

            // Layout progress bar proportionally when allocated
            const updateFill = () => {
                const totalWidth = track.get_width();
                if (totalWidth > 0) {
                    fill.set_width(Format.barFillWidth(m.remaining_percentage, totalWidth));
                }
            };
            // Store signal ID so we can disconnect it before the next destroy_all_children() call
            const sigId = track.connect('notify::allocation', updateFill);
            this._trackSignals.push({ track, signalId: sigId });
            updateFill();

            track.add_child(fill);
            card.add_child(track);

            // Bottom Row: Reset info
            const resetText = Format.resetText(m);
            if (resetText) {
                const resetLabel = new St.Label({
                    text: resetText,
                    style_class: 'antigravity-model-reset',
                });
                card.add_child(resetLabel);
            }

            this._modelsBox.add_child(card);
        }
    }

    destroy() {
        if (this._timeoutId) {
            GLib.source_remove(this._timeoutId);
            this._timeoutId = null;
        }
        if (this._debounceId) {
            GLib.source_remove(this._debounceId);
            this._debounceId = null;
        }
        if (this._fileMonitor && this._monitorId) {
            this._fileMonitor.disconnect(this._monitorId);
            this._fileMonitor.cancel();
            this._fileMonitor = null;
        }
        if (this._historyMonitor && this._historyMonitorId) {
            this._historyMonitor.disconnect(this._historyMonitorId);
            this._historyMonitor.cancel();
            this._historyMonitor = null;
        }
        if (this._claudeHistoryMonitor && this._claudeHistoryMonitorId) {
            this._claudeHistoryMonitor.disconnect(this._claudeHistoryMonitorId);
            this._claudeHistoryMonitor.cancel();
            this._claudeHistoryMonitor = null;
        }
        if (this._settings && this._settingsChangedId) {
            this._settings.disconnect(this._settingsChangedId);
            this._settingsChangedId = null;
        }
        // Disconnect all remaining allocation signal handlers
        for (const { track, signalId } of this._trackSignals)
            track.disconnect(signalId);
        this._trackSignals = [];
        super.destroy();
    }
});

export default class AntigravityTokenWatcherExtension extends Extension {
    enable() {
        this._indicator = new AntigravityTokenIndicator(this);
        const panelBox = this.getSettings().get_string('panel-box');
        Main.panel.addToStatusArea(this.uuid, this._indicator, 1, panelBox);
    }

    disable() {
        if (this._indicator) {
            this._indicator.destroy();
            this._indicator = null;
        }
    }
}
