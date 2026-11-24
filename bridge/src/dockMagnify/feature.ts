// SPDX-License-Identifier: GPL-3.0-or-later
// Icon magnification for the dash-to-dock dock in the macOS skin: the icon under the pointer grows
// and its neighbours grow less (cosine falloff), towards the inside of the screen.
// Active only when skin === 'macos' and animations are on (enable-animations, reduced motion).
// Non-invasive: it only sets scale and pivot on dash-to-dock's icon actors (and lifts clipping on
// the dock's own containers while the pointer is over it), and puts every value back when the
// pointer leaves, the dock goes away, the skin changes or the bridge is disabled. Without
// dash-to-dock, or with a different actor tree, it does nothing.
import Clutter from 'gi://Clutter';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {animationsEnabled} from '../ui/primitives.js';

/** dash-to-dock's names and classes (docking.js, dash.js). */
const CONTAINER_NAME = 'dashtodockContainer';
const BOX_NAME = 'dashtodockBox';
const ITEM_CLASS = 'dash-item-container';

/** Scale of the icon under the pointer. */
const PEAK = 1.45;
/** Distance, in icons, at which the scale is back to 1. */
const RANGE = 2.5;
const RESET_MS = 150;

interface Dock {
    container: Clutter.Actor;
    /** Actor whose events are followed (dash-to-dock's hover box, else the container). */
    events: Clutter.Actor;
    /** Handlers on `events`. */
    ids: number[];
    /** 'destroy' handler on `container`. */
    destroyId: number;
    /** Every actor whose scale was set, with its 'destroy' handler. */
    touched: Map<Clutter.Actor, number>;
    /** Containers whose clipping was lifted while magnifying, with the value to put back. */
    clips: [Clutter.Actor, 'clip_to_allocation' | 'clip_to_view'][];
    active: boolean;
}

function scaleAt(distance: number): number {
    if (distance >= RANGE)
        return 1;
    return 1 + (PEAK - 1) * (1 + Math.cos(Math.PI * distance / RANGE)) / 2;
}

function findByName(root: Clutter.Actor, name: string): Clutter.Actor | null {
    if (root.name === name)
        return root;
    for (const child of root.get_children()) {
        const found = findByName(child, name);
        if (found)
            return found;
    }
    return null;
}

function isItem(actor: Clutter.Actor): boolean {
    return actor instanceof St.Widget && actor.has_style_class_name(ITEM_CLASS);
}

/** The box that holds the application icons (the first one with dash item children). */
function findItemBox(root: Clutter.Actor): Clutter.Actor | null {
    const children = root.get_children();
    if (children.some(isItem))
        return root;
    for (const child of children) {
        const found = findItemBox(child);
        if (found)
            return found;
    }
    return null;
}

function resetActor(actor: Clutter.Actor): void {
    actor.remove_transition('scale-x');
    actor.remove_transition('scale-y');
    actor.set_scale(1, 1);
    actor.translation_y = 0;
}

export class DockMagnifyFeature {
    private _skin: string;
    private _stSettings = St.Settings.get();
    private _settingsIds: number[] = [];
    private _groupIds: number[] = [];
    private _docks = new Map<Clutter.Actor, Dock>();

    constructor(skin: string) {
        this._skin = skin;
        for (const prop of ['enable-animations', 'reduced-motion']) {
            this._settingsIds.push(
                this._stSettings.connect(`notify::${prop}`, () => { this._sync(); }));
        }
        this._sync();
    }

    setSkin(skin: string): void {
        if (this._skin === skin)
            return;
        this._skin = skin;
        this._sync();
    }

    private _sync(): void {
        const run = this._skin === 'macos' && animationsEnabled();
        if (run && this._groupIds.length === 0)
            this._start();
        else if (!run && this._groupIds.length > 0)
            this._stop();
    }

    private _start(): void {
        const group = Main.layoutManager.uiGroup;
        this._groupIds.push(
            group.connect('child-added', (_g: Clutter.Actor, child: Clutter.Actor) => { this._track(child); }),
            group.connect('child-removed', (_g: Clutter.Actor, child: Clutter.Actor) => { this._untrack(child); }),
        );
        for (const child of group.get_children())
            this._track(child);
    }

    private _stop(): void {
        const group = Main.layoutManager.uiGroup;
        for (const id of this._groupIds)
            group.disconnect(id);
        this._groupIds = [];
        for (const container of [...this._docks.keys()])
            this._untrack(container);
    }

