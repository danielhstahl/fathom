"""
Timeline (Netscape) format ingestion.

This module handles parsing of the classic Netscape/Firefox "Bookmarks" HTML
export format (DOCTYPE NETSCAPE-Bookmark-file-1), including the extended
metadata fields used by the Firefox "Timeline" export (CC, Tags, Description).
"""
import copy

from bs4 import BeautifulSoup

from ._base import (
    Reader,
    ensure_time,
    ensure_url,
    format_time,
    extract_tags,
    extract_keyword,
)


def process_bookmark(soupy_tag):
    """Process a single <DT> bookmark item and return its normalized row."""
    row = {}
    ensure_url(soupy_tag, row)
    extract_tags(soupy_tag, row)
    ensure_time(soupy_tag, row)
    extract_keyword(soupy_tag, row)
    return row


def extract_records(html_content):
    """
    Extract structured records from a Timeline (Netscape) HTML export.

    A record starts at a `<DT>` tag; the `<DD>` block that follows it is its
    body and every `<META>` up to the next `<DT>` describes it.

    Args:
        html_content: the HTML export, either `str` or `bytes`.

    Returns:
        list[dict]: one dict per record, of the form
        `{"attrs": {...}, "body": "..."}`, where `attrs` maps the lower-cased
        `NAME` of each describing `<META>` tag to the *list* of its `CONTENT`
        values -- so multi-valued fields such as `CC` keep every occurrence --
        and `body` holds the markup of the `<DD>` block that describes the
        record.  Returns `[]` when the input holds no records, never `None`.
    """
    if not html_content:
        return []
    if isinstance(html_content, str):
        html_content = html_content.encode("utf-8", "replace")
    elif not isinstance(html_content, (bytes, bytearray)):
        # Anything that is neither text nor bytes cannot be an export.
        return []

    reader = Reader()
    reader.feed(html_content)
    reader.close()
    soup = BeautifulSoup(reader.getvalue(), "html.parser")

    records = []
    # Records are collected in document order rather than by iterating a
    # container's children: depending on the parser the `<DT>` tags of the
    # export end up directly under the document root instead of inside
    # `<BODY>` -- walking `<BODY>` alone silently drops every record -- and
    # exports that leave `<DT>`/`<DD>` unclosed nest a record inside the one
    # before it.
    for tag in soup.find_all(["dt", "dd", "meta"]):
        name = tag.name.lower()
        if name == "dt":
            records.append(
                {
                    "attrs": {},
                    "body": "",
                    "_dd": [],
                    "_data": _has_content(tag),
                }
            )
        elif not records:
            # Markup before the first `<DT>` describes no record.
            continue
        elif name == "dd":
            # The first `<DD>` after a `<DT>` is that record's body block;
            # any further `<DD>` describes the next entry of the block, not
            # this record.
            if not records[-1]["_dd"]:
                records[-1]["_dd"].append(tag)
                records[-1]["_data"] = True
        elif name == "meta":
            if _add_attribute(records[-1]["attrs"], tag):
                records[-1]["_data"] = True

    result = []
    for record in records:
        has_data = record.pop("_data")
        record["body"] = "".join(_markup(dd) for dd in record.pop("_dd"))
        # A Timeline export uses bare `<DT>` tags as separators between
        # sections; they describe nothing and do not make a record.
        if has_data or record["attrs"] or record["body"]:
            result.append(record)
    return result


def _has_content(soupy_tag):
    """
    Tell whether a `<DT>` tag carries data of its own.

    `<DT>`/`<DD>` tags nested inside it are other records (an artefact of the
    export leaving tags unclosed) and do not count.
    """
    for child in soupy_tag.children:
        name = (getattr(child, "name", None) or "").lower()
        if name in ("dt", "dd"):
            continue
        if name:
            return True
        if str(child).strip():
            return True
    return False


def _add_attribute(attrs, soupy_tag):
    """
    Merge a `<META>` tag describing a record into its attribute dict.

    Returns whether the tag contributed an attribute.
    """
    attribute = soupy_tag.get("name")
    value = soupy_tag.get("content")
    if not attribute or value is None:
        return False
    attribute = attribute.lower()
    if isinstance(value, (list, tuple)):
        # bs4 hands back a list for multi-valued attributes; keep every value
        # but never record an empty one.
        values = [item for item in value if item]
    else:
        values = [value]
    if not values:
        return False
    attrs.setdefault(attribute, []).extend(values)
    return True


def _markup(soupy_tag):
    """
    Serialize the markup of a record body tag.

    Exports that leave `<DT>`/`<DD>` unclosed make the parser nest the records
    that follow inside the body of the current one; everything from such a
    nested record tag on belongs to another record and is trimmed here.
    """
    if soupy_tag.find("dt") is None and soupy_tag.find("dd") is None:
        return str(soupy_tag)

    fragment = copy.deepcopy(soupy_tag)
    dropping = False
    for node in list(fragment.descendants):
        if dropping or (getattr(node, "name", None) or "").lower() in ("dt", "dd"):
            dropping = True
            node.extract()
    return str(fragment)
