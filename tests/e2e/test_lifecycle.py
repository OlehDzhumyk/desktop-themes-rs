# SPDX-License-Identifier: GPL-3.0-or-later
"""Enable/disable and shutdown of the bridge: nothing may be left behind.

harness/hook.js records every GObject signal handler and GLib source the bridge's code creates
(by the bridge's path in the JS stack) and reports the ones still alive after disable(), plus any
actor with a desk- style class or a bridge GType left on the stage."""
import pytest

from conftest import SKINS
from harness.shell import UUID

SKIN_KEY = '/io/github/OlehDzhumyk/Desk/skin'
LANG_KEY = '/io/github/OlehDzhumyk/Desk/language'


def exercise(shell, shortcut):
    """Walk the code paths that create and drop UI: panel open/close, every skin, a language
    switch (rebuilds the UI)."""
    shell.keys(shortcut)
    shell.hook('waitPanel', True)
    shell.keys('Escape')
    shell.hook('waitPanel', False)
    for skin in ('macos', 'windows11', 'default'):
        shell.dconf_write(SKIN_KEY, f"'{skin}'")
        shell.hook('waitBridge', 'skin', skin)
        if skin != 'default':
            shell.hook('osdShowVolume', 0.4)  # the skinned OSD: actor, hide timer
    for lang in ('uk', 'system'):
        shell.dconf_write(LANG_KEY, f"'{lang}'")
        shell.hook('waitBridge', 'language', lang)
    shell.keys(SKINS['default'][1])
    shell.hook('waitPanel', True)
    shell.keys('Escape')
    shell.hook('waitPanel', False)


def test_disable_leaves_nothing_behind(make_shell):
    shell = make_shell(skin='default', extensions=())
    assert shell.hook('startTracking', UUID)
    try:
        shell.set_extension(True)
        exercise(shell, SKINS['default'][1])
        shell.set_extension(False)
        left = shell.hook('leaks')
    finally:
        shell.hook('stopTracking')
    assert left['recorded'] > 0, 'the tracker saw nothing: is the hook working?'
    assert left['handlers'] == [], 'signal handlers still connected after disable()'
    assert left['sources'] == [], 'main-loop sources still pending after disable()'
    assert left['actors'] == [], 'actors left on the stage after disable()'
    assert not left['keybinding'], 'the panel keybinding is still registered'
    # and it comes back cleanly
    shell.set_extension(True)
    assert shell.hook('bridgeState')['keybound']


@pytest.mark.parametrize('disable_first', [True, False], ids=['disable-then-logout', 'logout-enabled'])
def test_shutdown_without_gc_warnings(make_shell, disable_first):
    """Regression for the 'call back into JSAPI during the sweeping phase of GC ... Clutter actor
    with ::destroy signals connected' warning seen at logout: the bridge must not be part of it."""
    shell = make_shell(skin='default')
    exercise(shell, SKINS['default'][1])
    if disable_first:
        shell.set_extension(False)
        shell.set_extension(True)
        shell.set_extension(False)
    shell.hook('gc')
    shell.stop()
    ours = [w for w in shell.gc_warnings() if 'desk-bridge' in w or 'desk_bridge' in w]
    assert ours == [], '\n\n'.join(ours)
    # anything else the bridge caused during shutdown (e.g. touching an actor the stage destroyed)
    assert shell.errors_from((UUID,)) == []
