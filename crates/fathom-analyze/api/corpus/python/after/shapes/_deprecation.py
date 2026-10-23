"""The project's own deprecation decorator, so the marker is a real decorator."""


def deprecated(note: str):
    def wrap(func):
        return func

    return wrap
