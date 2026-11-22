# SPDX-License-Identifier: GPL-3.0-or-later
"""Per-feature shortcuts of the settings registry (`themes shortcut`): an override reaches the
bridge live, a taken accelerator is refused and nothing is written."""
from conftest import themes

KEY = '/io/github/OlehDzhumyk/Desk/clipboard/toggle-shortcut'
OVERRIDE = '/io/github/OlehDzhumyk/Desk/shortcuts/clipboard-toggle'


def test_override_takes_effect(make_shell):
    shell = make_shell(skin='default')
    rc, out, err = themes(shell, 'shortcut', 'set', 'clipboard-toggle', '<Super><Alt>c')
    assert rc == 0, (out, err)
    assert shell.dconf_read(KEY) == "['<Super><Alt>c']"
    assert shell.dconf_read(OVERRIDE) == "['<Super><Alt>c']"
    shell.keys('Super+Alt+c')
    shell.hook('waitPanel', True)
    shell.keys('Escape')
    shell.hook('waitPanel', False)
    rc, out, _ = themes(shell, 'shortcut', 'list')
    row = next(r for r in out if r['id'] == 'clipboard-toggle')
    assert row['source'] == 'override' and row['in_force'] == ['<Super><Alt>c']


def test_conflict_is_refused(make_shell):
    taken = '/org/gnome/desktop/wm/keybindings/switch-to-workspace-1'
    shell = make_shell(skin='default', dconf=f"[org/gnome/desktop/wm/keybindings]\nswitch-to-workspace-1=['<Super><Alt>1']\n")
    before = shell.dconf_read(KEY)
    rc, out, _ = themes(shell, 'shortcut', 'set', 'clipboard-toggle', '<Super><Alt>1')
    assert rc == 1
    assert out['ok'] is False and out['conflicts'], out
    assert shell.dconf_read(KEY) == before
    assert shell.dconf_read(OVERRIDE) == ''
    assert shell.dconf_read(taken) == "['<Super><Alt>1']"
    assert shell.hook('bridgeState')['keybound']
