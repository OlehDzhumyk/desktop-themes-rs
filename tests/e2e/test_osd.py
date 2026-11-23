# SPDX-License-Identifier: GPL-3.0-or-later
"""The skin-aware OSD (docs/spec/features/osd.md): the bridge patches Main.osdWindowManager only
for the windows11 and macos skins and restores it exactly otherwise."""
import pytest

SKIN_KEY = '/io/github/OlehDzhumyk/Desk/skin'
ENABLED_KEY = '/io/github/OlehDzhumyk/Desk/osd/enabled'


def test_default_skin_leaves_stock_osd(make_shell):
    """AC-OSD-05: with the default skin the manager is not modified and no bridge OSD is drawn."""
    shell = make_shell(skin='default')
    st = shell.hook('osdShowVolume', 0.4)
    assert st['patched'] == []
    assert st['actors'] == 0


@pytest.mark.parametrize('skin', ['windows11', 'macos'])
def test_skinned_osd_one_click_through_surface(make_shell, skin):
    """AC-OSD-01/02/03/06: one OSD with the level as a number, click-through, one hide timer."""
    shell = make_shell(skin=skin)
    assert shell.hook('osdState')['patched']
    st = shell.hook('osdShowVolume', 0.4)
    assert st['visible'] and f'desk-skin-{skin}' in st['classes']
    assert st['number'] == '40'
    assert st['reactive'] is False
    assert st['timer'] > 0
    assert st['name'] == 'Volume 40 percent'
    for _ in range(10):
        st = shell.hook('osdShowVolume', 0.5)
    assert st['actors'] == 1 and st['timer'] > 0


def test_over_amplification_marked(make_shell):
    """AC-OSD-04: the part above 100 % is drawn and the number is shown."""
    shell = make_shell(skin='windows11')
    st = shell.hook('osdShowVolume', 1.2, 1.5)
    assert st['over'] is True
    assert st['number'] == '120'


def test_skin_switch_and_disable_restore_stock(make_shell):
    """AC-OSD-05/07 and the skin switch edge case: hide at once, restore the manager."""
    shell = make_shell(skin='windows11')
    assert shell.hook('osdShowVolume', 0.4)['visible']
    shell.dconf_write(SKIN_KEY, "'default'")
    st = shell.hook('waitOsd', False)
    assert st['actors'] == 0
    shell.dconf_write(SKIN_KEY, "'macos'")
    shell.hook('waitOsd', True)
    shell.dconf_write(ENABLED_KEY, 'false')
    assert shell.hook('waitOsd', False)['actors'] == 0
    shell.dconf_write(ENABLED_KEY, 'true')
    shell.hook('waitOsd', True)
    shell.hook('osdShowVolume', 0.4)
    shell.set_extension(False)
    st = shell.hook('osdState')
    assert st['patched'] == [] and st['actors'] == 0
