# SPDX-License-Identifier: GPL-3.0-or-later
"""The bridge enables cleanly on every skin and draws the skin's panel."""
import pytest

from conftest import PANEL_CLASS, SKINS

ACTIVE = 1


@pytest.mark.smoke
@pytest.mark.parametrize('skin', list(SKINS))
def test_enables_without_errors(make_shell, skin):
    shell = make_shell(skin=skin)
    assert shell.ext_state() == ACTIVE, shell.ext_info()
    st = shell.hook('bridgeState')
    assert st['skin'] == skin
    assert st['panelClass'] == PANEL_CLASS[skin]
    assert st['keybound']
    shell.screenshot('desktop')
    # the make_shell fixture fails the test on any JS error of the bridge in the log


def test_skin_switch_live(make_shell):
    shell = make_shell(skin='default')
    for skin in ('macos', 'windows11', 'default'):
        shell.dconf_write('/io/github/OlehDzhumyk/Desk/skin', f"'{skin}'")
        shell.hook('waitBridge', 'skin', skin)
        assert shell.hook('bridgeState')['panelClass'] == PANEL_CLASS[skin]


ACCENT_FROM_WALLPAPER = '/io/github/OlehDzhumyk/Desk/appearance/accent-from-wallpaper'
ACCENT_COLOR = '/org/gnome/desktop/interface/accent-color'
PICKER = {'default': 'switch', 'macos': 'dots', 'windows11': 'dropdown'}


@pytest.mark.parametrize('skin', list(SKINS))
def test_accent_picker_per_skin(make_shell, skin):
    """theme-from-wallpaper spec, "Skins": macOS dot grid, Windows 11 «Automatic» drop-down,
    the default switch; each writes the settings deskd follows."""
    shell = make_shell(skin=skin)
    p = shell.hook('accentPicker')
    assert p['kind'] == PICKER[skin], p
    if skin == 'macos':
        names = [d['name'] for d in p['dots']]
        assert len(names) == 10 and names[0] == 'Blue' and names[-1] == 'From wallpaper', names
        assert all(d['focusable'] for d in p['dots']), p
        shell.hook('pickAccent', 'Teal')
        assert shell.dconf_read(ACCENT_COLOR) == "'teal'"
        assert shell.dconf_read(ACCENT_FROM_WALLPAPER) in ('', 'false')
        assert [d['name'] for d in shell.hook('accentPicker')['dots'] if d['checked']] == ['Teal']
        shell.hook('pickAccent', 'From wallpaper')
        assert shell.dconf_read(ACCENT_FROM_WALLPAPER) == 'true'
        # keyboard path: the dots are one row in focus order
        assert shell.hook('focusAccentDot', 0) == 'Blue'
        shell.hook_async('keys', 'right')
        shell.hook_async('waitFocus', 'Teal')
    elif skin == 'windows11':
        assert [o['label'] for o in p['options']] == ['Automatic', 'Manual'], p
        assert p['label'] == 'Accent colour: Manual', p
        shell.hook('pickAccent', 'Automatic')
        assert shell.dconf_read(ACCENT_FROM_WALLPAPER) == 'true'
        p = shell.hook('accentPicker')
        assert p['label'] == 'Accent colour: Automatic' and p['options'][0]['checked'], p
    else:
        assert p['label'] == 'Accent from wallpaper' and p['on'] is False, p
    shell.screenshot('accent-picker')


def test_accent_picker_follows_the_skin_live(make_shell):
    shell = make_shell(skin='default')
    for skin in ('macos', 'windows11', 'default'):
        shell.dconf_write('/io/github/OlehDzhumyk/Desk/skin', f"'{skin}'")
        shell.hook('waitBridge', 'skin', skin)
        assert shell.hook('accentPicker')['kind'] == PICKER[skin], skin
