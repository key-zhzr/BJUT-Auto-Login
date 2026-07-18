#!/usr/bin/env python3
"""Standard-library helper for scripts/capture-jfself.sh.

This file intentionally has no third-party dependencies so it can run while the
machine is isolated on the campus network.
"""

from __future__ import annotations

import argparse
from datetime import date, timedelta
import hashlib
import html
from html.parser import HTMLParser
import ipaddress
import json
from pathlib import Path
import re
import sys
from typing import Iterable
from urllib.parse import quote, urlencode, urljoin, urlparse, urlunparse


SENSITIVE_HEADER = re.compile(r"^(?:set-cookie|cookie|authorization|proxy-authorization):", re.I)
PHONE = re.compile(r"(?<!\d)1[3-9]\d{9}(?!\d)")
IDENTITY = re.compile(r"(?<![0-9A-Za-z])\d{17}[0-9Xx](?![0-9A-Za-z])")
EMAIL = re.compile(r"(?<![\w.+-])[\w.+-]+@[\w.-]+\.[A-Za-z]{2,}(?![\w.-])")
PASSWORD_FIELDS = re.compile(
    r"(?i)((?:password|passwd|pwd|upass)\s*[=:]\s*)(?![$({\[])([^&\s\"'<>]+)"
)
COOKIE_TEXT = re.compile(
    r"(?i)((?:session|jsessionid|token|sid)\s*[=:]\s*)(?![$({\[])([^&;\s\"'<>]+)"
)
CSRF_QUOTED_FIELD = re.compile(
    r'''(?i)(\b(?:ajax)?csrf(?:token)?\b\s*[=:]\s*)(["'])([^"']+)(\2)'''
)
CSRF_QUERY_FIELD = re.compile(
    r"(?i)((?:ajax)?csrftoken=)([^&\s\"'<>+]+)"
)
HTML_INPUT_TAG = re.compile(r"<input\b[^>]*>", re.I | re.S)
HTML_CSRF_NAME = re.compile(
    r'''(?i)\bname\s*=\s*(["']?)(?:ajax)?csrf(?:token)?\1'''
)
HTML_VALUE_ATTRIBUTE = re.compile(
    r'''(?i)(\bvalue\s*=\s*)(["'])([^"']*)(\2)'''
)
TOKEN_UUID = re.compile(
    r"(?i)(?<![0-9a-f])[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}(?![0-9a-f])"
)
JSON_PRIVATE_STRING_FIELD = re.compile(
    r'''(?i)("(?:account|userAccount|username|userName|userRealName|realName|userPassword|userIdNumber|identityNumber|userGender|userIp|loginIp|nasIp|ip|ipv6|mac|macAddress|sessionid|sessionId)"\s*:\s*)"((?:\\.|[^"\\])*)"'''
)
JSON_PRIVATE_NUMBER_FIELD = re.compile(
    r'''(?i)("(?:account|userId|startAdminId|stopAdminId)"\s*:\s*)-?\d+(?:\.\d+)?'''
)
JSON_PRIVATE_OBJECT_FIELD = re.compile(
    r'''(?i)("userExtar"\s*:\s*)\{[^{}]*\}'''
)
MAC_VALUE = re.compile(r"^(?:[0-9A-Fa-f]{2}[:-]?){5}[0-9A-Fa-f]{2}$")
PRIVATE_JSON_KEYS = {
    "account", "useraccount", "username", "user", "userid", "useridnumber",
    "userrealname", "realname", "userpassword", "identitynumber", "usergender",
    "userip", "loginip", "nasip", "ip", "ipv6", "mac", "macaddress",
    "sessionid", "session", "sid", "token", "csrftoken", "ajaxcsrftoken",
}
URL_CANDIDATE = re.compile(
    r"(?:(?:https?:)?//[A-Za-z0-9._:-]+/[A-Za-z0-9_?&=./%+~-]*|/[A-Za-z][A-Za-z0-9_?&=./%+~-]{2,})"
)


