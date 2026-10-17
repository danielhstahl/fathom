"""Handlers registered through a decorator imported from another module."""

from .util import register


class Handler:
    def handle(self, payload):
        return payload


class Deferred(Handler):
    async def handle(self, payload):
        return payload


@register(name="ping")
def ping(payload):
    return "pong"


@register
def pong(payload):
    return "ping"


HANDLERS = {}
