# SPDX-License-Identifier: GPL-3.0-or-later
"""The clean-room macOS logo menu in desk-bridge."""
import pytest

EXPECTED_ITEMS = [
    'About This System',
    'System Settings…',
    'Applications',
    'Force Quit…',
    'Sleep',
    'Restart…',
    'Shut Down…',
    'Lock Screen',
    'Log Out…',
]


def test_macos_logo_menu_shown(make_shell):
    shell = make_shell(skin='macos')
    st = shell.hook('logoMenuState')
    assert st['present'] is True
    assert st['visible'] is True
    assert st['activitiesVisible'] is False
    for label in EXPECTED_ITEMS:
        assert label in st['items'], f'missing {label!r} in {st["items"]!r}'


@pytest.mark.parametrize('skin', ['windows11', 'default'])
def test_other_skins_no_logo_menu(make_shell, skin):
    shell = make_shell(skin=skin)
    st = shell.hook('logoMenuState')
    assert st['present'] is False
    assert st['activitiesVisible'] is True


def test_disable_logo_menu_restores_activities(make_shell):
    shell = make_shell(skin='macos')
    st = shell.hook('logoMenuState')
    assert st['present'] is True
    assert st['activitiesVisible'] is False

    shell.dconf_write('/io/github/OlehDzhumyk/Desk/logo-menu/enabled', 'false')
    shell.hook('waitLogoMenu', 'present', False)

    st = shell.hook('logoMenuState')
    assert st['present'] is False
    assert st['activitiesVisible'] is True
