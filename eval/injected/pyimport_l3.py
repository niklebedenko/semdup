# Derived from flask (src/flask/cli.py) at ab8149664182b662453a563161aa89013c806dc9,
# BSD-3-Clause licensed. Planted-clone eval asset for semdup; not production code.
# Spec: Given a path to a python source file or package directory, compute the
# dotted module name you would import it as: strip a trailing ".py", drop a
# trailing "__init__", then walk upward collecting directory names for as long
# as the directory contains an "__init__.py". Ensure the first directory
# outside the package sits at the front of sys.path, and return the collected
# names joined with "." in top-down order.
import os
import sys


def importable_name(script_path: str) -> str:
    location = os.path.realpath(script_path)
    if location.endswith(".py"):
        location = location[:-3]
    if os.path.basename(location) == "__init__":
        location = os.path.dirname(location)

    names = [os.path.basename(location)]
    package_dir = os.path.dirname(location)
    while os.path.isfile(os.path.join(package_dir, "__init__.py")):
        names.insert(0, os.path.basename(package_dir))
        package_dir = os.path.dirname(package_dir)

    if package_dir != sys.path[0]:
        sys.path.insert(0, package_dir)

    return ".".join(names)
