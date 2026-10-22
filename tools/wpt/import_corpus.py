#!/usr/bin/env python3
"""Reproduce the complete pinned flexbox reftest inventory without running Mg.

python3 tools/wpt/import_corpus.py --output-dir tmp/wpt-import-UNIQUE
python3 tools/wpt/import_corpus.py --source-checkout /path/to/wpt --output-dir tmp/wpt-import-UNIQUE

The online provider uses configured gh credentials, pinned Git trees and verified
Git blobs. The offline provider reads committed objects, never checkout bytes.
Output must be new. No browser, renderer, outcome file or expectation is read.
"""
import argparse
import base64
import codecs
from collections import deque
from dataclasses import dataclass, field
import hashlib
from html.parser import HTMLParser
import json
import os
from pathlib import Path, PurePosixPath
import re
import stat
import subprocess
from urllib.parse import urljoin, urlsplit
import xml.etree.ElementTree as ET

REVISION = "84daed4ae966d9624bed22f43587c79283ad1fea"
PREFIX = "css/css-flexbox"
FLEX_TREE = "9ad051e4d7244fb68fc346148daa8e369ebb4844"
REPOSITORY = "https://github.com/web-platform-tests/wpt"
API_REPOSITORY = "repos/web-platform-tests/wpt"
MAX_FILES = 4096
MAX_BYTES = 10 * 1024 * 1024
LEGACY_CORPUS_SHA256 = "a7c74dc8f01eebd8f14ad8851e4d8c3bad3d2ac97764abd2e370602e40b1bbdd"
LEGACY_CORPUS_BYTES = 18464
MARKUP_SUFFIXES = {".html", ".htm", ".xht", ".xhtml", ".xml", ".svg"}
HTML_NAMESPACE = "http://www.w3.org/1999/xhtml"
SVG_NAMESPACE = "http://www.w3.org/2000/svg"
MATH_NAMESPACE = "http://www.w3.org/1998/Math/MathML"
REVIEWED_MISSING_DEPENDENCIES = {
    ("css/css-flexbox/percentage-descendant-of-anonymous-flex-item.html", "resources/flexbox.css",
     "css/css-flexbox/resources/flexbox.css"),
    ("css/css-flexbox/reference/flexbox-with-multi-column-property-ref.html", "resources/flexbox.css",
     "css/css-flexbox/reference/resources/flexbox.css"),
}
HERE = Path(__file__).resolve().parent


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def git_blob_sha1(data):
    return hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest()


def read_regular(path, size):
    """Admit a nonsymlink regular file by exact size before bounded body read."""
    flags = os.O_RDONLY | os.O_NONBLOCK | getattr(os, "O_NOFOLLOW", 0)
    descriptor = os.open(path, flags)
    try:
        before = os.fstat(descriptor)
        if not stat.S_ISREG(before.st_mode) or before.st_size != size:
            raise ValueError(f"Input has invalid regular-file type/size: {path}")
        with os.fdopen(descriptor, "rb", closefd=False) as stream:
            data = stream.read(size + 1)
        after = os.fstat(descriptor)
        if len(data) != size or after.st_size != size or after.st_mtime_ns != before.st_mtime_ns:
            raise ValueError(f"Input changed during bounded read: {path}")
        return data
    finally:
        os.close(descriptor)


def canonical_path(value, url_safe=True):
    if (not isinstance(value, str) or not value or value.startswith("/") or
            any(character in value for character in ("\\%?#\0" if url_safe else "\\\0")) or
            any(ord(character) < 32 for character in value) or
            any(part in ("", ".", "..") for part in value.split("/"))):
        raise ValueError(f"Noncanonical upstream path: {value!r}")
    return value


