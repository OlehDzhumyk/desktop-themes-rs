# SPDX-License-Identifier: GPL-3.0-or-later
"""`themes apply` live inside the isolated session, `themes doctor` afterwards, and the
confirmation rule for experimental themes (ADR 0006).

Third-party extensions of the themes (dash-to-dock, blur-my-shell, ...) are not installed in the
container: doctor reports them as warnings ("enabled but not installed"), which is expected here;
a clean report means no error-level finding."""
import pytest

from conftest import themes
from harness.shell import UUID

THEME_SKIN = {'macos': 'macos', 'windows11': 'windows11', 'fedora': 'default'}


@pytest.mark.parametrize('theme', list(THEME_SKIN))
def test_apply_live_and_doctor(make_shell, theme):
    shell = make_shell(skin='default')
    rc, res, err = themes(shell, 'apply', theme)
    assert rc == 0, (res, err)
    assert res['ok'] and res['live'] and not res['relogin'], res
    assert UUID not in res.get('rolled_back', []), res
    st = shell.hook('bridgeState')
    assert st['state'] == 1 and st['skin'] == THEME_SKIN[theme], st
    shell.screenshot('applied')
    rc, rep, err = themes(shell, 'doctor', '--no-notify')
    errors = [i for i in rep['items'] if i['level'] == 'error']
    assert errors == [], rep
    assert not [i for i in rep['items'] if i.get('extension') == UUID], rep
    rc, cur, _ = themes(shell, 'current')
    assert all(v == theme for v in cur['modules'].values()), cur


def test_experimental_needs_yes(make_shell):
    shell = make_shell(skin='default')
    before = shell.dconf_read('/org/gnome/shell/enabled-extensions')
    rc, out, err = themes(shell, 'apply', 'macos-glass')
    assert rc != 0, (out, err)
    assert shell.dconf_read('/org/gnome/shell/enabled-extensions') == before
    rc, cur, _ = themes(shell, 'current')
    assert 'macos-glass' not in (cur or {}).get('modules', {}).values()
