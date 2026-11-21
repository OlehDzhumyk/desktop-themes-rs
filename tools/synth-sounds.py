#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Synthesize look-alike (original, not copied) UI sound themes: macos-like and windows11-like.
Pure numpy/scipy additive+noise synthesis -> 48 kHz stereo Vorbis (.oga), ~-16 LUFS, peak <= -1 dBFS.
Licence of output: CC0 (original synthesis, no samples used).
Usage: synth-sounds.py <output-sounds-dir>   (writes <dir>/macos-like and <dir>/windows11-like)
"""
import os, sys, json
import numpy as np, soundfile as sf
from scipy import signal

SR = 48000
TARGET_LUFS = -16.0
PEAK_CEIL_DB = -1.0
rng = np.random.default_rng(20261121)

def T(d): return np.arange(int(d * SR)) / SR
def nf(name):
    base = {'C': 0, 'D': 2, 'E': 4, 'F': 5, 'G': 7, 'A': 9, 'B': 11}[name[0]]
    i = 1
    if name[1] == '#': base += 1; i = 2
    elif name[1] == 'b': base -= 1; i = 2
    return 440.0 * 2 ** (((int(name[i:]) + 1) * 12 + base - 69) / 12)

def canvas(d): return np.zeros((int(d * SR), 2))
def place(buf, x, at, pan=0.0, gain=1.0):
    s = int(at * SR)
    if s >= len(buf): return
    e = min(len(buf), s + len(x)); x = x[:e - s]
    a = (pan + 1) * np.pi / 4
    buf[s:e, 0] += x * np.cos(a) * gain; buf[s:e, 1] += x * np.sin(a) * gain

def attack(x, ms=1.5):
    n = min(len(x), int(SR * ms / 1000))
    x[:n] *= 0.5 - 0.5 * np.cos(np.pi * np.arange(n) / n)
    return x

def glass(f, d=1.0, tail=0.35, bright=1.0):
    """Glassy struck-bar timbre: inharmonic partials, slight detuned twin for shimmer."""
    t = T(d); x = np.zeros_like(t)
    for r, a, k in [(1, 1.0, 1.0), (2.76, 0.40 * bright, 0.55), (5.40, 0.16 * bright, 0.30), (8.93, 0.06 * bright, 0.18)]:
        for det, g in ((1.0, 1.0), (1.0018, 0.6)):
            x += g * a * np.sin(2 * np.pi * f * r * det * t + rng.uniform(0, 6.28)) * np.exp(-t / (tail * k))
    return attack(x / 1.6, 1.2)

def marimba(f, d=0.8, tail=0.30, warm=1.0):
    """Round mallet-bar timbre: 1 : ~3.9 : ~9.2 partials, fast decay of upper partials, soft thump."""
    t = T(d); x = np.zeros_like(t)
    for r, a, k in [(1, 1.0, 1.0), (3.93, 0.28 / warm, 0.22), (9.2, 0.07 / warm, 0.08), (0.5, 0.18 * warm, 0.9)]:
        x += a * np.sin(2 * np.pi * f * r * t) * np.exp(-t / (tail * k))
    thump = noise_band(0.02, 300, 1800, 0.004) * 0.25
    x[:len(thump)] += thump
    return attack(x, 2.0)

def bellsoft(f, d=1.2, tail=0.5):
    """Sine-ish soft bell (for Windows login bloom)."""
    t = T(d)
    x = np.sin(2 * np.pi * f * t) * np.exp(-t / tail) + 0.25 * np.sin(2 * np.pi * f * 2.0 * t) * np.exp(-t / (tail * .5))
    return attack(x, 4)

def noise_band(d, lo, hi, tau, hard=False):
    n = rng.standard_normal(int(d * SR))
    sos = signal.butter(2, [lo, min(hi, SR / 2 - 100)], 'band', fs=SR, output='sos')
    x = signal.sosfilt(sos, n)
    t = T(d)
    return x / (np.abs(x).max() + 1e-9) * np.exp(-t / tau)

def pad(freqs, d, att=0.05, tau=1.2, cutoff=2600, detune_cents=(-5, 0, 6)):
    t = T(d); x = np.zeros_like(t)
    for f in freqs:
        for c in detune_cents:
            ff = f * 2 ** (c / 1200)
            n = 1
            while ff * n < 5000:
                x += np.sin(2 * np.pi * ff * n * t + rng.uniform(0, 6.28)) / n * (1 / len(detune_cents)) / np.sqrt(len(freqs))
                n += 1
    sos = signal.butter(2, cutoff, 'low', fs=SR, output='sos')
    x = signal.sosfilt(sos, x)
    env = np.minimum(1, t / att) ** 1.5 * np.exp(-t / tau)
    return x * env

def sweep_tone(f0, f1, d, tau):
    t = T(d); ph = 2 * np.pi * np.cumsum(f0 * (f1 / f0) ** (t / d)) / SR
    return attack(np.sin(ph) * np.exp(-t / tau), 2)

def thud(f, d=0.5, tau=0.12):
    t = T(d)
    return attack((np.sin(2 * np.pi * f * t) + 0.3 * np.sin(2 * np.pi * f * 2.01 * t)) * np.exp(-t / tau), 4)

def click(bright=1.0):
    x = noise_band(0.04, 2200, 9000, 0.005) * 0.9 * bright
    th = thud(190, 0.06, 0.012)
    y = np.zeros(int(0.06 * SR)); y[:len(x)] += x; y += th[:len(y)] * 0.7
    return y

def crackle(d, density=900, lo=1500, hi=8000, shape=1.0):
    n = int(d * SR); t = T(d)
    p = (density / SR) * np.sin(np.pi * np.clip(t / d, 0, 1)) ** shape
    imp = (rng.random(n) < p) * rng.uniform(0.2, 1.0, n) * rng.choice([-1, 1], n)
    k = noise_band(0.004, lo, hi, 0.0009)
    x = signal.fftconvolve(imp, k)[:n]
    wash = noise_band(d, lo, hi / 1.5, 1e9) * 0.08 * np.sin(np.pi * np.clip(t / d, 0, 1))
    return x / (np.abs(x).max() + 1e-9) + wash

def reverb(buf, rt60, mix, predelay=0.012, lp=6000, size_pad=None):
    n = int(rt60 * 1.3 * SR); t = np.arange(n) / SR
    out = buf.copy()
    pad_n = int(rt60 * 1.0 * SR)
    out = np.vstack([out, np.zeros((pad_n, 2))])
    sos = signal.butter(2, lp, 'low', fs=SR, output='sos')
    for ch in range(2):
        ir = rng.standard_normal(n) * np.exp(-6.91 * t / rt60)
        ir = signal.sosfilt(sos, ir)
        ir[:int(0.003 * SR)] *= np.linspace(0, 1, int(0.003 * SR))
        ir /= np.sqrt((ir ** 2).sum())
        pd = int(predelay * SR)
        wet = signal.fftconvolve(out[:len(buf), ch] if False else np.pad(buf[:, ch], (0, pad_n)), ir)[:len(out)]
        wet = np.concatenate([np.zeros(pd), wet])[:len(out)]
        out[:, ch] = out[:, ch] + wet * mix * 2.2
    return out

# ---------- loudness (ITU-R BS.1770-4, 48 kHz) ----------
_S1 = (np.array([1.53512485958697, -2.69169618940638, 1.19839281085285]), np.array([1.0, -1.69065929318241, 0.73248077421585]))
_S2 = (np.array([1.0, -2.0, 1.0]), np.array([1.0, -1.99004745483398, 0.99007225036621]))
def lufs(x):
    pad = max(0, int(0.8 * SR) - len(x))
    x = np.vstack([x, np.zeros((pad, 2))])
    y = np.stack([signal.lfilter(*_S2, signal.lfilter(*_S1, x[:, c])) for c in range(2)], 1)
    blk, hop = int(0.4 * SR), int(0.1 * SR)
    z = np.array([ (y[i:i + blk] ** 2).mean(0).sum() for i in range(0, len(y) - blk + 1, hop)])
    l = -0.691 + 10 * np.log10(z + 1e-12)
    g = z[l > -70]
    if len(g) == 0: return -70.0
    rel = -0.691 + 10 * np.log10(g.mean()) - 10
    g2 = z[l > max(-70, rel)]
    return -0.691 + 10 * np.log10(g2.mean())

def finish(buf, tail_db=-62):
    x = buf.copy()
    x -= x.mean(0)
    sos = signal.butter(2, 25, 'high', fs=SR, output='sos')
    x = signal.sosfilt(sos, x, axis=0)
    env = np.abs(x).max(1); pk = env.max()
    idx = np.where(env > pk * 10 ** (tail_db / 20))[0]
    end = min(len(x), idx[-1] + int(0.05 * SR)) if len(idx) else len(x)
    x = x[:end]
    nfo = min(len(x), int(0.04 * SR)); x[-nfo:] *= np.linspace(1, 0, nfo)[:, None] ** 2
    nfi = int(0.0008 * SR); x[:nfi] *= np.linspace(0, 1, nfi)[:, None]
    ceil = 10 ** (PEAK_CEIL_DB / 20)
    base = x.copy()
    for drive in (1.0, 1.6, 2.4, 3.5, 5.0):   # soft-saturate transient-heavy sounds only as far as needed
        x = np.tanh(base * drive / np.abs(base).max()) if drive > 1.0 else base
        g = 10 ** ((TARGET_LUFS - lufs(x)) / 20)
        if np.abs(x).max() * g <= ceil: break
    x = x * g
    if np.abs(x).max() > ceil: x *= ceil / np.abs(x).max()
    return x

# ======================= macOS-like =======================
def mac():
    S = {}
    def seq(notes, dur, rt=0.9, mix=0.28, tail=0.30, gap=0.09, bright=1.0, spread=0.25):
        b = canvas(dur)
        for i, (n, ) in enumerate([(n,) for n in notes]):
            place(b, glass(nf(n), 1.4, tail, bright), i * gap, pan=(i - (len(notes) - 1) / 2) * spread * 0.6)
        return reverb(b, rt, mix)
    tink = lambda: reverb(_p(glass(nf('E6'), 0.5, 0.11, 0.8), 0.55), 0.5, 0.2)
    _t = tink(); S['bell'] = S['bell-terminal'] = S['bell-window-system'] = _t
    S['message-new-instant'] = seq(['C6', 'G6'], 1.6, 1.1, 0.30, 0.34, 0.11)
    S['message'] = seq(['D6'], 1.4, 0.9, 0.28, 0.32)
    S['complete'] = seq(['C6', 'E6', 'G6'], 1.9, 1.2, 0.30, 0.34, 0.10)
    S['dialog-information'] = seq(['A5'], 1.0, 0.7, 0.25, 0.22)
    def basso(dur, hits):
        b = canvas(dur)
        for i, (f0, f1, g) in enumerate(hits):
            place(b, sweep_tone(f0, f1, 0.7, 0.16) * g + 0.35 * g * sweep_tone(f0 * 2, f1 * 2, 0.7, 0.08), i * 0.17)
        return reverb(b, 0.6, 0.18, lp=3500)
    S['dialog-warning'] = basso(1.0, [(262, 196, 1.0)])
    S['dialog-error'] = basso(1.2, [(247, 175, 1.0), (233, 165, 0.9)])
    def shutter(dur=0.45):
        b = canvas(dur); place(b, click(1.0), 0.0, -0.1); place(b, click(0.8), 0.085, 0.1, 0.8)
        return reverb(b, 0.25, 0.10, lp=7000)
    _s = shutter(); S['screen-capture'] = S['camera-shutter'] = _s
    tr = canvas(1.3); place(tr, crackle(0.8, 1400, 1200, 8500), 0.0); place(tr, thud(120, 0.2, 0.05), 0.78, gain=0.5)
    S['trash-empty'] = reverb(tr, 0.3, 0.08)
    S['audio-volume-change'] = reverb(_p(glass(nf('A5'), 0.3, 0.07, 0.6), 0.35), 0.3, 0.12)
    S['device-added'] = seq(['G5', 'D6'], 1.2, 0.8, 0.26, 0.20, 0.10)
    S['device-removed'] = seq(['D6', 'G5'], 1.2, 0.8, 0.26, 0.20, 0.10)
    def plug(up):
        b = canvas(1.2); place(b, click(0.7), 0, gain=0.6)
        place(b, glass(nf('E6' if up else 'A5'), 1.0, 0.25 if up else 0.18), 0.03, gain=0.9)
        if not up: place(b, glass(nf('E5'), 0.8, 0.14), 0.12, gain=0.7)
        return reverb(b, 0.6, 0.2)
    S['power-plug'] = plug(True); S['power-unplug'] = plug(False)
    # login: warm wide chord (F#-based, add9), long reverb + soft sparkle on top
    chord = [nf(n) for n in ['F#2', 'C#3', 'F#3', 'A#3', 'C#4', 'G#4', 'F#4']]
    b = canvas(5.0)
    place(b, pad(chord, 4.6, 0.09, 1.1, 2200), 0.0, gain=0.9)
    place(b, pad([nf('F#2'), nf('F#1')], 4.0, 0.12, 0.9, 400), 0.0, gain=0.6)
    place(b, glass(nf('C#6'), 2.5, 0.8, 0.6), 0.30, 0.25, 0.20)
    place(b, glass(nf('A#6'), 2.0, 0.6, 0.5), 0.45, -0.3, 0.12)
    S['desktop-login'] = reverb(b, 2.4, 0.35, lp=4500)
    b = canvas(3.2)
    place(b, pad([nf(n) for n in ['D3', 'A3', 'D4', 'F#4', 'E4']], 2.2, 0.06, 0.55, 1800), 0.0, gain=0.8)
    place(b, glass(nf('A5'), 1.2, 0.35, 0.6), 0.0, -0.2, 0.3); place(b, glass(nf('D5'), 1.4, 0.4, 0.6), 0.22, 0.2, 0.3)
    S['desktop-logout'] = reverb(b, 1.6, 0.3, lp=4000)
    S['service-login'] = seq(['E6', 'B6'], 1.3, 0.8, 0.25, 0.22, 0.08)
    S['service-logout'] = seq(['B6', 'E6'], 1.3, 0.8, 0.25, 0.22, 0.08)
    S['window-attention'] = seq(['G6', 'E6', 'G6'], 1.4, 0.9, 0.26, 0.20, 0.13)
    S['alarm-clock-elapsed'] = seq(['E6', 'G#6', 'B6', 'E6', 'G#6', 'B6'], 2.0, 1.0, 0.25, 0.25, 0.20)
    S['battery-low'] = seq(['E6', 'C6'], 1.3, 0.8, 0.25, 0.25, 0.14)
    S['battery-caution'] = seq(['E6', 'C6', 'A5'], 1.5, 0.8, 0.25, 0.25, 0.14)
    S['network-connectivity-established'] = seq(['A5', 'E6'], 1.2, 0.8, 0.25, 0.20, 0.09)
    S['network-connectivity-lost'] = seq(['E6', 'A5'], 1.2, 0.8, 0.25, 0.20, 0.09)
    return S

def _p(x, d):
    b = canvas(d); place(b, x, 0); return b

# ======================= Windows 11-like =======================
def win():
    S = {}
    def seq(notes, dur, rt=0.6, mix=0.14, tail=0.28, gap=0.12, warm=1.0, spread=0.2, vel=None):
        b = canvas(dur)
        for i, n in enumerate(notes):
            v = 1.0 if vel is None else vel[i]
            place(b, marimba(nf(n), 1.0, tail, warm), i * gap, pan=(i - (len(notes) - 1) / 2) * spread * 0.5, gain=v)
        return reverb(b, rt, mix, lp=5000)
    one = lambda n='A5', tail=0.22: seq([n], 0.7, 0.4, 0.12, tail)
    _o = one(); S['bell'] = S['bell-terminal'] = S['bell-window-system'] = _o
    S['message-new-instant'] = seq(['E5', 'B5'], 1.3, 0.7, 0.16, 0.34, 0.16)
    S['message'] = seq(['G5'], 1.0, 0.6, 0.14, 0.30)
    S['complete'] = seq(['C5', 'G5', 'C6'], 1.5, 0.8, 0.16, 0.30, 0.11)
    S['dialog-information'] = seq(['D6'], 0.9, 0.5, 0.12, 0.2)
    S['dialog-warning'] = seq(['B4', 'F#4'], 1.1, 0.5, 0.12, 0.25, 0.13, warm=1.3)
    S['dialog-error'] = seq(['E4', 'B3', 'E3'], 1.4, 0.6, 0.14, 0.28, 0.14, warm=1.5)
    def shutter():
        b = canvas(0.4); place(b, click(0.8), 0.0); place(b, marimba(nf('A6'), 0.2, 0.05), 0.0, gain=0.15)
        return reverb(b, 0.2, 0.08, lp=6500)
    _s = shutter(); S['screen-capture'] = S['camera-shutter'] = _s
    tr = canvas(1.0); place(tr, crackle(0.55, 900, 900, 6000, 1.5), 0.0, gain=0.9); place(tr, marimba(nf('A4'), 0.4, 0.06), 0.52, gain=0.25)
    S['trash-empty'] = reverb(tr, 0.25, 0.06)
    S['audio-volume-change'] = seq(['C6'], 0.5, 0.3, 0.10, 0.10)
    S['device-added'] = seq(['E5', 'A5', 'E6'], 1.1, 0.5, 0.14, 0.2, 0.07)
    S['device-removed'] = seq(['E6', 'A5', 'E5'], 1.1, 0.5, 0.14, 0.2, 0.07)
    S['power-plug'] = seq(['A5', 'E6'], 0.9, 0.4, 0.12, 0.2, 0.07)
    S['power-unplug'] = seq(['E6', 'A5'], 0.9, 0.4, 0.12, 0.2, 0.07)
    # login: soft rising arpeggio + bell bloom
    b = canvas(4.0)
    notes = ['C5', 'E5', 'G5', 'B5', 'D6', 'G6']
    for i, n in enumerate(notes):
        place(b, marimba(nf(n), 1.6, 0.45 + 0.06 * i, 0.9), i * 0.13, pan=-0.35 + 0.14 * i, gain=0.8 + 0.04 * i)
    place(b, bellsoft(nf('G6'), 2.5, 0.8), 0.65, 0.15, 0.18)
    place(b, pad([nf('C4'), nf('G4'), nf('E5'), nf('B4')], 3.0, 0.25, 0.9, 1800, (-4, 4)), 0.1, gain=0.25)
    S['desktop-login'] = reverb(b, 1.7, 0.24, lp=5000)
    b = canvas(3.0)
    for i, n in enumerate(['G5', 'E5', 'C5', 'G4']):
        place(b, marimba(nf(n), 1.4, 0.40, 1.0), i * 0.14, pan=0.3 - 0.2 * i, gain=1.0 - 0.07 * i)
    place(b, bellsoft(nf('C5'), 1.8, 0.5), 0.4, 0, 0.15)
    S['desktop-logout'] = reverb(b, 1.3, 0.22)
    S['service-login'] = seq(['G5', 'D6'], 1.0, 0.5, 0.12, 0.25, 0.10)
    S['service-logout'] = seq(['D6', 'G5'], 1.0, 0.5, 0.12, 0.25, 0.10)
    S['window-attention'] = seq(['E6', 'E6'], 0.9, 0.4, 0.12, 0.15, 0.14, vel=[1.0, 0.7])
    S['alarm-clock-elapsed'] = seq(['A5', 'E6', 'A5', 'E6', 'A5', 'E6'], 1.9, 0.7, 0.14, 0.22, 0.2)
    S['battery-low'] = seq(['E5', 'B4'], 1.0, 0.5, 0.12, 0.25, 0.16, warm=1.2)
    S['battery-caution'] = seq(['E5', 'B4', 'E4'], 1.2, 0.5, 0.12, 0.25, 0.16, warm=1.3)
    S['network-connectivity-established'] = seq(['D5', 'A5'], 1.0, 0.5, 0.12, 0.22, 0.08)
    S['network-connectivity-lost'] = seq(['A5', 'D5'], 1.0, 0.5, 0.12, 0.22, 0.08)
    return S

THEMES = {
    'macos-like': ('macOS-like', 'Original synthesized glassy/soft UI sounds in the spirit of macOS (no Apple audio)', mac),
    'windows11-like': ('Windows11-like', 'Original synthesized round marimba chimes in the spirit of Windows 11 (no Microsoft audio)', win),
}

def main(out):
    report = {}
    for dirname, (title, comment, fn) in THEMES.items():
        d = os.path.join(out, dirname, 'stereo'); os.makedirs(d, exist_ok=True)
        with open(os.path.join(out, dirname, 'index.theme'), 'w') as f:
            f.write(f"[Sound Theme]\nName={title}\nComment={comment}\nInherits=freedesktop\nDirectories=stereo\n\n[stereo]\nOutputProfile=stereo\n")
        rep = {}
        for name, buf in fn().items():
            x = finish(buf)
            sf.write(os.path.join(d, name + '.oga'), x.astype(np.float32), SR, format='OGG', subtype='VORBIS')
            rep[name] = (len(x) / SR, 20 * np.log10(np.abs(x).max()), lufs(x))
        report[dirname] = rep
    json.dump(report, sys.stdout, indent=1)

if __name__ == '__main__':
    main(sys.argv[1])
