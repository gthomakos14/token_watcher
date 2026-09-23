import Adw from 'gi://Adw';
import Gtk from 'gi://Gtk';
import Gio from 'gi://Gio';
import GObject from 'gi://GObject';
import { ExtensionPreferences, gettext as _ } from 'resource:///org/gnome/Shell/Extensions/js/extensions/prefs.js';

export default class AntigravityTokenWatcherPreferences extends ExtensionPreferences {
    fillPreferencesWindow(window) {
        const settings = this.getSettings();

        const page = new Adw.PreferencesPage({
            title: _('General'),
            icon_name: 'preferences-system-symbolic',
        });

        // Display Group
        const displayGroup = new Adw.PreferencesGroup({
            title: _('Top Bar Display'),
            description: _('Configure how Antigravity quota is displayed in the panel'),
        });

        // Display Format
        const formatRow = new Adw.ComboRow({
            title: _('Display Format'),
            subtitle: _('Choose the text format for the top bar label'),
            model: new Gtk.StringList({
                strings: [
                    _('Percentage (e.g. 86%)'),
                    _('Model & Percentage (e.g. Flash: 86%)'),
                    _('Dual View (Flash & Claude)'),
                    _('Icon Only'),
                ],
            }),
        });

        const formats = ['percentage', 'model-and-percentage', 'dual', 'icon-only'];
        const currentFormat = settings.get_string('display-format');
        const formatIndex = formats.indexOf(currentFormat);
        formatRow.selected = formatIndex >= 0 ? formatIndex : 0;

        formatRow.connect('notify::selected', () => {
            settings.set_string('display-format', formats[formatRow.selected]);
        });
        displayGroup.add(formatRow);

        // Primary Model
        const primaryModelRow = new Adw.ComboRow({
            title: _('Primary Model'),
            subtitle: _('Which model to prioritize for top bar percentage'),
            model: new Gtk.StringList({
                strings: [
                    'Gemini Flash',
                    'Gemini Pro',
                    'Claude Sonnet',
                ],
            }),
        });

        const primaryModels = ['Flash', 'Pro', 'Sonnet'];
        const currentPrimary = settings.get_string('primary-model');
        const primaryIndex = primaryModels.indexOf(currentPrimary);
        primaryModelRow.selected = primaryIndex >= 0 ? primaryIndex : 0;

        primaryModelRow.connect('notify::selected', () => {
            settings.set_string('primary-model', primaryModels[primaryModelRow.selected]);
        });
        displayGroup.add(primaryModelRow);

        // Panel Box Position
        const panelBoxRow = new Adw.ComboRow({
            title: _('Panel Position'),
            subtitle: _('Which section of the top panel to place the icon'),
            model: new Gtk.StringList({
                strings: [
                    _('Right'),
                    _('Center'),
                    _('Left'),
                ],
            }),
        });

        const panelBoxes = ['right', 'center', 'left'];
        const currentBox = settings.get_string('panel-box');
        const boxIndex = panelBoxes.indexOf(currentBox);
        panelBoxRow.selected = boxIndex >= 0 ? boxIndex : 0;

        panelBoxRow.connect('notify::selected', () => {
            settings.set_string('panel-box', panelBoxes[panelBoxRow.selected]);
        });
        displayGroup.add(panelBoxRow);

        page.add(displayGroup);

        // Monitoring & Alerts Group
        const alertsGroup = new Adw.PreferencesGroup({
            title: _('Refresh & Alerts'),
            description: _('Auto-refresh and quota warning thresholds'),
        });

        // Refresh Interval
        const intervalRow = new Adw.SpinRow({
            title: _('Periodic Refresh Interval (seconds)'),
            subtitle: _('File changes are also detected instantly via inotify'),
            adjustment: new Gtk.Adjustment({
                lower: 5,
                upper: 300,
                step_increment: 5,
                page_increment: 15,
                value: settings.get_int('refresh-interval'),
            }),
        });
        intervalRow.connect('notify::value', () => {
            settings.set_int('refresh-interval', Math.round(intervalRow.value));
        });
        alertsGroup.add(intervalRow);

        // Warn Low Quota
        const warnRow = new Adw.SwitchRow({
            title: _('Highlight Low Quota'),
            subtitle: _('Color top bar label amber or red when quota drops'),
            active: settings.get_boolean('warn-low-quota'),
        });
        warnRow.connect('notify::active', () => {
            settings.set_boolean('warn-low-quota', warnRow.active);
        });
        alertsGroup.add(warnRow);

        // Low Quota Threshold
        const thresholdRow = new Adw.SpinRow({
            title: _('Low Quota Threshold (%)'),
            subtitle: _('Percentage below which warning styling is applied'),
            adjustment: new Gtk.Adjustment({
                lower: 5,
                upper: 50,
                step_increment: 5,
                page_increment: 10,
                value: settings.get_int('low-quota-threshold'),
            }),
        });
        thresholdRow.connect('notify::value', () => {
            settings.set_int('low-quota-threshold', Math.round(thresholdRow.value));
        });
        alertsGroup.add(thresholdRow);

        // Show Badges
        const badgeRow = new Adw.SwitchRow({
            title: _('Show Model Badges'),
            subtitle: _('Show "Fast" and model tags in the dropdown menu'),
            active: settings.get_boolean('show-badge'),
        });
        badgeRow.connect('notify::active', () => {
            settings.set_boolean('show-badge', badgeRow.active);
        });
        alertsGroup.add(badgeRow);

        page.add(alertsGroup);
        window.add(page);
    }
}
