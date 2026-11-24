// SPDX-License-Identifier: GPL-3.0-or-later
// Typed views of the few shell objects whose typings are `any` or incomplete.
import type Clutter from 'gi://Clutter';
import type Meta from 'gi://Meta';
import type Shell from 'gi://Shell';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import type {OutputStreamSlider} from 'resource:///org/gnome/shell/ui/status/volume.js';

/** The part of Main.inputMethod the bridge uses (the typings declare it `any`). */
export interface InputMethodInfo {
    readonly content_purpose: Clutter.InputContentPurpose;
    connectObject(signal: string, callback: () => void, owner: object): void;
    disconnectObject(owner: object): void;
    /** Where the focused client says its text cursor is (private to the shell). */
    readonly _cursorRect?: {readonly x: number; readonly y: number; readonly height: number} | null;
}

export function inputMethod(): InputMethodInfo {
    return Main.inputMethod as InputMethodInfo;
}

/** Main.pushModal with its real types (the typings declare an `any` overload); null: grab refused. */
export function pushModal(actor: Clutter.Actor, actionMode: Shell.ActionMode): Clutter.Grab | null {
    return Main.pushModal(actor, {actionMode}) as Clutter.Grab | null;
}

/** The window that has the focus; the typings say there always is one. */
export function focusWindow(): Meta.Window | null {
    return global.display.focus_window;
}

/** The shell's current action mode (the typings give it the literal type of its initial value). */
export function actionMode(): Shell.ActionMode {
    const mode: Shell.ActionMode = Main.actionMode;
    return mode;
}

/** The screen is locked (the typings give `isLocked` only to some session modes). */
export function sessionLocked(): boolean {
    return (Main.sessionMode as {isLocked?: boolean}).isLocked === true;
}

/** Text of anything that was thrown. */
export function errorText(e: unknown): string {
    return e instanceof Error ? e.message : String(e);
}

/** GNOME's output volume slider in quick settings (null until the shell has set it up). */
export function outputSlider(): OutputStreamSlider | null {
    return Main.panel.statusArea.quickSettings?._volumeOutput?._output ?? null;
}