def resolved_edge(path, href, relation=None):
    """Retain the parsed original href, including unsupported protocols."""
    edge = {"href": href, "path": None}
    if relation is not None:
        edge = {"relation": relation, **edge}
    reason = None
    try:
        value = href.strip()
        if not value or any(ord(char) < 32 for char in value) or "\\" in value:
            reason = ("unsafe-reference", "Empty, control-bearing or backslash reference")
        elif "{{" in value or "}}" in value:
            reason = ("dynamic-reference", "WPT server substitution requires a separate protocol")
        else:
            url = urlsplit(urljoin("http://wpt.invalid/" + path, value))
            if url.scheme != "http" or url.netloc != "wpt.invalid":
                reason = ("nonlocal-reference", "Nonlocal or embedded URL is not fetched")
            elif url.query:
                reason = ("query-reference", "Query-bearing reference requires a separate protocol")
                edge["source_path"] = canonical_path(url.path.lstrip("/"))
            elif url.fragment:
                reason = ("fragment-reference", "Fragment-bearing reference requires a separate protocol")
                edge["source_path"] = canonical_path(url.path.lstrip("/"))
            else:
                edge["path"] = canonical_path(url.path.lstrip("/"))
    except (ValueError, TypeError) as error:
        reason = ("unsafe-reference", str(error))
    if reason:
        edge["unsupported"] = {"code": reason[0], "reason": reason[1]}
    return edge


def _css_unescape(value):
    def replace(match):
        token = match.group(1)
        if re.fullmatch(r"[0-9a-fA-F]{1,6}\s?", token):
            number = int(token.strip(), 16)
            return chr(number) if 0 < number <= 0x10FFFF and not 0xD800 <= number <= 0xDFFF else "\ufffd"
        return token
    return re.sub(r"\\([0-9a-fA-F]{1,6}(?:\s)?|[^\r\n])", replace, value)


def css_resources(text):
    """Read URL/import tokens in CSS context, excluding comments and strings."""
    resources = []
    index = 0
    escape = r"\\(?:[0-9a-fA-F]{1,6}\s?|[^\r\n])"
    identifier = re.compile(r"(?:[-_a-zA-Z]|" + escape + r")(?:[-_a-zA-Z0-9]|" + escape + r")*")

    def space(position):
        while position < len(text):
            if text[position].isspace():
                position += 1
            elif text.startswith("/*", position):
                end = text.find("*/", position + 2)
                position = len(text) if end < 0 else end + 2
            else:
                break
        return position

    def quoted(position):
        quote = text[position]
        begin = position + 1
        position = begin
        while position < len(text):
            if text[position] == "\\":
                position += 2
            elif text[position] == quote:
                return _css_unescape(text[begin:position]), position + 1
            else:
                position += 1
        return _css_unescape(text[begin:]), len(text)

    while index < len(text):
        next_index = space(index)
        if next_index != index:
            index = next_index
            continue
        if text[index] in "\"'":
            _, index = quoted(index)
            continue
        at_rule = identifier.match(text, index + 1) if text[index] == "@" else None
        if at_rule and _css_unescape(at_rule[0]).lower() == "import":
            index = space(at_rule.end())
            if index < len(text) and text[index] in "\"'":
                value, index = quoted(index)
                resources.append(value)
            continue
        match = identifier.match(text, index)
        if not match:
            index += 1
            continue
        token = match[0]
        index = match.end()
        start = space(index)
        if _css_unescape(token).lower() != "url" or start >= len(text) or text[start] != "(":
            continue
        index = space(start + 1)
        if index < len(text) and text[index] in "\"'":
            value, index = quoted(index)
            resources.append(value)
            end = text.find(")", index)
            index = len(text) if end < 0 else end + 1
        else:
            begin = index
            while index < len(text):
                if text[index] == "\\":
                    index += 2
                elif text[index] == ")":
                    break
                else:
                    index += 1
            resources.append(_css_unescape(text[begin:index].strip()))
            index += 1
    return resources


@dataclass
class MarkupInfo:
    references: list = field(default_factory=list)
    resources: list = field(default_factory=list)
    errors: list = field(default_factory=list)
    elements: list = field(default_factory=list)
    css_texts: list = field(default_factory=list)
    foreign_links: list = field(default_factory=list)


