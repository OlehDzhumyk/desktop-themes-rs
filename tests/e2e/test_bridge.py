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
