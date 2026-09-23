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

const AntigravityTokenIndicator = GObject.registerClass(
class AntigravityTokenIndicator extends PanelMenu.Button {
    _init(extension) {
        super._init(0.0, 'Antigravity Token Watcher');
        this._extension = extension;
        this._settings = extension.getSettings();
        this._timeoutId = null;
        this._debounceId = null;
        this._fileMonitor = null;
        this._monitorId = null;
        this._latestData = null;

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
            text: 'Antigravity User',
            style_class: 'antigravity-user-name',
        });
        this._userEmailLabel = new St.Label({
            text: '',
            style_class: 'antigravity-user-email',
        });
        this._planBadgeLabel = new St.Label({
            text: 'Starter Quota',
            style_class: 'antigravity-plan-badge',
        });

        this._headerBox.add_child(this._userNameLabel);
        this._headerBox.add_child(this._userEmailLabel);
        this._headerBox.add_child(this._planBadgeLabel);
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
            reactive: false,
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
        });
        refreshBtn.connect('clicked', () => {
            this._fetchStatus();
        });
        actionsBox.add_child(refreshBtn);

        // Open Antigravity IDE button
        const openIdeBtn = new St.Button({
            label: _('Open IDE'),
            style_class: 'button antigravity-action-btn',
            x_expand: true,
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
            const dbPath = GLib.build_filenamev([home, '.config', 'Antigravity', 'User', 'globalStorage', 'state.vscdb']);
            const dbFile = Gio.File.new_for_path(dbPath);

            if (dbFile.query_exists(null)) {
                this._fileMonitor = dbFile.monitor_file(Gio.FileMonitorFlags.NONE, null);
                this._monitorId = this._fileMonitor.connect('changed', (monitor, file, otherFile, eventType) => {
                    if (eventType === Gio.FileMonitorEvent.CHANGES_DONE_HINT ||
                        eventType === Gio.FileMonitorEvent.CHANGED) {
                        this._debouncedRefresh();
                    }
                });
            }
        } catch (e) {
            console.error('[Antigravity Token Watcher] Error setting up file monitor:', e);
        }
    }

    _debouncedRefresh() {
        if (this._debounceId) {
            GLib.source_remove(this._debounceId);
        }
        this._debounceId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 500, () => {
            this._debounceId = null;
            this._fetchStatus();
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
            this._fetchStatus();
            return GLib.SOURCE_CONTINUE;
        });
    }

    async _fetchStatus() {
        const bin = this._getBinaryPath();
        try {
            const proc = new Gio.Subprocess({
                argv: [bin, '--json'],
                flags: Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_PIPE,
            });
            proc.init(null);

            proc.communicate_utf8_async(null, null, (obj, res) => {
                try {
                    const [, stdout, stderr] = proc.communicate_utf8_finish(res);
                    if (proc.get_successful() && stdout) {
                        const data = JSON.parse(stdout);
                        if (!data.error) {
                            this._latestData = data;
                            this._renderPanel(data);
                            this._renderMenu(data);
                            return;
                        }
                    }
                    if (stderr) {
                        console.error('[Antigravity Token Watcher] stderr:', stderr);
                    }
                } catch (e) {
                    console.error('[Antigravity Token Watcher] Parse error:', e);
                }
            });
        } catch (e) {
            console.error('[Antigravity Token Watcher] Execution error:', e);
        }
    }

    _renderPanel(data) {
        const format = this._settings.get_string('display-format');
        const primaryKeyword = this._settings.get_string('primary-model').toLowerCase();
        const warnLow = this._settings.get_boolean('warn-low-quota');
        const threshold = this._settings.get_int('low-quota-threshold');

        if (format === 'icon-only') {
            this._label.visible = false;
            return;
        }
        this._label.visible = true;

        const models = data.models || [];
        let primaryModel = models.find(m => m.name.toLowerCase().includes(primaryKeyword));
        if (!primaryModel && models.length > 0) {
            primaryModel = models[0];
        }

        // Color coding
        this._label.remove_style_class_name('antigravity-status-good');
        this._label.remove_style_class_name('antigravity-status-warn');
        this._label.remove_style_class_name('antigravity-status-critical');

        if (primaryModel && warnLow) {
            if (primaryModel.remaining_percentage <= threshold) {
                this._label.add_style_class_name('antigravity-status-critical');
            } else if (primaryModel.remaining_percentage <= threshold + 20) {
                this._label.add_style_class_name('antigravity-status-warn');
            } else {
                this._label.add_style_class_name('antigravity-status-good');
            }
        }

        if (format === 'percentage') {
            if (primaryModel) {
                this._label.set_text(`${primaryModel.remaining_percentage.toFixed(0)}%`);
            } else {
                this._label.set_text('N/A');
            }
        } else if (format === 'model-and-percentage') {
            if (primaryModel) {
                const shortName = primaryModel.name.includes('Flash') ? 'Flash' :
                                  primaryModel.name.includes('Pro') ? 'Pro' :
                                  primaryModel.name.includes('Claude') ? 'Claude' :
                                  primaryModel.name.split(' ')[0];
                this._label.set_text(`${shortName}: ${primaryModel.remaining_percentage.toFixed(0)}%`);
            } else {
                this._label.set_text('N/A');
            }
        } else if (format === 'dual') {
            const flash = models.find(m => m.name.includes('Flash'));
            const claude = models.find(m => m.name.includes('Claude') || m.name.includes('Sonnet'));
            if (flash && claude) {
                this._label.set_text(`⚡ ${flash.remaining_percentage.toFixed(0)}% | 🧠 ${claude.remaining_percentage.toFixed(0)}%`);
            } else if (primaryModel) {
                this._label.set_text(`${primaryModel.remaining_percentage.toFixed(0)}%`);
            }
        }
    }

    _renderMenu(data) {
        // User Header
        if (data.user) {
            this._userNameLabel.set_text(data.user.name || 'Antigravity User');
            this._userEmailLabel.set_text(data.user.email || '');
            this._userEmailLabel.visible = !!data.user.email;
        }
        if (data.plan) {
            this._planBadgeLabel.set_text(data.plan.name || 'Antigravity Quota');
            this._planBadgeLabel.visible = true;
        } else {
            this._planBadgeLabel.visible = false;
        }

        // Models list
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
                style_class: 'antigravity-bar-fill ' +
                    (m.remaining_percentage > 50 ? 'antigravity-bar-fill-good' :
                     m.remaining_percentage > 20 ? 'antigravity-bar-fill-warn' : 'antigravity-bar-fill-critical'),
            });

            // Layout progress bar proportionally when allocated
            track.connect('notify::allocation', () => {
                const totalWidth = track.get_width();
                if (totalWidth > 0) {
                    const fillWidth = Math.max(3, Math.round((m.remaining_percentage / 100.0) * totalWidth));
                    fill.set_width(fillWidth);
                }
            });

            track.add_child(fill);
            card.add_child(track);

            // Bottom Row: Reset info
            if (m.time_until_reset || m.reset_time) {
                const resetText = m.time_until_reset ?
                    `Resets in ${m.time_until_reset} (${m.reset_time || ''})` :
                    `Resets: ${m.reset_time}`;
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
        if (this._settings && this._settingsChangedId) {
            this._settings.disconnect(this._settingsChangedId);
            this._settingsChangedId = null;
        }
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