def _element(info, tag, attrs, html=True):
    # ElementTree supplies expanded names; HTMLParser resolves prefix bindings.
    if tag.startswith("{"):
        namespace, local = tag[1:].split("}", 1)
    else:
        namespace, local = "", tag
    if html:
        local = local.lower()
    normalized = {}
    for key, value in attrs.items():
        key = key.rsplit("}", 1)[-1]
        normalized[key.lower() if html else key] = value or ""
    attrs = normalized
    info.elements.append((local, dict(attrs)))
    if local == "link" and namespace not in ("", HTML_NAMESPACE):
        info.foreign_links.append({"namespace": namespace, "href": attrs.get("href", ""),
                                   "rel": attrs.get("rel", "")})
    if local == "link":
        rel = attrs.get("rel", "").lower().split()
        if namespace in ("", HTML_NAMESPACE):
            for label, relation in (("match", "=="), ("mismatch", "!=")):
                if label in rel:
                    info.references.append((relation, attrs.get("href", "")))
        # The production resource collector is generic DOM code; retain the
        # possible stylesheet fetch even when a foreign link cannot be a test.
        if "stylesheet" in rel or "preload" in rel:
            info.resources.append(attrs.get("href", ""))
    if local in ("img", "script", "iframe", "audio", "video", "source", "track", "embed"):
        if "src" in attrs:
            info.resources.append(attrs["src"])
    if local == "object" and "data" in attrs:
        info.resources.append(attrs["data"])
    if local in ("image", "use", "feImage") and "href" in attrs:
        info.resources.append(attrs["href"])
    if "poster" in attrs:
        info.resources.append(attrs["poster"])
    if "srcset" in attrs:
        for candidate in attrs["srcset"].split(","):
            if candidate.strip():
                info.resources.append(candidate.strip().split()[0])
    if "style" in attrs:
        info.css_texts.append(attrs["style"])
        info.resources.extend(css_resources(attrs["style"]))


