#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Shared atomic write helper, corrected after a real failure in round 28.

The first `save()` in `f28b.py` did:

    io.open(path, "w", encoding="utf-8").write(text)   # truncates on open
    back = load(path)
    assert label in back                                # raises here

When the assert fired, the file had ALREADY been truncated and rewritten.  So
the read-back assertion detected the mismatch correctly -- L43 working as
intended -- but the mutation was still lost, and worse, the file was briefly on
disk in the intended-but-unasserted state.  A verifier that protects you from a
lost write by destroying the write is not a safety net.

This version is atomic: write to a sibling temp file, read the temp file back,
assert against the temp file, and only then move it over the original with
`os.rename`, which is atomic on POSIX.  A failed assertion leaves the original
untouched.
"""
import io, os


def load(p):
    return io.open(p, encoding="utf-8").read()


def save(p, t, label):
    """Atomically write, verify the write, and only then replace the original."""
    tmp = p + ".tmp-verify"
    try:
        with io.open(tmp, "w", encoding="utf-8") as f:
            f.write(t)
        back = load(tmp)
        assert label in back, "content mismatch in temp write: %r" % (label[:60],)
        # Confirm byte length actually landed, not just the marker.
        assert len(back) == len(t), (
            "length mismatch: temp=%d expected=%d" % (len(back), len(t)))
        os.rename(tmp, p)
    except Exception:
        if os.path.exists(tmp):
            os.unlink(tmp)
        raise
    # Final confirmation against the real file, post-rename.
    final = load(p)
    assert label in final, "post-rename verification failed: %r" % (label[:60],)
    print("   [atomically written and verified] %s" % label[:60])
