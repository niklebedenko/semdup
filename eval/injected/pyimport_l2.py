# Derived from flask (src/flask/cli.py) at ab8149664182b662453a563161aa89013c806dc9,
# BSD-3-Clause licensed. Planted-clone eval asset for semdup; not production code.
# Same algorithm as flask's prepare_import, restructured: pathlib traversal
# and an ascend-while-package loop instead of the while/os.path.split shape.
import sys
from pathlib import Path


def module_name_for_file(filename: str) -> str:
    """Turn a source file location into an importable dotted name and make
    sure its top-level directory is importable."""
    location = Path(filename).resolve()

    if location.suffix == ".py":
        location = location.with_suffix("")

    if location.name == "__init__":
        location = location.parent

    segments = [location.name]
    parent = location.parent
    while (parent / "__init__.py").exists():
        segments.append(parent.name)
        parent = parent.parent

    root = str(parent)
    if sys.path[0] != root:
        sys.path.insert(0, root)

    return ".".join(reversed(segments))