class _HTML(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.info = MarkupInfo()
        self.styles = 0
        self.namespaces = [("", {}, HTML_NAMESPACE, HTML_NAMESPACE)]

    def handle_starttag(self, tag, attributes):
        # HTML's duplicate-attribute policy keeps the first occurrence, as does
        # the production html5ever parser. Later decoys must not hide a URL.
        attrs = {}
        for key, value in attributes:
            attrs.setdefault(key, value)
        bindings = dict(self.namespaces[-1][1])
        for key, value in attrs.items():
            if key == "xmlns":
                bindings[""] = value
            elif key.startswith("xmlns:"):
                bindings[key.split(":", 1)[1]] = value
        prefix, separator, local = tag.partition(":")
        parent_namespace = self.namespaces[-1][3]
        breakout = {"b", "big", "blockquote", "body", "br", "center", "code", "dd", "div", "dl", "dt",
                    "em", "embed", "h1", "h2", "h3", "h4", "h5", "h6", "head", "hr", "i", "img",
                    "li", "listing", "menu", "meta", "nobr", "ol", "p", "pre", "ruby", "s", "small",
                    "span", "strong", "strike", "sub", "sup", "table", "tt", "u", "ul", "var"}
        if not separator and parent_namespace not in ("", HTML_NAMESPACE) and tag in breakout:
            while len(self.namespaces) > 1 and self.namespaces[-1][3] not in ("", HTML_NAMESPACE):
                self.namespaces.pop()
            parent_namespace = HTML_NAMESPACE
        if separator:
            namespace = bindings.get(prefix, "")
        elif "xmlns" in attrs:
            namespace = attrs["xmlns"]
        elif tag == "svg":
            namespace = SVG_NAMESPACE
        elif tag == "math":
            namespace = MATH_NAMESPACE
        else:
            namespace = parent_namespace
        child_namespace = namespace
        if (namespace == SVG_NAMESPACE and tag in ("foreignobject", "desc", "title") or
                namespace == MATH_NAMESPACE and tag in ("mi", "mo", "mn", "ms", "mtext") or
                namespace == MATH_NAMESPACE and tag == "annotation-xml" and
                (attrs.get("encoding") or "").lower() in ("text/html", "application/xhtml+xml")):
            child_namespace = HTML_NAMESPACE
        void = {"area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr"}
        if tag not in void or namespace not in ("", HTML_NAMESPACE):
            self.namespaces.append((tag, bindings, namespace, child_namespace))
        resolved = "{" + namespace + "}" + (local if separator else tag) if namespace else tag
        _element(self.info, resolved, attrs)
        if (local if separator else tag) == "style":
            self.styles += 1

    def handle_endtag(self, tag):
        if tag.rsplit(":", 1)[-1] == "style":
            self.styles = max(0, self.styles - 1)
        for index in range(len(self.namespaces) - 1, 0, -1):
            if self.namespaces[index][0] == tag:
                del self.namespaces[index:]
                break

    def handle_startendtag(self, tag, attrs):
        self.handle_starttag(tag, attrs)
        self.handle_endtag(tag)

    def handle_data(self, text):
        if self.styles:
            self.info.css_texts.append(text)
            self.info.resources.extend(css_resources(text))


def decode_source(path, data):
    """Decode source for inventory; original bytes remain the imported object."""
    for marker, encoding in ((codecs.BOM_UTF32_LE, "utf-32"),
                             (codecs.BOM_UTF32_BE, "utf-32"),
                             (codecs.BOM_UTF8, "utf-8-sig"),
                             (codecs.BOM_UTF16_LE, "utf-16"),
                             (codecs.BOM_UTF16_BE, "utf-16")):
        if data.startswith(marker):
            return data.decode(encoding)
    for marker, encoding in ((b"\x00\x00\x00<", "utf-32-be"),
                             (b"<\x00\x00\x00", "utf-32-le"),
                             (b"\x00<\x00?", "utf-16-be"),
                             (b"<\x00?\x00", "utf-16-le")):
        if data.startswith(marker):
            return data.decode(encoding)
    declaration = re.search(br"^\s*<\?xml\b[^>]*\bencoding\s*=\s*['\"]([^'\"]+)['\"]", data[:1024], re.I)
    if declaration is None and PurePosixPath(path).suffix.lower() in (".html", ".htm"):
        declaration = re.search(br"<meta\b[^>]*\bcharset\s*=\s*['\"]?\s*([A-Za-z0-9._-]+)", data[:4096], re.I)
    if declaration:
        label = declaration[1].decode("ascii")
        if not re.fullmatch(r"[A-Za-z0-9._-]+", label):
            raise ValueError(f"Invalid source encoding declaration: {path}")
        encoding = codecs.lookup(label).name
        return data.decode(encoding)
    # Undeclared HTML can contain legacy text while its markup stays ASCII.
    # Surrogate escape preserves those bytes rather than dropping the document.
    return data.decode("utf-8", errors="surrogateescape")


def parse_document(path, data):
    """Shared importer/runner source parser; never executes active content."""
    suffix = PurePosixPath(path).suffix.lower()
    info = MarkupInfo()
    if suffix in (".html", ".htm"):
        parser = _HTML()
        parser.feed(decode_source(path, data))
        return parser.info
    if suffix in MARKUP_SUFFIXES:
        try:
            root = ET.fromstring(data)
        except (ET.ParseError, LookupError, ValueError) as error:
            # Preserve a source-derived protocol error rather than silently
            # exclude an XML reftest whose DTD/entity processing is unavailable.
            parser = _HTML()
            parser.feed(decode_source(path, data))
            parser.info.errors.append("XML parsing requires additional protocol: " + str(error))
            return parser.info
        for element in root.iter():
            if not isinstance(element.tag, str):
                continue
            _element(info, element.tag, element.attrib, html=False)
            if element.tag.rsplit("}", 1)[-1] == "style":
                text = "".join(element.itertext())
                info.css_texts.append(text)
                info.resources.extend(css_resources(text))
        return info
    text = decode_source(path, data)
    if suffix == ".css":
        info.css_texts.append(text)
        info.resources.extend(css_resources(text))
    return info


def reference_edges(path, data):
    return [resolved_edge(path, href, relation) for relation, href in parse_document(path, data).references]


def resource_edges(path, data):
    return [resolved_edge(path, href) for href in parse_document(path, data).resources]


@dataclass(frozen=True)
class Blob:
    path: str
    sha: str
    size: int
    mode: str = "100644"

    def validate(self):
        canonical_path(self.path)
        if (not re.fullmatch(r"[0-9a-f]{40}", self.sha) or type(self.size) is not int or
                self.size < 0 or self.mode not in ("100644", "100755")):
            raise ValueError(f"Invalid/nonregular Git blob metadata: {self.path}")


class Budget:
    def __init__(self):
        self.blobs = {}
        self.bytes = 0

    def add(self, blob):
        blob.validate()
        if blob.path in self.blobs:
            if self.blobs[blob.path] != blob:
                raise ValueError(f"Conflicting upstream metadata: {blob.path}")
            return False
        if len(self.blobs) >= MAX_FILES or blob.size > MAX_BYTES - self.bytes:
            raise ValueError("Corpus exceeds reviewed 4096-file/10-MiB admission before body read")
        self.blobs[blob.path] = blob
        self.bytes += blob.size
        return True


def git_subtree_sha1(blobs, prefix=PREFIX):
    """Reconstruct the Git tree identity, proving the complete blob roster."""
    root = {}
    for blob in blobs:
        blob.validate()
        if not blob.path.startswith(prefix + "/"):
            raise ValueError("Blob is outside the requested complete subtree")
        parts = blob.path[len(prefix) + 1:].split("/")
        node = root
        for component in parts[:-1]:
            child = node.setdefault(component, {})
            if not isinstance(child, dict):
                raise ValueError("File/tree path collision")
            node = child
        if parts[-1] in node:
            raise ValueError("Duplicate subtree blob path")
        node[parts[-1]] = blob

    def tree(node):
        entries = []
        for name in sorted(node, key=lambda name: (name + ("/" if isinstance(node[name], dict) else "")).encode("utf-8")):
            value = node[name]
            mode, oid = ("40000", tree(value)) if isinstance(value, dict) else (value.mode, value.sha)
            entries.append(mode.encode() + b" " + name.encode("utf-8") + b"\0" + bytes.fromhex(oid))
        data = b"".join(entries)
        return hashlib.sha1(b"tree " + str(len(data)).encode() + b"\0" + data).hexdigest()

    return tree(root)


class Source:
    def __init__(self):
        self.data = {}
        self.cache = None

    def configure_cache(self, path):
        self.cache = Path(path)
        self.cache.mkdir(parents=True, exist_ok=True)

    def cached(self, blob):
        if self.cache is None:
            return False
        path = self.cache / blob.sha
        if not path.exists():
            return False
        if path.is_symlink() or path.stat().st_size != blob.size:
            raise ValueError(f"Cached upstream blob has invalid size/type: {blob.path}")
        self.verify(blob, read_regular(path, blob.size))
        return True

    def verify(self, blob, data):
        if len(data) != blob.size or git_blob_sha1(data) != blob.sha:
            raise ValueError(f"Upstream Git blob verification failed: {blob.path}")
        self.data[blob.path] = data
        if self.cache is not None:
            path = self.cache / blob.sha
            if not path.exists():
                with path.open("xb") as output:
                    output.write(data)

    def load_many(self, blobs):
        for blob in blobs:
            if blob.path not in self.data and not self.cached(blob):
                self.verify(blob, self.read_blob(blob))


class GitSource(Source):
    def __init__(self, path):
        super().__init__()
        self.path = Path(path).resolve()
        revision = self.git("rev-parse", REVISION + "^{commit}").decode().strip()
        if revision != REVISION:
            raise ValueError("Offline source does not contain the exact pinned commit")

    def git(self, *args):
        return subprocess.check_output(["git", "-C", str(self.path), *args], timeout=120)

    def entries(self, directory, recursive=False):
        target = REVISION + (":" + directory if directory else "")
        arguments = ["ls-tree", "-lz"]
        if recursive:
            arguments.append("-r")
        data = self.git(*arguments, target)
        entries = []
        for item in data.split(b"\0"):
            if not item:
                continue
            metadata, name = item.split(b"\t", 1)
            mode, kind, sha, size = metadata.decode().split()
            path = "/".join(filter(None, (directory, name.decode("utf-8"))))
            entries.append({"path": path, "mode": mode, "type": kind, "sha": sha,
                            "size": int(size) if kind == "blob" else None})
        return entries

    def tree_sha(self, directory):
        return self.git("rev-parse", REVISION + ":" + directory).decode().strip()

    def metadata(self, path):
        parent = str(PurePosixPath(path).parent)
        parent = "" if parent == "." else parent
        entry = next((item for item in self.entries(parent) if item["path"] == path), None)
        return _blob(entry)

    def read_blob(self, blob):
        return self.git("cat-file", "blob", blob.sha)


def _blob(entry):
    if not entry or entry["type"] != "blob":
        return None
    return Blob(entry["path"], entry["sha"], entry["size"], entry["mode"])


class GitHubSource(Source):
    def __init__(self):
        super().__init__()
        commit = self.api(f"{API_REPOSITORY}/git/commits/{REVISION}")
        if commit.get("sha") != REVISION:
            raise ValueError("GitHub returned a different upstream commit")
        self.tree_ids = {"": commit["tree"]["sha"]}
        self.trees = {}

    def api(self, endpoint, body=None):
        command = ["gh", "api", endpoint]
        if body is not None:
            command += ["--input", "-"]
        result = subprocess.run(command, input=json.dumps(body).encode() if body else None,
                                stdout=subprocess.PIPE, check=True, timeout=120)
        return json.loads(result.stdout)

    def entries(self, directory, recursive=False):
        if directory not in self.tree_ids:
            parent = str(PurePosixPath(directory).parent)
            self.entries("" if parent == "." else parent)
        if directory not in self.tree_ids:
            return []
        key = (directory, recursive)
        if key not in self.trees:
            oid = self.tree_ids[directory]
            response = self.api(f"{API_REPOSITORY}/git/trees/{oid}" + ("?recursive=1" if recursive else ""))
            if response.get("sha") != oid or response.get("truncated") is not False:
                raise ValueError(f"Truncated or changed pinned Git tree: {directory}")
            entries = []
            for item in response["tree"]:
                path = "/".join(filter(None, (directory, item["path"])))
                # Unrelated siblings may have literal URL-special characters.
                # Admission/URL resolution validates the files actually used.
                canonical_path(path, url_safe=False)
                entry = {**item, "path": path}
                entries.append(entry)
                if item["type"] == "tree":
                    self.tree_ids[path] = item["sha"]
            self.trees[key] = entries
        return self.trees[key]

    def tree_sha(self, directory):
        self.entries(directory)
        return self.tree_ids[directory]

    def metadata(self, path):
        parent = str(PurePosixPath(path).parent)
        entry = next((item for item in self.entries("" if parent == "." else parent)
                      if item["path"] == path), None)
        return _blob(entry)

    def read_blob(self, blob):
        response = self.api(f"{API_REPOSITORY}/git/blobs/{blob.sha}")
        if response.get("sha") != blob.sha or response.get("encoding") != "base64":
            raise ValueError(f"Unexpected Git blob response: {blob.path}")
        return base64.b64decode(response["content"], validate=False)

    def load_many(self, blobs):
        pending = [blob for blob in blobs if blob.path not in self.data and not self.cached(blob)]
        for start in range(0, len(pending), 64):
            batch = pending[start:start + 64]
            aliases = [f'b{i}:object(oid:{json.dumps(blob.sha)}){{... on Blob{{oid byteSize text}}}}'
                       for i, blob in enumerate(batch)]
            query = 'query { repository(owner:"web-platform-tests",name:"wpt") {' + " ".join(aliases) + "} }"
            response = self.api("graphql", {"query": query})
            if response.get("errors"):
                raise ValueError(f"GitHub blob query failed: {response['errors']}")
            objects = response["data"]["repository"]
            for index, blob in enumerate(batch):
                value = objects[f"b{index}"]
                if value is None or value.get("oid") != blob.sha or value.get("byteSize") != blob.size:
                    raise ValueError(f"GitHub returned different blob metadata: {blob.path}")
                text = value.get("text")
                data = text.encode("utf-8") if text is not None else b""
                if len(data) != blob.size or git_blob_sha1(data) != blob.sha:
                    data = self.read_blob(blob)
                self.verify(blob, data)


def _license_name(path):
    name = PurePosixPath(path).name.upper()
    return (name.startswith(("LICENSE", "LICENCE", "COPYING", "COPYRIGHT")) or
            name in ("NOTICE", "NOTICE.TXT", "NOTICE.MD"))


def validate_reviewed_absences(records, flex_paths):
    """Only the exact predeclared upstream defects may lack physical bytes."""
    expected_evidence = {"directory_tree_sha": FLEX_TREE, "inventory_prefix": PREFIX,
                         "absent_from_complete_tree": True}
    triples = []
    for record in records:
        triple = (record.get("source"), record.get("href"), record.get("path"))
        if (triple not in REVIEWED_MISSING_DEPENDENCIES or record.get("evidence") != expected_evidence or
                triple[0] not in flex_paths or triple[2] in flex_paths or
                not triple[2].startswith(PREFIX + "/")):
            raise ValueError("Unreviewed or unproven upstream missing dependency")
        triples.append(triple)
    if len(triples) != len(set(triples)) or set(triples) != REVIEWED_MISSING_DEPENDENCIES:
        raise ValueError("Changed reviewed upstream missing dependency set")


def import_corpus(source, old, output):
    """Import into a new directory; all selection occurs before any Mg run."""
    output = Path(output)
    if output.exists():
        raise ValueError("Import output must be a new directory")
    if source.tree_sha(PREFIX) != FLEX_TREE:
        raise ValueError("Pinned flexbox directory tree identity changed")
    entries = source.entries(PREFIX, recursive=True)
    flex = [_blob(item) for item in entries if item["type"] == "blob"]
    if git_subtree_sha1(flex) != FLEX_TREE:
        raise ValueError("Complete flexbox blob roster does not reconstruct its pinned Git tree")
    budget = Budget()
    for blob in flex:
        budget.add(blob)
    for entry in old["files"]:
        blob = source.metadata(canonical_path(entry["path"]))
        if blob is None:
            raise ValueError(f"Old corpus file is absent upstream: {entry['path']}")
        budget.add(blob)
    # Register the complete initial inventory before any source body is read.
    source.load_many(list(budget.blobs.values()))
    candidates = sorted(blob.path for blob in flex if PurePosixPath(blob.path).suffix.lower() in MARKUP_SUFFIXES)
    tests = []
    for path in candidates:
        edges = reference_edges(path, source.data[path])
        if edges:
            tests.append({"path": path, "references": edges})
    old_tests = {test["path"]: test for test in old["tests"]}
    new_tests = {test["path"]: test for test in tests}
    for path, test in old_tests.items():
        if path not in new_tests or new_tests[path]["references"] != test["references"]:
            raise ValueError(f"Old selected test/reference changed: {path}")
        # Preserve historical descriptive fields as well as the original edges.
        new_tests[path].update(test)
    for entry in old["files"]:
        data = source.data[entry["path"]]
        if len(data) != entry["bytes"] or sha256(data) != entry["sha256"]:
            raise ValueError(f"Old corpus bytes changed: {entry['path']}")

    queue = deque(sorted(budget.blobs))
    visited = set()
    checked_licenses = set()
    unresolved = []
    missing = []
    foreign_links = []
    frontier = []
    licenses = set(old["upstream"]["license_files"])
    while queue:
        path = queue.popleft()
        if path in visited:
            continue
        visited.add(path)
        parent = PurePosixPath(path).parent
        for ancestor in (parent, *parent.parents):
            directory = "" if str(ancestor) == "." else str(ancestor)
            if directory in checked_licenses:
                continue
            checked_licenses.add(directory)
            for entry in source.entries(directory):
                if entry["type"] == "blob" and _license_name(entry["path"]):
                    licenses.add(entry["path"])
                    blob = _blob(entry)
                    if budget.add(blob):
                        source.load_many([blob])
                        queue.append(blob.path)
        data = source.data[path]
        info = parse_document(path, data)
        foreign_links.extend({"source": path, **link} for link in info.foreign_links)
        for message in info.errors:
            unresolved.append({"source": path, "kind": "markup-protocol", "reason": message})
        edges = [resolved_edge(path, href, relation) for relation, href in info.references]
        edges += [resolved_edge(path, href) for href in info.resources]
        for edge in edges:
            frontier.append({"source": path, **edge})
            child = edge["path"] or edge.get("source_path")
            if child is None:
                unresolved.append({"source": path, **edge})
                continue
            blob = source.metadata(child)
            if blob is None:
                if ((path, edge["href"], child) not in REVIEWED_MISSING_DEPENDENCIES or
                        not child.startswith(PREFIX + "/") or any(blob.path == child for blob in flex)):
                    raise ValueError(f"Unreviewed missing static dependency: {path} -> {edge['href']} ({child})")
                missing.append({"source": path, "href": edge["href"], "path": child,
                                "evidence": {"directory_tree_sha": FLEX_TREE,
                                             "inventory_prefix": PREFIX,
                                             "absent_from_complete_tree": True}})
                continue
            if budget.add(blob):
                source.load_many([blob])
                queue.append(child)

    validate_reviewed_absences(missing, {blob.path for blob in flex})
    if visited != set(budget.blobs):
        raise ValueError("Unvisited imported source frontier")
    files = [{"path": path, "bytes": budget.blobs[path].size,
              "git_blob_sha1": budget.blobs[path].sha, "git_mode": budget.blobs[path].mode,
              "sha256": sha256(source.data[path])}
             for path in sorted(budget.blobs)]
    selection = {
        "id": "css-flexbox-recursive-reftests-v2",
        "rule": "All recursive css/css-flexbox/ .html/.htm/.xht/.xhtml/.xml/.svg documents declaring link rel=match or rel=mismatch, sorted by path; no outcome, flags, script, resource or protocol filter.",
        "rationale": "Complete pinned-directory reftest selection frozen before browser execution; not full WPT or testharness support.",
        "limit": len(tests), "selected_tests_count": len(tests),
        "directory_tree_sha": FLEX_TREE, "recursive_markup_candidates_count": len(candidates),
        "recursive_upstream_files_count": len(flex),
        "recursive_upstream_bytes": sum(blob.size for blob in flex),
        "candidate_paths_sha256": sha256(("\n".join(candidates) + "\n").encode()),
        "selected_paths_sha256": sha256(("\n".join(test["path"] for test in tests) + "\n").encode()),
        "max_files": MAX_FILES, "max_total_bytes": MAX_BYTES,
        "preserved_tests_count": len(old_tests), "preserved_files_count": len(old["files"]),
    }
    manifest = {"schema_version": 1,
                "upstream": {**old["upstream"], "license_files": sorted(licenses)},
                "selection": selection, "files": files, "tests": tests,
                "upstream_missing_dependencies": sorted(missing, key=lambda item: (item["source"], item["href"]))}
    output.mkdir(parents=True)
    upstream = output / "upstream"
    for entry in files:
        target = upstream / entry["path"]
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(source.data[entry["path"]])
    write_json(output / "corpus.json", manifest)
    write_json(output / "import-report.json", {
        "revision": REVISION, "selection": selection, "files": len(files),
        "bytes": budget.bytes, "license_files": sorted(licenses),
        "unresolved_dependencies": unresolved,
        "upstream_missing_dependencies": manifest["upstream_missing_dependencies"],
        "foreign_link_declarations": foreign_links,
        "dependency_edges": frontier,
        "all_imported_frontiers_visited": True,
        "reconstructed_flex_tree_sha1": git_subtree_sha1(flex),
        "corpus_sha256": sha256((output / "corpus.json").read_bytes()),
        "browser_executed": False,
    })
    return manifest


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, ensure_ascii=True) + "\n", encoding="utf-8")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--source-checkout", type=Path)
    frozen = HERE / "baselines/css-flexbox-first-24-v1.corpus.json"
    parser.add_argument("--old-corpus", type=Path, default=frozen if frozen.exists() else HERE / "corpus.json")
    parser.add_argument("--cache-dir", type=Path, help="optional verified Git-blob cache in ignored tmp/")
    parser.add_argument("--output-dir", type=Path, required=True)
    arguments = parser.parse_args()
    old_bytes = read_regular(arguments.old_corpus, LEGACY_CORPUS_BYTES)
    if sha256(old_bytes) != LEGACY_CORPUS_SHA256:
        raise ValueError("Legacy corpus must retain its exact original SHA-256")
    old = json.loads(old_bytes)
    if old["upstream"]["revision"] != REVISION:
        raise ValueError("Old corpus must use the unchanged pinned revision")
    source = GitSource(arguments.source_checkout) if arguments.source_checkout else GitHubSource()
    if arguments.cache_dir:
        source.configure_cache(arguments.cache_dir)
    manifest = import_corpus(source, old, arguments.output_dir)
    print(json.dumps({"output": str(arguments.output_dir), "tests": len(manifest["tests"]),
                      "files": len(manifest["files"]),
                      "bytes": sum(entry["bytes"] for entry in manifest["files"]),
                      "selected_paths_sha256": manifest["selection"]["selected_paths_sha256"]}, sort_keys=True))


if __name__ == "__main__":
    main()
