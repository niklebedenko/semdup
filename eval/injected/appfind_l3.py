# Derived from flask (src/flask/cli.py) at ab8149664182b662453a563161aa89013c806dc9,
# BSD-3-Clause licensed. Planted-clone eval asset for semdup; not production code.
# Spec: Given an imported module, return "the application object" using these
# rules in order: (1) if an attribute named "app" or "application" is an App
# instance, return it; (2) if exactly one module attribute is an App instance,
# return it — more than one is an error; (3) for each of "create_app" and
# "make_app" that is a plain function, call it with no arguments and return
# the result if it is an App; (4) otherwise raise.
import inspect


def resolve_application(module):
    preferred = [module.__dict__.get("app"), module.__dict__.get("application")]
    for obj in preferred:
        if isinstance(obj, App):
            return obj

    every_app = [v for v in module.__dict__.values() if isinstance(v, App)]
    if len(every_app) > 1:
        raise RuntimeError("ambiguous: more than one App in " + module.__name__)
    if every_app:
        return every_app[0]

    for maker in ("create_app", "make_app"):
        candidate = module.__dict__.get(maker)
        if candidate is None or not inspect.isfunction(candidate):
            continue
        made = candidate()
        if isinstance(made, App):
            return made

    raise RuntimeError("could not resolve an App from " + module.__name__)
