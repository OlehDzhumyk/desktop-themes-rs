# SPDX-License-Identifier: GPL-3.0-or-later
"""deskd's Stage 3/4 engines over the real session bus (window layouts, search, colour picker,
screenshots, OCR) and the bridge features that use them.

deskd answers these methods only to the shell (ADR 0008), so the calls go through the shell's own
bus connection (hook.js deskCall); the test process subscribes to the signals itself. Each session
has its own HOME, so deskd's files (layouts.json, colors.json, ...) start empty."""
import json
import pathlib
import struct
import zlib

import pytest

from gi.repository import Gio, GLib

from conftest import DESK
from harness.shell import wait_for

ROOT = '/io/github/OlehDzhumyk/Desk'
LAYOUTS = (f'{ROOT}/Layouts', f'{DESK}.Layouts')
SEARCH = (f'{ROOT}/Search', f'{DESK}.Search')
COLOR = (f'{ROOT}/ColorPicker', f'{DESK}.ColorPicker')
CAPTURE = (f'{ROOT}/Capture', f'{DESK}.Capture')
OCR = (f'{ROOT}/Ocr', f'{DESK}.Ocr')

BUILTIN_LAYOUTS = ['halves', 'thirds', 'two-thirds', 'quarters', 'focus', 'stack']
MONITOR = 'e2e-virtual-monitor'
FEATURES = ['_colorPicker', '_ocr', '_capture', '_layouts', '_quickLook', '_shortcutGuide']


def desk(shell, obj, method, sig='', *args):
    """A deskd call as the shell makes it: {ok, value} or {ok: False, name, message}."""
    path, iface = obj
    return shell.hook('deskCall', path, iface, method, sig, list(args))


def ok(reply):
    assert reply['ok'], reply
    return reply['value']


def error_name(reply):
    assert not reply['ok'], reply
    return reply['name']


class Signals:
    """Changed signals of one deskd object, counted from the moment of subscription."""

    def __init__(self, shell, obj):
        self.shell = shell
        self.count = 0
        self._wake = None
        path, iface = obj
        self.sid = shell.bus.signal_subscribe(DESK, iface, 'Changed', path, None,
                                              Gio.DBusSignalFlags.NONE, self._got)

    def _got(self, *_):
        self.count += 1
        if self._wake:
            self._wake()

    def wait(self, n, what):
        """Wait until at least `n` signals came (they may already be queued)."""
        def setup(done):
            self._wake = lambda: self.count >= n and done(True)
            if self.count >= n:
                done(True)
            return lambda: setattr(self, '_wake', None)
        return wait_for(setup, 20, what)

    def close(self):
        self.shell.bus.signal_unsubscribe(self.sid)


def png(width, height, rgb=(0, 0, 0), text=b''):
    """A minimal RGB PNG; `text` adds a tEXt chunk (metadata deskd must remove)."""
    def chunk(ty, data):
        return struct.pack('>I', len(data)) + ty + data + struct.pack('>I', zlib.crc32(ty + data))
    row = b'\x00' + bytes(rgb) * width
    out = b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 2, 0, 0, 0))
    if text:
        out += chunk(b'tEXt', b'Comment\x00' + text)
    return out + chunk(b'IDAT', zlib.compress(row * height)) + chunk(b'IEND', b'')


def overlap(a, b):
    return a['x'] < b['x'] + b['w'] and b['x'] < a['x'] + a['w'] \
        and a['y'] < b['y'] + b['h'] and b['y'] < a['y'] + a['h']


# ---- window layouts (docs/spec/features/window-layouts.md)

def test_layouts_list_and_zones_in_the_work_area(make_shell):
    shell = make_shell(skin='windows11')
    layouts = json.loads(ok(desk(shell, LAYOUTS, 'ListLayouts'))[0])
    assert [lay['id'] for lay in layouts if lay['builtin']] == BUILTIN_LAYOUTS, layouts
    assert all(lay['zones'] for lay in layouts)

    # AC-WIN-01: the left half of a 1920x1080 work area under a 27 px top bar, gaps 8/6
    zones = json.loads(ok(desk(shell, LAYOUTS, 'ResolveZones', '(siiii)', 'halves', 0, 27, 1920, 1053))[0])
    assert zones[0] == {'x': 8, 'y': 36, 'w': 950, 'h': 1036}, zones
    assert zones[1]['x'] - (zones[0]['x'] + zones[0]['w']) == 6, zones
    assert zones[1]['x'] + zones[1]['w'] == 1912, zones

    # every built-in layout in the shell's real work area: inside it, even, no overlap
    wa = shell.hook('workArea')
    assert wa['w'] > 0 and wa['h'] > 0, wa
    for lay in layouts:
        zones = json.loads(ok(desk(shell, LAYOUTS, 'ResolveZones', '(siiii)', lay['id'],
                                   wa['x'], wa['y'], wa['w'], wa['h']))[0])
        assert len(zones) == len(lay['zones']), (lay['id'], zones)
        for i, z in enumerate(zones):
            assert all(z[k] % 2 == 0 for k in 'xywh'), (lay['id'], z)
            assert z['w'] > 0 and z['h'] > 0, (lay['id'], z)
            assert wa['x'] <= z['x'] and z['x'] + z['w'] <= wa['x'] + wa['w'], (lay['id'], z, wa)
            assert wa['y'] <= z['y'] and z['y'] + z['h'] <= wa['y'] + wa['h'], (lay['id'], z, wa)
            assert not any(overlap(z, o) for o in zones[i + 1:]), (lay['id'], zones)

    assert error_name(desk(shell, LAYOUTS, 'ResolveZones', '(siiii)', 'nope', 0, 0, 1920, 1080)) \
        == 'org.freedesktop.DBus.Error.InvalidArgs'


