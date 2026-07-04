# Derived from flask (src/flask/cli.py) at ab8149664182b662453a563161aa89013c806dc9,
# BSD-3-Clause licensed. Planted-clone eval asset for semdup; not production code.
# Same discovery rules as flask's find_best_app, restructured: dict of
# instances, next(iter()) selection, and continue-based factory loop.
import inspect


def pick_service_object(mod):
    """Locate the service instance a module exports: conventional names
    first, then a unique instance, then zero-argument factories."""
    from .service import Service

    for name in ("app", "application"):
        value = getattr(mod, name, None)
        if isinstance(value, Service):
            return value

    instances = {k: v for k, v in vars(mod).items() if isinstance(v, Service)}
    if len(instances) > 1:
        raise LookupError(
            f"module {mod.__name__!r} exports several Service objects:"
            f" {sorted(instances)}; qualify which one you mean"
        )
    if instances:
        return next(iter(instances.values()))

    for factory_name in ("create_app", "make_app"):
        factory = getattr(mod, factory_name, None)
        if not inspect.isfunction(factory):
            continue
        try:
            produced = factory()
        except TypeError:
            if _factory_call_itself_failed(factory):
                raise LookupError(
                    f"factory {factory_name!r} in {mod.__name__!r} requires"
                    " arguments; pass them explicitly"
                )
            raise
        if isinstance(produced, Service):
            return produced

    raise LookupError(f"no Service instance or factory found in {mod.__name__!r}")
