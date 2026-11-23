# SPDX-License-Identifier: GPL-3.0-or-later
"""`themes apply` live inside the isolated session, `themes doctor` afterwards, and the
confirmation rule for experimental themes (ADR 0006).

Third-party extensions of the themes (dash-to-dock, blur-my-shell, ...) are not installed in the
container: doctor reports them as warnings ("enabled but not installed"), which is expected here;
a clean report means no error-level finding."""
import pytest

from gi.repository import GLib

from conftest import themes
from harness.shell import UUID

THEMES_BUS = 'io.github.OlehDzhumyk.Themes'
THEMES_PATH = '/io/github/OlehDzhumyk/Themes'

THEME_SKIN = {'macos': 'macos', 'windows11': 'windows11', 'fedora': 'default'}


@pytest.mark.parametrize('theme', list(THEME_SKIN))
def test_apply_live_and_doctor(make_shell, theme):
    shell = make_shell(skin='default')
    rc, res, err = themes(shell, 'apply', theme)
    # exit code 4: applied, but the automatic rollback had to turn an extension off
    assert res.get('rolled_back', []) == [], res.get('warnings')
    assert rc == 0, (res, err)
    assert res['ok'] and res['live'] and not res['relogin'], res
    st = shell.hook('bridgeState')
    assert st['state'] == 1 and st['skin'] == THEME_SKIN[theme], st
    shell.screenshot('applied')
    rc, rep, err = themes(shell, 'doctor', '--no-notify')
    assert rep['shell_running'] and not rep['extensions_disabled_by_gnome'], rep
    errors = [i for i in rep['issues'] if i['severity'] == 'error']
    assert errors == [], rep
    assert not [i for i in rep['issues'] if i.get('uuid') == UUID], rep
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


def test_apply_live_many_times_no_shell_crash(make_shell):
    """Switching themes back and forth toggles extensions while interface keys change: the case
    that crashed GNOME Shell 51.1 (docs/architecture.md, "Applying a theme"). Every apply must be
    live, need no rollback, and leave the same shell process running."""
    shell = make_shell(skin='default')
    pid = shell.shell_proc.pid
    for theme in ['macos', 'windows11', 'fedora', 'windows11', 'macos', 'fedora']:
        rc, res, err = themes(shell, 'apply', theme)
        assert res.get('rolled_back', []) == [], (theme, res.get('warnings'))
        assert rc == 0, (theme, res, err)
        assert res['live'] and not res['relogin'], (theme, res)
        assert shell.shell_proc.poll() is None and shell.shell_proc.pid == pid, theme
        assert shell.ext_state() == 1, theme
    rc, rep, err = themes(shell, 'doctor', '--no-notify')
    assert not [i for i in rep['issues'] if i['kind'] == 'interrupted-apply'], rep
    assert not rep['shell_crashes'], rep
    # the waits ended on the shell's signals, not on their upper bound, and the resets went to
    # the dconf writer as one change
    log = (shell.themes_root / 'apply.log').read_text()
    assert 'shell-settle=timeout' not in log and 'shell-settle=ok' in log, log
    assert '/per-key' not in log, log


def test_dbus_apply_needs_confirmation(make_shell):
    """The service refuses an experimental theme on Apply (ADR 0006); ApplyConfirmed exists for
    the confirmed case. Only the refusal is exercised with an experimental theme."""
    shell = make_shell(skin='default')
    before = shell.dconf_read('/org/gnome/shell/enabled-extensions')
    with pytest.raises(GLib.Error) as e:
        shell.call(THEMES_BUS, THEMES_PATH, THEMES_BUS, 'Apply',
                   GLib.Variant('(sas)', ('macos-glass', [])), '(bsas)', timeout_ms=120000)
    assert 'AccessDenied' in str(e.value), e.value
    assert shell.dconf_read('/org/gnome/shell/enabled-extensions') == before
    (exp,) = shell.call(THEMES_BUS, THEMES_PATH, THEMES_BUS, 'Experimental', None, '(a{ss})').unpack()
    assert 'macos-glass' in exp, exp
    relogin, _msg, _warn = shell.call(
        THEMES_BUS, THEMES_PATH, THEMES_BUS, 'ApplyConfirmed',
        GLib.Variant('(sas)', ('windows11', ['behavior'])), '(bsas)', timeout_ms=120000).unpack()
    assert not relogin
    (cur,) = shell.call(THEMES_BUS, THEMES_PATH, THEMES_BUS, 'Current', None, '(a{ss})').unpack()
    assert cur['behavior'] == 'windows11' and 'macos-glass' not in cur.values(), cur
