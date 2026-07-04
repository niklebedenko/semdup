# Derived from flask (src/flask/cli.py) at ab8149664182b662453a563161aa89013c806dc9,
# BSD-3-Clause licensed. Planted-clone eval asset for semdup; not production code.
import os
import sys


def resolve_module_path(target: str) -> str:
    """Given a file location, work out the dotted module path, put the
    containing directory on the import search path, and return the module
    name that should be imported.
    """
    target = os.path.realpath(target)

    stem, suffix = os.path.splitext(target)
    if suffix == ".py":
        target = stem

    if os.path.basename(target) == "__init__":
        target = os.path.dirname(target)

    dotted_parts = []

    # climb until we are outside the package (directory without __init__.py)
    while True:
        target, segment = os.path.split(target)
        dotted_parts.append(segment)

        if not os.path.exists(os.path.join(target, "__init__.py")):
            break

    if sys.path[0] != target:
        sys.path.insert(0, target)

    return ".".join(dotted_parts[::-1])
