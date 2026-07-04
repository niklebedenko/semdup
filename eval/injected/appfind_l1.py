# Derived from flask (src/flask/cli.py) at ab8149664182b662453a563161aa89013c806dc9,
# BSD-3-Clause licensed. Planted-clone eval asset for semdup; not production code.
import inspect


def locate_primary_app(candidate_module):
    """Given an imported module, pick the most plausible application object
    inside it, or raise an exception if none can be determined.
    """
    from . import Flask

    # Try the conventional attribute names before anything else.
    for candidate_name in ("app", "application"):
        instance = getattr(candidate_module, candidate_name, None)

        if isinstance(instance, Flask):
            return instance

    # Otherwise look for exactly one Flask object among the attributes.
    found = [v for v in candidate_module.__dict__.values() if isinstance(v, Flask)]

    if len(found) == 1:
        return found[0]
    elif len(found) > 1:
        raise NoAppException(
            "Detected multiple Flask applications in module"
            f" '{candidate_module.__name__}'. Use"
            f" '{candidate_module.__name__}:name' to specify the correct one."
        )

    # Look for application factory callables.
    for candidate_name in ("create_app", "make_app"):
        factory = getattr(candidate_module, candidate_name, None)

        if inspect.isfunction(factory):
            try:
                instance = factory()

                if isinstance(instance, Flask):
                    return instance
            except TypeError as err:
                if not _called_with_wrong_args(factory):
                    raise

                raise NoAppException(
                    f"Detected factory '{candidate_name}' in module"
                    f" '{candidate_module.__name__}', but could not call it"
                    f" without arguments. Use"
                    f" '{candidate_module.__name__}:{candidate_name}(args)'"
                    " to specify arguments."
                ) from err

    raise NoAppException(
        "Failed to find Flask application or factory in module"
        f" '{candidate_module.__name__}'. Use"
        f" '{candidate_module.__name__}:name' to specify one."
    )
