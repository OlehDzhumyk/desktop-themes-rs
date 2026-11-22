# SPDX-License-Identifier: GPL-3.0-or-later
"""Clipboard end to end: a Wayland client copies text, deskd records it, the theme's shortcut
opens the skin's panel, selecting an item pastes it into the focused window."""
import pytest

from conftest import SKINS, Clipboard

SAMPLES = ['Перший зразок тексту', 'Second sample: hello world', 'Третій зразок — 3']


@pytest.mark.smoke
@pytest.mark.parametrize('skin', list(SKINS))
def test_copy_history_panel_paste(make_shell, procs, paste_target, skin):
    shell = make_shell(skin=skin)
    target = paste_target(shell)
    clip = Clipboard(shell)
    for t in SAMPLES:
        clip.copy_and_wait(t, procs)
    history = clip.texts()
    assert history[:3] == SAMPLES[::-1], history

    shell.keys(SKINS[skin][1])
    shell.hook('waitPanel', True)
    assert shell.hook('panelItems')[:3] == SAMPLES[::-1]
    shell.screenshot('panel')
    # newest first: one step down is the second sample
    shell.keys('Down')
    shell.screenshot('selected')
    shell.keys('Return')
    shell.hook('waitPanel', False)
    got = target.wait(lambda o: o.get('text'), 'the pasted text')
    assert got['text'] == SAMPLES[1]
    shell.screenshot('pasted')