class CaptureParser(HTMLParser):
    def __init__(self, base_url: str = "") -> None:
        super().__init__(convert_charrefs=True)
        self.base_url = base_url
        self.forms: list[dict[str, object]] = []
        self.assets: set[str] = set()
        self.links: set[str] = set()
        self.text: list[str] = []
        self._form: dict[str, object] | None = None
        self._ignored_depth = 0

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        attrs_map = {key.lower(): (value or "") for key, value in attrs}
        tag = tag.lower()
        if tag in {"script", "style", "noscript"}:
            self._ignored_depth += 1
        if tag == "form":
            action = urljoin(self.base_url, attrs_map.get("action", ""))
            self._form = {
                "method": attrs_map.get("method", "get").upper(),
                "action": action,
                "inputs": [],
            }
            self.forms.append(self._form)
        elif tag in {"input", "button", "select", "textarea"} and self._form is not None:
            field_type = attrs_map.get("type", tag).lower()
            value = attrs_map.get("value", "")
            self._form["inputs"].append(
                {
                    "tag": tag,
                    "name": attrs_map.get("name", ""),
                    "id": attrs_map.get("id", ""),
                    "type": field_type,
                    "valueShape": value_shape(value, field_type),
                    "required": "required" in attrs_map,
                }
            )
        if tag == "script" and attrs_map.get("src"):
            self._add_asset(attrs_map["src"])
        if tag == "link" and attrs_map.get("href"):
            rel = attrs_map.get("rel", "").lower()
            href = attrs_map["href"]
            if "stylesheet" in rel or urlparse(href).path.lower().endswith(".css"):
                self._add_asset(href)
        if tag == "a" and attrs_map.get("href"):
            resolved = urljoin(self.base_url, attrs_map["href"])
            if same_origin(self.base_url, resolved):
                self.links.add(resolved)

    def handle_endtag(self, tag: str) -> None:
        tag = tag.lower()
        if tag in {"script", "style", "noscript"} and self._ignored_depth:
            self._ignored_depth -= 1
        if tag == "form":
            self._form = None

    def handle_data(self, data: str) -> None:
        if self._ignored_depth == 0:
            normalized = " ".join(data.split())
            if normalized:
                self.text.append(normalized)

    def _add_asset(self, value: str) -> None:
        resolved = without_session_path_parameter(urljoin(self.base_url, value))
        parsed = urlparse(resolved)
        if parsed.scheme in {"http", "https"} and same_origin(self.base_url, resolved):
            if parsed.path.lower().endswith((".js", ".css")):
                self.assets.add(resolved)


