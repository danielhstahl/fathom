"""`ADR-007 §4.7`'s worked example, verbatim.

The comprehension is the row a reviewer is most likely to miscount, and it is counted the way the
`for` loop it replaces would be.
"""


def f(xs, ok):
    if ok and len(xs) > 2:
        return [x for x in xs if x]
    elif ok or xs:
        try:
            return xs[0]
        except IndexError:
            return None
    return "y" if ok else "n"
