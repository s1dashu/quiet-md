#!/usr/bin/env python3
"""Fix a sample library's top-level order for the web demo, before it is first
opened: Home and the sidebar list folders newest first, so the named ones get
modification times an hour apart from a day ago, the rest follow by name. The
first open (web/README.md) keeps these times in .quiet/meta, which the bundle
carries; shared-mock/guides/shots/shots.sh orders the README's shot the same way.

usage: web/curate.py <library dir> "<top name>" "<top name>" ...
"""
import os, sys, time
root, tops = sys.argv[1], sys.argv[2:]
assert not os.path.isdir(os.path.join(root, '.quiet', 'meta')), 'curate before the first open'
order = tops + sorted(f for f in os.listdir(root) if f not in tops and not f.startswith('.') and os.path.isdir(os.path.join(root, f)))
t = time.time() - 86400
for i, f in enumerate(order): os.utime(os.path.join(root, f), (t - i * 3600,) * 2)
print(f'{root}: {len(order)} folders, {len(tops)} placed')