class CaptchaStateParser(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.found = False
        self.required = False

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        if tag.lower() != "div":
            return
        attrs_map = {key.lower(): (value or "") for key, value in attrs}
        if attrs_map.get("id") != "randomDiv":
            return
        self.found = True
        self.required = "hide" not in set(attrs_map.get("class", "").split())


def without_session_path_parameter(url: str) -> str:
    parsed = urlparse(url)
    if parsed.params.lower().startswith("jsessionid="):
        parsed = parsed._replace(params="")
    return urlunparse(parsed)


def same_origin(base: str, candidate: str) -> bool:
    base_parsed = urlparse(base)
    candidate_parsed = urlparse(candidate)
    return (
        base_parsed.scheme in {"http", "https"}
        and candidate_parsed.scheme == base_parsed.scheme
        and candidate_parsed.hostname == base_parsed.hostname
        and (candidate_parsed.port or default_port(candidate_parsed.scheme))
        == (base_parsed.port or default_port(base_parsed.scheme))
        and not candidate_parsed.username
        and not candidate_parsed.password
    )


def default_port(scheme: str) -> int:
    return 443 if scheme == "https" else 80


def value_shape(value: str, field_type: str) -> str:
    if not value:
        return "empty"
    if field_type == "password":
        return "redacted"
    if value.isdigit():
        return f"digits:{len(value)}"
    return f"text:{len(value)}"


def extract_checkcode(path: Path) -> str:
    source = path.read_text(encoding="utf-8", errors="replace")
    patterns = (
        r"<input\b[^>]*\bname\s*=\s*['\"]checkcode['\"][^>]*\bvalue\s*=\s*['\"]([^'\"]+)['\"]",
        r"<input\b[^>]*\bvalue\s*=\s*['\"]([^'\"]+)['\"][^>]*\bname\s*=\s*['\"]checkcode['\"]",
    )
    for pattern in patterns:
        match = re.search(pattern, source, flags=re.I | re.S)
        if match:
            return html.unescape(match.group(1)).strip()
    raise ValueError("checkcode input was not found")


def extract_login_action(path: Path, base_url: str) -> str:
    source = path.read_text(encoding="utf-8", errors="replace")
    parser = CaptureParser(base_url)
    parser.feed(source)
    required_fields = {"checkcode", "account", "password", "code"}
    for form in parser.forms:
        field_names = {
            str(field.get("name", ""))
            for field in form.get("inputs", [])
            if isinstance(field, dict)
        }
        if form.get("method") != "POST" or not required_fields.issubset(field_names):
            continue
        action = str(form.get("action", ""))
        parsed = urlparse(action)
        if (
            parsed.scheme != "https"
            or parsed.hostname != "jfself.bjut.edu.cn"
            or parsed.port not in (None, 443)
            or parsed.path != "/Self/login/verify"
            or parsed.query
            or parsed.fragment
            or (parsed.params and not parsed.params.lower().startswith("jsessionid="))
        ):
            raise ValueError("login form action is outside the allowed endpoint")
        return action
    raise ValueError("matching login form was not found")


def extract_safe_redirect(path: Path, base_url: str) -> str:
    location = ""
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        key, separator, value = line.partition(":")
        if separator and key.strip().lower() == "location":
            location = value.strip()
    if not location:
        raise ValueError("redirect response did not include Location")
    resolved = urljoin(base_url, location)
    parsed = urlparse(resolved)
    if (
        parsed.scheme != "https"
        or parsed.hostname != "jfself.bjut.edu.cn"
        or parsed.port not in (None, 443)
        or parsed.username
        or parsed.password
        or not parsed.path.startswith("/Self/")
    ):
        raise ValueError("redirect target is outside the allowed jfself origin")
    return resolved


def captcha_state(path: Path) -> str:
    parser = CaptchaStateParser()
    parser.feed(path.read_text(encoding="utf-8", errors="replace"))
    if not parser.found:
        raise ValueError("randomDiv CAPTCHA container was not found")
    return "required" if parser.required else "hidden"


def login_outcome(path: Path) -> str:
    parser = CaptureParser("https://jfself.bjut.edu.cn/Self/login/")
    parser.feed(path.read_text(encoding="utf-8", errors="replace"))
    required_fields = {"checkcode", "account", "password", "code"}
    for form in parser.forms:
        field_names = {
            str(field.get("name", ""))
            for field in form.get("inputs", [])
            if isinstance(field, dict)
        }
        if required_fields.issubset(field_names):
            return "rejected"
    return "authenticated"


def build_form_body(checkcode: str, account: str, password: str) -> str:
    return urlencode(
        {
            "checkcode": checkcode,
            "account": account,
            "password": password,
            "code": "",
        }
    )


def redact_text(source: str, secrets: Iterable[str], *, headers: bool = False) -> str:
    redacted = source
    if headers:
        redacted = "\n".join(
            "[REDACTED SENSITIVE HEADER]" if SENSITIVE_HEADER.match(line.strip()) else line
            for line in redacted.splitlines()
        ) + ("\n" if source.endswith("\n") else "")
    variants: set[str] = set()
    for secret in secrets:
        if not secret:
            continue
        encoded = quote(secret, safe="")
        form_encoded = urlencode({"value": secret}).partition("=")[2]
        variants.update(
            {
                secret,
                html.escape(secret),
                encoded,
                encoded.lower(),
                form_encoded,
                form_encoded.lower(),
            }
        )
    for secret in sorted(variants, key=len, reverse=True):
        redacted = redacted.replace(secret, "[REDACTED]")
    redacted = PASSWORD_FIELDS.sub(r"\1[REDACTED]", redacted)
    redacted = COOKIE_TEXT.sub(r"\1[REDACTED]", redacted)
    redacted = CSRF_QUOTED_FIELD.sub(
        lambda match: f"{match.group(1)}{match.group(2)}[REDACTED_TOKEN]{match.group(2)}",
        redacted,
    )
    redacted = CSRF_QUERY_FIELD.sub(r"\1[REDACTED_TOKEN]", redacted)

    def redact_csrf_input(match: re.Match[str]) -> str:
        tag = match.group(0)
        if not HTML_CSRF_NAME.search(tag):
            return tag
        return HTML_VALUE_ATTRIBUTE.sub(
            lambda value: f"{value.group(1)}{value.group(2)}[REDACTED_TOKEN]{value.group(2)}",
            tag,
            count=1,
        )

    redacted = HTML_INPUT_TAG.sub(redact_csrf_input, redacted)
    # The site also concatenates literal UUID tokens into URLs, where the field
    # name and value are not adjacent. UUID-shaped values in captures are
    # session-local identifiers and are never needed for offline analysis.
    redacted = TOKEN_UUID.sub("[REDACTED_TOKEN]", redacted)
    # Authenticated dashboard pages embed a large `window.user` JSON object.
    # Several private values are unrelated to the supplied credentials and
    # therefore cannot be removed by matching known secrets alone.
    redacted = JSON_PRIVATE_STRING_FIELD.sub(r'\1"[REDACTED_PRIVATE]"', redacted)
    redacted = JSON_PRIVATE_NUMBER_FIELD.sub(r"\g<1>0", redacted)
    redacted = JSON_PRIVATE_OBJECT_FIELD.sub(r"\1{}", redacted)
    redacted = PHONE.sub("[REDACTED_PHONE]", redacted)
    redacted = IDENTITY.sub("[REDACTED_ID]", redacted)
    redacted = EMAIL.sub("[REDACTED_EMAIL]", redacted)
    return redacted


def discover_private_values(raw_dir: Path) -> list[str]:
    """Find private values named by the site's own embedded JSON fields.

    Adding them to the normal secret list also removes a real name, IP, MAC or
    identifier if the same value is rendered elsewhere as plain HTML text.
    """
    values: set[str] = set()
    for path in raw_dir.iterdir():
        if not path.is_file() or path.suffix.lower() not in {".html", ".json", ".txt"}:
            continue
        source = path.read_text(encoding="utf-8", errors="replace")
        for match in JSON_PRIVATE_STRING_FIELD.finditer(source):
            try:
                value = json.loads(f'"{match.group(2)}"')
            except json.JSONDecodeError:
                value = match.group(2)
            if isinstance(value, str) and value and value not in {"0.0.0.0", "N/A"}:
                values.add(value)
    return sorted(values, key=len, reverse=True)


def looks_like_network_identifier(value: str) -> bool:
    candidate = value.strip()
    if MAC_VALUE.fullmatch(candidate):
        return True
    try:
        ipaddress.ip_address(candidate)
        return True
    except ValueError:
        return False


def sanitize_json_value(value: object, key: str = "") -> object:
    if key.lower() in PRIVATE_JSON_KEYS:
        return "[REDACTED_PRIVATE]"
    if isinstance(value, dict):
        return {str(item_key): sanitize_json_value(item, str(item_key)) for item_key, item in value.items()}
    if isinstance(value, list):
        return [sanitize_json_value(item) for item in value]
    if isinstance(value, str) and looks_like_network_identifier(value):
        # Some legacy table endpoints return positional arrays with no field
        # names. Redact network identifiers by value while preserving row shape.
        return "[REDACTED_NETWORK]"
    return value


def sanitize_structured_text(path: Path, source: str) -> str:
    if path.suffix.lower() != ".json":
        return source
    try:
        value = json.loads(source)
    except json.JSONDecodeError:
        return source
    sanitized = sanitize_json_value(value)
    return json.dumps(sanitized, ensure_ascii=False, separators=(",", ":"))


def page_report(path: Path, base_url: str, secrets: list[str]) -> dict[str, object]:
    source = path.read_text(encoding="utf-8", errors="replace")
    parser = CaptureParser(base_url)
    parser.feed(source)
    visible_text = redact_text(" | ".join(parser.text), secrets)
    forms = []
    for form in parser.forms:
        sanitized_form = dict(form)
        sanitized_form["action"] = redact_text(str(form.get("action", "")), secrets)
        forms.append(sanitized_form)
    return {
        "file": path.name,
        "forms": forms,
        "sameOriginLinks": [redact_text(value, secrets) for value in sorted(parser.links)],
        "assets": [redact_text(value, secrets) for value in sorted(parser.assets)],
        "visibleText": visible_text[:12000],
    }


def endpoint_candidates(files: Iterable[Path], base_url: str, secrets: list[str]) -> list[str]:
    candidates: set[str] = set()
    for path in files:
        if path.suffix.lower() not in {".html", ".htm", ".js", ".css", ".txt"}:
            continue
        source = path.read_text(encoding="utf-8", errors="replace")
        for match in URL_CANDIDATE.findall(source):
            candidate = match.replace("\\/", "/")
            if candidate.startswith("//"):
                candidate = "https:" + candidate
            resolved = urljoin(base_url, candidate)
            parsed = urlparse(resolved)
            if same_origin(base_url, resolved) and parsed.path.startswith("/Self/"):
                candidates.add(redact_text(resolved, secrets))
    return sorted(candidates)


def json_shape(value: object, depth: int = 0) -> object:
    if depth >= 5:
        return "nested"
    if isinstance(value, dict):
        return {str(key): json_shape(item, depth + 1) for key, item in value.items()}
    if isinstance(value, list):
        if not value:
            return ["empty"]
        shapes = []
        for item in value[:3]:
            shape = json_shape(item, depth + 1)
            if shape not in shapes:
                shapes.append(shape)
        return shapes
    if value is None:
        return "null"
    if isinstance(value, bool):
        return "boolean"
    if isinstance(value, (int, float)):
        return "number"
    return "string"


def data_response_report(path: Path, secrets: list[str]) -> dict[str, object]:
    meta = read_meta(path.with_suffix(".meta"))
    report: dict[str, object] = {
        "file": path.name,
        "meta": safe_meta(meta, secrets),
        "bytes": path.stat().st_size,
    }
    if path.suffix.lower() == ".json":
        try:
            report["jsonShape"] = json_shape(
                json.loads(path.read_text(encoding="utf-8", errors="replace"))
            )
        except json.JSONDecodeError:
            report["jsonShape"] = "invalid-json"
    return report


def copy_sanitized_tree(raw_dir: Path, share_dir: Path, secrets: list[str]) -> None:
    allowed_suffixes = {
        ".html", ".htm", ".js", ".css", ".json", ".txt", ".tsv", ".meta", ".headers"
    }
    for source in raw_dir.rglob("*"):
        if not source.is_file() or source.name == "PRIVATE-DO-NOT-SHARE.txt":
            continue
        if source.suffix.lower() not in allowed_suffixes:
            continue
        relative = source.relative_to(raw_dir)
        target = share_dir / "sanitized" / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        text = source.read_text(encoding="utf-8", errors="replace")
        text = sanitize_structured_text(source, text)
        text = redact_text(text, secrets, headers=source.suffix.lower() == ".headers")
        target.write_text(text, encoding="utf-8")


def sanitize(args: argparse.Namespace) -> int:
    raw_dir = Path(args.raw_dir).resolve()
    share_dir = Path(args.share_dir).resolve()
    if not raw_dir.is_dir():
        raise ValueError(f"raw directory does not exist: {raw_dir}")
    share_dir.mkdir(parents=True, exist_ok=True)
    password = sys.stdin.read() if args.password_stdin else ""
    secrets = [args.account, password, args.checkcode, *discover_private_values(raw_dir)]

    copy_sanitized_tree(raw_dir, share_dir, secrets)

    login_meta = read_meta(raw_dir / "00-login.meta")
    verify_meta = read_meta(raw_dir / "01-verify.meta")
    login_url = login_meta.get("url_effective", "https://jfself.bjut.edu.cn/Self/login/?302=LI")
    verify_url = verify_meta.get("url_effective", "https://jfself.bjut.edu.cn/Self/login/verify")
    pages = []
    for path in sorted(raw_dir.glob("*.html")):
        meta = read_meta(path.with_suffix(".meta"))
        fallback_url = login_url if path.name == "00-login.html" else verify_url
        base_url = meta.get("url_effective", fallback_url)
        pages.append(page_report(path, base_url, secrets))

    searchable_files = [path for path in raw_dir.rglob("*") if path.is_file()]
    data_responses = [
        data_response_report(path, secrets)
        for path in sorted(raw_dir.iterdir())
        if path.is_file()
        and path.suffix.lower() in {".json", ".txt"}
        and path.name != "PRIVATE-DO-NOT-SHARE.txt"
    ]
    report = {
        "formatVersion": 1,
        "target": {
            "origin": "https://jfself.bjut.edu.cn",
            "fixedAddress": "172.21.0.16",
            "routeUsed": args.route,
        },
        "privacy": {
            "account": "redacted",
            "password": "never persisted; redacted from responses if echoed",
            "cookies": "excluded",
            "checkcode": "redacted",
        },
        "loginRequest": {
            "method": "POST",
            "url": "https://jfself.bjut.edu.cn/Self/login/verify",
            "contentType": "application/x-www-form-urlencoded",
            "fields": ["checkcode", "account", "password", "code"],
            "attempts": 1,
        },
        "loginOutcome": login_outcome(raw_dir / "01-verify.html"),
        "loginPageMeta": safe_meta(login_meta, secrets),
        "verifyMeta": safe_meta(verify_meta, secrets),
        "pages": pages,
        "dataResponses": data_responses,
        "endpointCandidates": endpoint_candidates(searchable_files, verify_url, secrets),
    }
    (share_dir / "report.json").write_text(
        json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    (share_dir / "README.txt").write_text(
        "BJUT-AL jfself redacted capture\n\n"
        "This folder is intended for offline protocol analysis. Known account, password,\n"
        "cookie, checkcode, name, device address, phone, identity-number and email\n"
        "values were removed.\n"
        "Please inspect report.json and sanitized/ before sharing. The raw sibling\n"
        "directory remains private and must not be sent or committed.\n",
        encoding="utf-8",
    )
    return 0


def resanitize_shared(args: argparse.Namespace) -> int:
    """Apply the latest field-level filters to an already-redacted share tree.

    This intentionally needs no account, password, cookie, or raw capture. It
    exists so older share folders can be repaired after a new embedded private
    field is discovered.
    """
    share_dir = Path(args.share_dir).resolve()
    if not share_dir.is_dir():
        raise ValueError(f"share directory does not exist: {share_dir}")
    allowed_suffixes = {
        ".html", ".htm", ".js", ".css", ".json", ".txt", ".tsv", ".meta", ".headers"
    }
    for path in share_dir.rglob("*"):
        if not path.is_file() or path.suffix.lower() not in allowed_suffixes:
            continue
        source = path.read_text(encoding="utf-8", errors="replace")
        source = sanitize_structured_text(path, source)
        redacted = redact_text(source, [], headers=path.suffix.lower() == ".headers")
        path.write_text(redacted, encoding="utf-8")

    report_path = share_dir / "report.json"
    if report_path.is_file():
        report = json.loads(report_path.read_text(encoding="utf-8", errors="replace"))
        candidates = report.get("endpointCandidates", [])
        if isinstance(candidates, list):
            base_url = "https://jfself.bjut.edu.cn/Self/dashboard"
            report["endpointCandidates"] = sorted(
                {
                    candidate
                    for candidate in candidates
                    if isinstance(candidate, str)
                    and same_origin(base_url, candidate)
                    and urlparse(candidate).path.startswith("/Self/")
                }
            )
        report_path.write_text(
            json.dumps(report, ensure_ascii=False, indent=2) + "\n",
            encoding="utf-8",
        )
    return 0


def read_meta(path: Path) -> dict[str, str]:
    result: dict[str, str] = {}
    if not path.exists():
        return result
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        key, separator, value = line.partition("=")
        if separator:
            result[key] = value
    return result


def safe_meta(meta: dict[str, str], secrets: list[str]) -> dict[str, str]:
    allowed = {
        "http_code",
        "url_effective",
        "remote_ip",
        "content_type",
        "num_redirects",
        "ssl_verify_result",
        "time_total",
    }
    return {
        key: redact_text(value, secrets)
        for key, value in meta.items()
        if key in allowed
    }


def asset_urls(paths: list[Path], base_url: str) -> list[str]:
    assets: set[str] = set()
    for path in paths:
        if not path.exists():
            continue
        parser = CaptureParser(base_url)
        parser.feed(path.read_text(encoding="utf-8", errors="replace"))
        assets.update(parser.assets)
    return sorted(assets)


def asset_name(index: int, url: str) -> str:
    parsed = urlparse(url)
    basename = Path(parsed.path).name or "asset"
    basename = re.sub(r"[^A-Za-z0-9._-]", "_", basename)
    digest = hashlib.sha256(url.encode("utf-8")).hexdigest()[:10]
    return f"{index:03d}-{digest}-{basename}"


def capture_date_window(days: int = 60) -> tuple[str, str, str]:
    """Return a portable date window for the read-only billing queries."""
    if days < 0 or days > 366:
        raise ValueError("date window must be between 0 and 366 days")
    end = date.today()
    start = end - timedelta(days=days)
    return start.isoformat(), end.isoformat(), str(end.year)


def extract_ajax_csrf_token(path: Path) -> str:
    """Extract the session-local AJAX token without printing surrounding JS."""
    source = path.read_text(encoding="utf-8", errors="replace")
    match = re.search(
        r"(?i)(?<![A-Za-z0-9_])ajaxCsrfToken(?![A-Za-z0-9_])"
        r"\s*[:=]\s*['\"]([A-Za-z0-9_.-]{1,256})['\"]",
        source,
    )
    if not match:
        raise ValueError("AJAX CSRF token was not found")
    return match.group(1)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    checkcode = subparsers.add_parser("checkcode")
    checkcode.add_argument("html", type=Path)

    form = subparsers.add_parser("form-body")
    form.add_argument("checkcode")
    form.add_argument("account")

    action = subparsers.add_parser("login-action")
    action.add_argument("--base-url", required=True)
    action.add_argument("html", type=Path)

    redirect = subparsers.add_parser("safe-redirect")
    redirect.add_argument("--base-url", required=True)
    redirect.add_argument("headers", type=Path)

    captcha = subparsers.add_parser("captcha-state")
    captcha.add_argument("html", type=Path)

    outcome = subparsers.add_parser("login-outcome")
    outcome.add_argument("html", type=Path)

    assets = subparsers.add_parser("asset-urls")
    assets.add_argument("--base-url", required=True)
    assets.add_argument("html", nargs="+", type=Path)

    name = subparsers.add_parser("asset-name")
    name.add_argument("index", type=int)
    name.add_argument("url")

    date_window = subparsers.add_parser("date-window")
    date_window.add_argument("--days", type=int, default=60)

    ajax_token = subparsers.add_parser("ajax-token")
    ajax_token.add_argument("javascript", type=Path)

    scrub = subparsers.add_parser("sanitize")
    scrub.add_argument("--raw-dir", required=True)
    scrub.add_argument("--share-dir", required=True)
    scrub.add_argument("--account", required=True)
    scrub.add_argument("--checkcode", required=True)
    scrub.add_argument("--route", required=True, choices=("system", "fixed", "auto"))
    scrub.add_argument("--password-stdin", action="store_true")

    resanitize = subparsers.add_parser("resanitize-shared")
    resanitize.add_argument("--share-dir", required=True)
    return parser


def main() -> int:
    args = build_parser().parse_args()
    if args.command == "checkcode":
        print(extract_checkcode(args.html))
        return 0
    if args.command == "form-body":
        password = sys.stdin.read()
        sys.stdout.write(build_form_body(args.checkcode, args.account, password))
        return 0
    if args.command == "login-action":
        print(extract_login_action(args.html, args.base_url))
        return 0
    if args.command == "safe-redirect":
        print(extract_safe_redirect(args.headers, args.base_url))
        return 0
    if args.command == "captcha-state":
        print(captcha_state(args.html))
        return 0
    if args.command == "login-outcome":
        print(login_outcome(args.html))
        return 0
    if args.command == "asset-urls":
        for url in asset_urls(args.html, args.base_url):
            print(url)
        return 0
    if args.command == "asset-name":
        print(asset_name(args.index, args.url))
        return 0
    if args.command == "date-window":
        print("\t".join(capture_date_window(args.days)))
        return 0
    if args.command == "ajax-token":
        print(extract_ajax_csrf_token(args.javascript))
        return 0
    if args.command == "sanitize":
        return sanitize(args)
    if args.command == "resanitize-shared":
        return resanitize_shared(args)
    raise AssertionError(f"unhandled command: {args.command}")


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError) as error:
        print(f"Error: {error}", file=sys.stderr)
        raise SystemExit(1)
