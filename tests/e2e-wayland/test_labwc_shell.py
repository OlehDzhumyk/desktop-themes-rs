# SPDX-License-Identifier: GPL-3.0-or-later
"""desk-shell --skin windows11 and deskd on headless labwc (ADR 0012), through the real binaries.

One session per module (conftest.Session): labwc with the bundled rc.xml, then deskd, then
desk-shell, as data/labwc/windows11/autostart starts them. The flyouts are driven the way the
rc.xml key bindings drive them: a second `desk-shell --toggle-...` process.
"""

import pathlib

import pytest

from conftest import DESK, DESK_SHELL, LAYER_LIBRARY, destroy_layer, new_layer, problems

# flag -> layer-shell namespace of the flyout it toggles (start_menu.rs, quick_settings.rs,
# clipboard_flyout.rs)
FLYOUTS = {
    '--toggle-start': 'desk-shell-start',
    '--toggle-quick-settings': 'desk-shell-quick-settings',
    '--toggle-clipboard': 'desk-shell-clipboard',
}


def layer_surface(log, namespace, since=0):
    """The object id of the next zwlr_layer_surface_v1 that desk-shell creates for `namespace`."""
    return log.wait(new_layer(namespace), since).group(1)


def test_desk_shell_loads_gtk4_layer_shell(session):
    maps = pathlib.Path(f'/proc/{session.procs["desk-shell"].pid}/maps').read_text()
    assert LAYER_LIBRARY in maps
    assert not [line for line in session.logs['desk-shell'].snapshot()
                if 'using normal windows' in line]


def test_taskbar_is_a_layer_surface_on_labwc(session):
    log = session.logs['desk-shell']
    taskbar = layer_surface(log, 'desk-shell-taskbar')
    # auto exclusive zone: labwc keeps windows off the bar once GTK knows its height
    log.wait(rf'-> zwlr_layer_surface_v1#{taskbar}\.set_exclusive_zone\([1-9]\d*\)')


def test_deskd_and_desk_shell_own_their_names(session):
    assert session.name_owner(DESK)
    assert session.name_owner(DESK_SHELL)


@pytest.mark.parametrize('flag', list(FLYOUTS))
def test_toggle_reaches_the_running_shell(session, flag):
    log = session.logs['desk-shell']
    owner = session.name_owner(DESK_SHELL)
    for step in ('open', 'close'):
        since = log.mark()
        r = session.run_shell(flag)
        err = r.stderr.decode(errors='replace')
        assert r.returncode == 0, f'{flag} ({step}) exited {r.returncode}: {err}'
        assert 'already running' not in err
        assert not problems(err.splitlines()), err
        if step == 'open':
            surface = layer_surface(log, FLYOUTS[flag], since)
        else:
            log.wait(destroy_layer(surface), since)
    # the primary instance answered both times: still running, same bus name owner
    assert session.alive('desk-shell')
    assert session.name_owner(DESK_SHELL) == owner


def test_shell_and_deskd_stay_alive_without_errors(session):
    for name in ('labwc', 'deskd', 'desk-shell'):
        assert session.alive(name), f'{name} exited (see {name}.log)'
    assert session.name_owner(DESK)
    assert session.name_owner(DESK_SHELL)
    for name in ('deskd', 'desk-shell'):
        found = problems(session.logs[name].snapshot())
        assert not found, f'{name}: ' + '\n'.join(found)
    # every layer surface the toggles opened was closed again; the taskbar stays
    log = session.logs['desk-shell'].snapshot()
    created, destroyed, opened = new_layer(), destroy_layer(), {}
    for line in log:
        if m := created.search(line):
            opened[m.group(1)] = m.group(2)
        elif m := destroyed.search(line):
            opened.pop(m.group(1), None)
    assert sorted(opened.values()) == ['desk-shell-taskbar']