def test_layouts_active_layout_round_trip(make_shell):
    shell = make_shell(skin='windows11')
    assert ok(desk(shell, LAYOUTS, 'ActiveLayout', '(s)', MONITOR)) == ['halves']
    changed = Signals(shell, LAYOUTS)
    try:
        ok(desk(shell, LAYOUTS, 'SetActiveLayout', '(ss)', MONITOR, 'quarters'))
        changed.wait(1, 'Layouts.Changed after SetActiveLayout')
        assert ok(desk(shell, LAYOUTS, 'ActiveLayout', '(s)', MONITOR)) == ['quarters']
        # an unknown layout is refused and changes nothing
        assert error_name(desk(shell, LAYOUTS, 'SetActiveLayout', '(ss)', MONITOR, 'nope')) \
            == 'org.freedesktop.DBus.Error.InvalidArgs'
        assert ok(desk(shell, LAYOUTS, 'ActiveLayout', '(s)', MONITOR)) == ['quarters']
        # back to the default: a new choice again, so a second Changed
        ok(desk(shell, LAYOUTS, 'SetActiveLayout', '(ss)', MONITOR, 'halves'))
        changed.wait(2, 'Layouts.Changed after the reset')
        assert ok(desk(shell, LAYOUTS, 'ActiveLayout', '(s)', MONITOR)) == ['halves']
    finally:
        changed.close()


# ---- search (docs/spec/features/search.md)

def test_search_calculator_actions_and_usage(make_shell):
    shell = make_shell(skin='macos')
    # AC-SRCH-03
    results = json.loads(ok(desk(shell, SEARCH, 'Query', '(su)', '= 12*(3+4)', 10))[0])
    assert results[0]['kind'] == 'calculator', results
    assert results[0]['title'] == '84' and results[0]['activate'] == '84', results

    results = json.loads(ok(desk(shell, SEARCH, 'Query', '(su)', '> lock', 10))[0])
    assert results and all(r['kind'] == 'action' for r in results), results
    assert results[0]['activate'] == 'lock', results

    # AC-SRCH-06: web-search is off by default, so no query gives a web result
    for text in ('weather', 'text editor', '= 2+2'):
        results = json.loads(ok(desk(shell, SEARCH, 'Query', '(su)', text, 10))[0])
        assert all(r['kind'] != 'web' for r in results), (text, results)

    ok(desk(shell, SEARCH, 'RecordLaunch', '(s)', 'action:lock'))
    ok(desk(shell, SEARCH, 'ClearUsage'))
    assert error_name(desk(shell, SEARCH, 'RecordLaunch', '(s)', '')) \
        == 'org.freedesktop.DBus.Error.InvalidArgs'


def test_search_web_result_only_when_enabled(make_shell):
    """The other side of AC-SRCH-06: with web-search on, a plain query ends with the web item,
    and a prefixed scope still has none."""
    shell = make_shell(skin='macos', dconf='[io/github/OlehDzhumyk/Desk/search]\nweb-search=true\n')
    results = json.loads(ok(desk(shell, SEARCH, 'Query', '(su)', 'weather', 10))[0])
    assert results[-1]['kind'] == 'web' and results[-1]['activate'] == 'weather', results
    results = json.loads(ok(desk(shell, SEARCH, 'Query', '(su)', '> lock', 10))[0])
    assert all(r['kind'] != 'web' for r in results), results


def test_search_disabled_answers_not_supported(make_shell):
    shell = make_shell(skin='macos', dconf='[io/github/OlehDzhumyk/Desk/search]\nenabled=false\n')
    assert error_name(desk(shell, SEARCH, 'Query', '(su)', 'lock', 10)) \
        == 'org.freedesktop.DBus.Error.NotSupported'
    ok(desk(shell, SEARCH, 'ClearUsage'))  # always works


# ---- colour picker, screenshots, OCR: exported on deskd, safe without hardware