    private _track(container: Clutter.Actor): void {
        if (container.name !== CONTAINER_NAME || this._docks.has(container))
            return;
        const events = findByName(container, BOX_NAME) ?? container;
        const dock: Dock = {container, events, ids: [], destroyId: 0, touched: new Map(), clips: [], active: false};
        dock.ids.push(
            events.connect('motion-event', (_a: Clutter.Actor, event: Clutter.Event) => {
                this._magnify(dock, event);
                return Clutter.EVENT_PROPAGATE;
            }),
            events.connect('leave-event', (_a: Clutter.Actor, event: Clutter.Event) => {
                // the typings say there is always one; there is none when the pointer leaves the stage
                const to = event.get_related() as Clutter.Actor | null;
                if (!to || !events.contains(to))
                    this._reset(dock, true);
                return Clutter.EVENT_PROPAGATE;
            }),
        );
        dock.destroyId = container.connect('destroy', () => { this._untrack(container); });
        this._docks.set(container, dock);
    }

    private _untrack(container: Clutter.Actor): void {
        const dock = this._docks.get(container);
        if (!dock)
            return;
        this._docks.delete(container);
        for (const id of dock.ids)
            dock.events.disconnect(id);
        container.disconnect(dock.destroyId);
        dock.active = false;
        for (const [actor, id] of dock.touched) {
            actor.disconnect(id);
            resetActor(actor);
        }
        dock.touched.clear();
        this._restoreClips(dock);
    }

    private _magnify(dock: Dock, event: Clutter.Event): void {
        const box = findItemBox(dock.events);
        if (!box)
            return;
        const items = box.get_children().filter(c => c.visible && isItem(c));
        if (items.length === 0)
            return;
        const [stageX, stageY] = event.get_coords();
        const [ok, x, y] = box.transform_stage_point(stageX, stageY);
        if (!ok)
            return;

        const vertical = box.height > box.width;
        const monitor = Main.layoutManager.findMonitorForActor(dock.container);
        const [cx, cy] = dock.container.get_transformed_position();
        const [cw, ch] = dock.container.get_transformed_size();
        // grow away from the screen edge the dock sits on
        const [px, py] = vertical
            ? [monitor && cx + cw / 2 > monitor.x + monitor.width / 2 ? 1 : 0, 0.5]
            : [0.5, monitor && cy + ch / 2 < monitor.y + monitor.height / 2 ? 0 : 1];

        if (!dock.active) {
            dock.active = true;
            this._liftClips(dock, box);
        }
        const pointer = vertical ? y : x;
        for (const item of items) {
            const target = item.get_first_child() ?? item;
            const a = item.get_allocation_box();
            const size = vertical ? a.get_height() : a.get_width();
            if (size <= 0)
                continue;
            const center = vertical ? (a.y1 + a.y2) / 2 : (a.x1 + a.x2) / 2;
            const scale = scaleAt(Math.abs(pointer - center) / size);
            if (!dock.touched.has(target)) {
                if (scale === 1)
                    continue;
                dock.touched.set(target, target.connect('destroy', () => { dock.touched.delete(target); }));
            }
            target.remove_transition('scale-x');
            target.remove_transition('scale-y');
            target.set_pivot_point(px, py);
            target.set_scale(scale, scale);
        }
    }

    private _reset(dock: Dock, animate: boolean): void {
        if (!dock.active)
            return;
        dock.active = false;
        let pending = 0;
        const done = () => {
            pending--;
            if (pending <= 0 && !dock.active)
                this._restoreClips(dock);
        };
        for (const actor of dock.touched.keys()) {
            if (!animate || !actor.mapped) {
                resetActor(actor);
                continue;
            }
            pending++;
            // snake_case: the shell names the transitions after these keys ('scale-x'), and
            // onStopped only fires for keys that match a transition (the typings want camelCase)
            actor.ease({
                scale_x: 1,
                scale_y: 1,
                duration: RESET_MS,
                mode: Clutter.AnimationMode.EASE_OUT_QUAD,
                onStopped: done,
            } as Parameters<Clutter.Actor['ease']>[0]);
        }
        if (pending === 0)
            this._restoreClips(dock);
    }

    /** Magnified icons reach past the dock: lift its own clipping from the icon box up. */
    private _liftClips(dock: Dock, box: Clutter.Actor): void {
        if (dock.clips.length > 0)
            return;
        for (let a: Clutter.Actor | null = box; a; a = a.get_parent()) {
            if (a.clip_to_allocation) {
                dock.clips.push([a, 'clip_to_allocation']);
                a.clip_to_allocation = false;
            }
            const viewport = a as Clutter.Actor & {clip_to_view?: boolean};
            if (viewport.clip_to_view === true) {
                dock.clips.push([a, 'clip_to_view']);
                viewport.clip_to_view = false;
            }
            if (a === dock.container)
                break;
        }
    }

    private _restoreClips(dock: Dock): void {
        for (const [actor, prop] of dock.clips) {
            if (actor !== dock.container && !dock.container.contains(actor))
                continue;
            (actor as Clutter.Actor & Record<typeof prop, boolean>)[prop] = true;
        }
        dock.clips = [];
    }

    destroy(): void {
        for (const id of this._settingsIds)
            this._stSettings.disconnect(id);
        this._settingsIds = [];
        this._stop();
    }
}
