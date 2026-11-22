// SPDX-License-Identifier: GPL-3.0-or-later
// A shell modal dialog asking for confirmation (e.g. before applying an experimental theme).
// Esc / “Cancel” answer no; the confirming button is not the default.
import Clutter from 'gi://Clutter';

import * as Dialog from 'resource:///org/gnome/shell/ui/dialog.js';
import * as ModalDialog from 'resource:///org/gnome/shell/ui/modalDialog.js';

import {_} from './i18n.js';

export function confirm(title: string, description: string, okLabel: string): Promise<boolean> {
    return new Promise(resolve => {
        const dialog = new ModalDialog.ModalDialog({destroyOnClose: true});
        dialog.contentLayout.add_child(new Dialog.MessageDialogContent({title, description}));
        let answered = false;
        const answer = (yes: boolean) => {
            if (answered)
                return;
            answered = true;
            dialog.close();
            resolve(yes);
        };
        dialog.setButtons([
            {label: _('Cancel'), action: () => { answer(false); }, key: Clutter.KEY_Escape, default: true},
            {label: okLabel, action: () => { answer(true); }},
        ]);
        if (!dialog.open())
            answer(false);
    });
}