def test_color_picker_history(make_shell):
    shell = make_shell(skin='default')
    assert ok(desk(shell, COLOR, 'List')) == [[]]
    assert ok(desk(shell, COLOR, 'Contrast')) == [0]
    assert ok(desk(shell, COLOR, 'Format', '(yyys)', 255, 0, 0, 'hex')) == ['#FF0000']
    assert ok(desk(shell, COLOR, 'List')) == [[]], 'Format must not record'

    changed = Signals(shell, COLOR)
    try:
        formatted, hexa, _name = ok(desk(shell, COLOR, 'Record', '(yyy)', 255, 255, 255))
        assert (formatted, hexa) == ('#FFFFFF', '#FFFFFF')
        changed.wait(1, 'ColorPicker.Changed after Record')
        ok(desk(shell, COLOR, 'Record', '(yyy)', 0, 0, 0))
        (items,) = ok(desk(shell, COLOR, 'List'))
        assert [i[0] for i in items] == ['#000000', '#FFFFFF'], items
        assert ok(desk(shell, COLOR, 'Contrast'))[0] == pytest.approx(21.0)
        assert ok(desk(shell, COLOR, 'Remove', '(s)', 'ffffff')) == [True]
        assert ok(desk(shell, COLOR, 'Remove', '(s)', 'ffffff')) == [False]
        ok(desk(shell, COLOR, 'Clear'))
        assert ok(desk(shell, COLOR, 'List')) == [[]]
    finally:
        changed.close()


def test_capture_saves_a_scrubbed_png(make_shell):
    shell = make_shell(skin='default', dconf="[io/github/OlehDzhumyk/Desk/capture]\nsave-folder='~/e2e-shots'\n")
    assert error_name(desk(shell, CAPTURE, 'SaveScreenshot', '(ay)', list(b'not a png'))) \
        == 'org.freedesktop.DBus.Error.InvalidArgs'
    folder = shell.home / 'e2e-shots'
    assert not folder.exists() or not any(folder.iterdir()), 'a refused capture wrote a file'

    data = png(4, 2, (200, 10, 10), text=b'metadata for the scrubber')
    (path,) = ok(desk(shell, CAPTURE, 'SaveScreenshot', '(ay)', list(data)))
    saved = pathlib.Path(path)
    assert saved.parent == folder, path
    assert saved.name.startswith('Screenshot ') and saved.suffix == '.png', path
    body = saved.read_bytes()
    assert body.startswith(b'\x89PNG') and b'tEXt' not in body and b'metadata' not in body

    (again,) = ok(desk(shell, CAPTURE, 'SaveScreenshot', '(ay)', list(data)))
    assert again != path, 'a second capture must not replace the first'


def test_ocr_answers_without_hardware(make_shell):
    shell = make_shell(skin='default')
    (available,) = ok(desk(shell, OCR, 'IsAvailable'))
    (langs,) = ok(desk(shell, OCR, 'AvailableLanguages'))
    assert isinstance(available, bool)
    if not available:
        assert langs == [], langs
    assert error_name(desk(shell, OCR, 'Recognize', '(ayuu)', list(b'not a png'), 4, 2)) \
        == f'{DESK}.Ocr.Error.InvalidImage'
    assert error_name(desk(shell, OCR, 'Recognize', '(ayuu)', list(png(4, 2)), 8, 8)) \
        == f'{DESK}.Ocr.Error.InvalidImage'


def test_foreign_client_is_refused(make_shell):
    """ADR 0008 for the new objects: this test process is not the shell."""
    shell = make_shell(skin='default')
    for (path, iface), method, args, rtype in (
            (LAYOUTS, 'ListLayouts', None, '(s)'),
            (SEARCH, 'Query', GLib.Variant('(su)', ('lock', 10)), '(s)'),
            (COLOR, 'List', None, '(a(sssx))'),
            (OCR, 'IsAvailable', None, '(b)')):
        with pytest.raises(GLib.Error) as e:
            shell.call(DESK, path, iface, method, args, rtype)
        assert 'AccessDenied' in str(e.value), (method, e.value)


# ---- the bridge with the Stage 3/4 features

def test_bridge_features_follow_enable_and_disable(make_shell):
    """A feature whose constructor throws is only logged and left null: all must be up, on every
    skin, and gone after disable (test_lifecycle checks that nothing of them stays behind)."""
    shell = make_shell(skin='windows11')
    assert shell.hook('bridgeFeatures') == dict.fromkeys(FEATURES, True)
    for skin in ('macos', 'default', 'windows11'):
        shell.dconf_write('/io/github/OlehDzhumyk/Desk/skin', f"'{skin}'")
        shell.hook('waitBridge', 'skin', skin)
    shell.set_extension(False)
    assert shell.hook('bridgeFeatures') == dict.fromkeys(FEATURES, False)
    shell.set_extension(True)
    assert shell.hook('bridgeFeatures') == dict.fromkeys(FEATURES, True)
    # deskd still serves the shell after the bridge came back
    assert ok(desk(shell, LAYOUTS, 'ActiveLayout', '(s)', MONITOR)) == ['halves']
