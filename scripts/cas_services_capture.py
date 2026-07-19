#!/usr/bin/env python3
"""Offline helper for scripts/capture-cas-services.sh.

Only Python's standard library is used so the capture can run while Codex is
unreachable.  The helper validates CAS forms and redirect targets, discovers
same-origin assets/endpoints, and builds a redacted share package.
"""

from __future__ import annotations

import argparse
from html import escape, unescape
from html.parser import HTMLParser
import hashlib
import json
from pathlib import Path
import re
import sys
from typing import Iterable
from urllib.parse import parse_qs, quote, unquote, urlencode, urljoin, urlparse


CAS_HOST = "cas.bjut.edu.cn"
UC_HOST = "uc.bjut.edu.cn"
YD_HOST = "ydapp.bjut.edu.cn"
ALLOWED_HOSTS = {CAS_HOST, UC_HOST, YD_HOST}
UC_SERVICE_URL = "https://uc.bjut.edu.cn/"
TEXT_SUFFIXES = {
    ".css",
    ".headers",
    ".htm",
    ".html",
    ".js",
    ".json",
    ".map",
    ".meta",
    ".tsv",
    ".txt",
    ".xml",
}

SENSITIVE_HEADER = re.compile(
    r"^(?:set-cookie|cookie|authorization|proxy-authorization):", re.I
)
PHONE = re.compile(r"(?<!\d)1[3-9]\d{9}(?!\d)")
IDENTITY = re.compile(r"(?<![0-9A-Za-z])\d{17}[0-9Xx](?![0-9A-Za-z])")
EMAIL = re.compile(r"(?<![\w.+-])[\w.+-]+@[\w.-]+\.[A-Za-z]{2,}(?![\w.-])")
CAS_TICKET = re.compile(r"(?<![A-Za-z0-9])(?:ST|TGT)-[A-Za-z0-9._~-]{8,}")
HTML_INPUT_TAG = re.compile(r"<input\b[^>]*>", re.I | re.S)
HTML_SENSITIVE_NAME = re.compile(
    r'''(?i)\bname\s*=\s*(["']?)(?:execution|lt|ticket|openid|'''
    r'''_csrf|csrf(?:token)?|token|password|passwd|pwd|code|state)\1'''
)
HTML_VALUE_ATTRIBUTE = re.compile(
    r'''(?i)(\bvalue\s*=\s*)(["'])([^"']*)(\2)'''
)
QUERY_SECRET = re.compile(
    r"(?i)((?:[?&#]|&amp;)(?:ticket|openid|execution|lt|_csrf|csrf(?:token)?|"
    r"access_token|refresh_token|token|code|state)=)([^&#\s\"'<>]+)"
)
FORM_SECRET = re.compile(
    r"(?i)((?:^|[&;\s])(?:username|account|password|passwd|pwd|execution|lt|"
    r"ticket|openid|_csrf|csrf(?:token)?|access_token|refresh_token|token|code|state)=)"
    r"([^&;\s\"'<>]+)"
)
QUOTED_PRIVATE_FIELD = re.compile(
    r'''(?i)(["'](?:username|userName|account|studentNo|studentId|xh|realName|name|'''
    r'''mobile|phone|email|idCard|identityNumber|campusCardNo|cardNo|openid|openId|'''
    r'''ticket|execution|accessToken|refreshToken|csrfToken|token|password|passwd|pwd)'''
    r'''["']\s*:\s*)(["'])(.*?)(\2)'''
)
NUMERIC_PRIVATE_FIELD = re.compile(
    r'''(?i)(["'](?:studentNo|studentId|xh|idCard|identityNumber|campusCardNo|cardNo)'''
    r'''["']\s*:\s*)-?\d+'''
)
COOKIE_VALUE = re.compile(
    r"(?i)((?:CASTGC|TGC|JSESSIONID|SESSION|SESSIONID|sid)\s*[=:]\s*)"
    r"([^;\s\"'<>]+)"
)
OPENID_PARAMETER = re.compile(
    r"(?i)(?:[?&#]|&amp;)openid=([^&#\s\"'<>]+)"
)
ABSOLUTE_URL = re.compile(
    r"https://(?:cas|uc|ydapp)\.bjut\.edu\.cn/[^\s\"'<>\\)]*",
    re.I,
)
RELATIVE_ENDPOINT = re.compile(
    r'''(?P<quote>["'])(?P<path>/(?:api|openV8HomePage|pages|service|auth|cas|'''
    r'''user|recharge|network|payment|pay)[A-Za-z0-9_?&=./%+~:@${}\[\]-]{1,300})(?P=quote)''',
    re.I,
)
LITERAL_ASSET = re.compile(
    r'''(?P<quote>["'])(?P<url>(?:(?:https:)?//[^"']+|(?:\.{0,2}/)?'''
    r'''[A-Za-z0-9_./-]+)\.(?:js|css)(?:\?[^"']*)?)(?P=quote)''',
    re.I,
)


def normalized_https_url(url: str, allowed_hosts: set[str]) -> str:
    parsed = urlparse(url)
    host = (parsed.hostname or "").lower()
    if (
        parsed.scheme != "https"
        or host not in allowed_hosts
        or parsed.port not in (None, 443)
        or parsed.username
        or parsed.password
    ):
        raise ValueError(f"URL left the trusted HTTPS host allowlist: {url}")
    return url


def same_origin(base_url: str, candidate: str) -> bool:
    base = urlparse(base_url)
    other = urlparse(candidate)
    return (
        base.scheme == "https"
        and other.scheme == "https"
        and (base.hostname or "").lower() == (other.hostname or "").lower()
        and base.port in (None, 443)
        and other.port in (None, 443)
        and not other.username
        and not other.password
    )


class CasPageParser(HTMLParser):
    def __init__(self, base_url: str) -> None:
        super().__init__(convert_charrefs=True)
        self.base_url = base_url
        self.forms: list[dict[str, object]] = []
        self.assets: set[str] = set()
        self.captcha_required = False
        self._form: dict[str, object] | None = None

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        attrs_map = {key.lower(): (value or "") for key, value in attrs}
        tag = tag.lower()
        if tag == "form":
            self._form = {
                "method": attrs_map.get("method", "get").upper(),
                "action": urljoin(self.base_url, attrs_map.get("action", "")),
                "inputs": [],
            }
            self.forms.append(self._form)
        elif tag == "input" and self._form is not None:
            field_type = attrs_map.get("type", "text").lower()
            self._form["inputs"].append(
                {
                    "name": attrs_map.get("name", ""),
                    "type": field_type,
                    "value": attrs_map.get("value", ""),
                    "checked": "checked" in attrs_map,
                    "disabled": "disabled" in attrs_map,
                }
            )
            name = attrs_map.get("name", "").lower()
            if "captcha" in name and field_type != "hidden":
                self.captcha_required = True
        elif tag in {"iframe", "img"}:
            source = attrs_map.get("src", "").lower()
            if any(marker in source for marker in ("captcha", "geetest", "verifycode")):
                self.captcha_required = True

        candidate = ""
        if tag == "script":
            candidate = attrs_map.get("src", "")
        elif tag == "link":
            candidate = attrs_map.get("href", "")
        if candidate:
            resolved = urljoin(self.base_url, unescape(candidate))
            if same_origin(self.base_url, resolved):
                path = urlparse(resolved).path.lower()
                if path.endswith((".js", ".css")):
                    self.assets.add(resolved)

    def handle_endtag(self, tag: str) -> None:
        if tag.lower() == "form":
            self._form = None


def parse_page(path: Path, base_url: str) -> CasPageParser:
    parser = CasPageParser(base_url)
    parser.feed(path.read_text(encoding="utf-8", errors="replace"))
    return parser


def select_login_form(path: Path, base_url: str) -> tuple[CasPageParser, dict[str, object]]:
    normalized_https_url(base_url, {CAS_HOST})
    parser = parse_page(path, base_url)
    for form in parser.forms:
        inputs = form.get("inputs", [])
        names = {
            str(field.get("name", "")).lower()
            for field in inputs
            if isinstance(field, dict)
        }
        if form.get("method") == "POST" and {"username", "password", "execution"}.issubset(names):
            action = str(form.get("action", ""))
            parsed = urlparse(normalized_https_url(action, {CAS_HOST}))
            if parsed.path.rstrip("/") != "/login":
                raise ValueError("CAS login form action is not /login")
            query = parse_qs(parsed.query, keep_blank_values=True)
            unknown = set(query) - {"service"}
            if unknown:
                raise ValueError("CAS login form action has unexpected query fields")
            if "service" in query and query["service"] != [UC_SERVICE_URL]:
                raise ValueError("CAS login form targets an unexpected service")
            return parser, form
    raise ValueError("CAS username/password login form was not found")


def login_action(path: Path, base_url: str) -> str:
    parser, form = select_login_form(path, base_url)
    if parser.captcha_required:
        raise ValueError("CAS currently requires a visual CAPTCHA")
    return str(form["action"])


def captcha_state(path: Path, base_url: str) -> str:
    parser, _ = select_login_form(path, base_url)
    return "required" if parser.captcha_required else "clear"


def build_form_body(path: Path, base_url: str, account: str, password: str) -> str:
    parser, form = select_login_form(path, base_url)
    if parser.captcha_required:
        raise ValueError("CAS currently requires a visual CAPTCHA")

    fields: dict[str, str] = {}
    for item in form.get("inputs", []):
        if not isinstance(item, dict) or item.get("disabled"):
            continue
        name = str(item.get("name", ""))
        field_type = str(item.get("type", "text")).lower()
        if not name or field_type in {"button", "file", "reset", "submit"}:
            continue
        if field_type in {"checkbox", "radio"} and not item.get("checked"):
            continue
        fields.setdefault(name, str(item.get("value", "")))

    if not fields.get("execution"):
        raise ValueError("CAS execution token is missing")
    fields["username"] = account
    fields["password"] = password
    fields["type"] = "username_password"
    fields["_eventId"] = "submit"
    return urlencode(list(fields.items()))


def extract_safe_redirect(path: Path, base_url: str, allowed_hosts: set[str]) -> str:
    location = ""
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        key, separator, value = line.partition(":")
        if separator and key.strip().lower() == "location":
            location = value.strip()
    if not location:
        raise ValueError("redirect response did not include Location")
    return normalized_https_url(urljoin(base_url, location), allowed_hosts)


def asset_urls(paths: list[Path], base_url: str) -> list[str]:
    normalized_https_url(base_url, ALLOWED_HOSTS)
    result: set[str] = set()
    for path in paths:
        if not path.is_file():
            continue
        source = path.read_text(encoding="utf-8", errors="replace")
        parser = CasPageParser(base_url)
        parser.feed(source)
        result.update(parser.assets)
        for match in LITERAL_ASSET.finditer(source):
            literal = unescape(match.group("url"))
            if literal.startswith("//"):
                literal = f"https:{literal}"
            resolved = urljoin(base_url, literal)
            if same_origin(base_url, resolved):
                result.add(resolved)
    return sorted(result)


def asset_name(index: int, url: str) -> str:
    normalized_https_url(url, ALLOWED_HOSTS)
    basename = Path(urlparse(url).path).name or "asset"
    basename = re.sub(r"[^A-Za-z0-9._-]", "_", basename)
    digest = hashlib.sha256(url.encode("utf-8")).hexdigest()[:10]
    return f"{index:03d}-{digest}-{basename}"


def extract_openid(files: list[Path]) -> str | None:
    for path in files:
        if not path.is_file():
            continue
        source = path.read_text(encoding="utf-8", errors="replace")
        match = OPENID_PARAMETER.search(source)
        if not match:
            continue
        value = unquote(unescape(match.group(1))).strip()
        if 8 <= len(value) <= 512 and not any(character.isspace() for character in value):
            return value
    return None


def secret_variants(secrets: Iterable[str]) -> set[str]:
    variants: set[str] = set()
    for secret in secrets:
        if not secret:
            continue
        encoded = quote(secret, safe="")
        form_encoded = urlencode({"value": secret}).partition("=")[2]
        variants.update(
            {
                secret,
                escape(secret),
                encoded,
                encoded.lower(),
                form_encoded,
                form_encoded.lower(),
            }
        )
    return variants


def redact_text(source: str, secrets: Iterable[str], *, headers: bool = False) -> str:
    redacted = source
    if headers:
        lines = []
        for line in redacted.splitlines():
            lines.append(
                "[REDACTED SENSITIVE HEADER]"
                if SENSITIVE_HEADER.match(line.strip())
                else line
            )
        redacted = "\n".join(lines) + ("\n" if source.endswith("\n") else "")

    for secret in sorted(secret_variants(secrets), key=len, reverse=True):
        redacted = redacted.replace(secret, "[REDACTED]")

    def redact_input(match: re.Match[str]) -> str:
        tag = match.group(0)
        if not HTML_SENSITIVE_NAME.search(tag):
            return tag
        return HTML_VALUE_ATTRIBUTE.sub(
            lambda value: (
                f"{value.group(1)}{value.group(2)}[REDACTED_PRIVATE]"
                f"{value.group(2)}"
            ),
            tag,
            count=1,
        )

    redacted = HTML_INPUT_TAG.sub(redact_input, redacted)
    redacted = QUERY_SECRET.sub(r"\1[REDACTED_PRIVATE]", redacted)
    redacted = FORM_SECRET.sub(r"\1[REDACTED_PRIVATE]", redacted)
    redacted = QUOTED_PRIVATE_FIELD.sub(r"\1\2[REDACTED_PRIVATE]\2", redacted)
    redacted = NUMERIC_PRIVATE_FIELD.sub(r"\g<1>0", redacted)
    redacted = COOKIE_VALUE.sub(r"\1[REDACTED_PRIVATE]", redacted)
    redacted = CAS_TICKET.sub("[REDACTED_CAS_TICKET]", redacted)
    redacted = PHONE.sub("[REDACTED_PHONE]", redacted)
    redacted = IDENTITY.sub("[REDACTED_ID]", redacted)
    redacted = EMAIL.sub("[REDACTED_EMAIL]", redacted)
    return redacted


PRIVATE_KEYS = {
    "username",
    "user_name",
    "account",
    "studentno",
    "student_no",
    "studentid",
    "student_id",
    "xh",
    "realname",
    "real_name",
    "name",
    "mobile",
    "phone",
    "email",
    "idcard",
    "identitynumber",
    "campuscardno",
    "cardno",
    "openid",
    "open_id",
    "ticket",
    "execution",
    "token",
    "accesstoken",
    "refreshtoken",
}


def collect_json_private_values(value: object, result: set[str]) -> None:
    if isinstance(value, dict):
        for key, item in value.items():
            normalized = re.sub(r"[^a-z0-9_]", "", str(key).lower())
            if normalized in PRIVATE_KEYS and isinstance(item, (str, int, float)):
                text = str(item)
                if len(text) >= 3:
                    result.add(text)
            collect_json_private_values(item, result)
    elif isinstance(value, list):
        for item in value:
            collect_json_private_values(item, result)


def discover_private_values(raw_dir: Path) -> list[str]:
    values: set[str] = set()
    openid = extract_openid([path for path in raw_dir.rglob("*") if path.is_file()])
    if openid:
        values.add(openid)
    for path in raw_dir.rglob("*"):
        if not path.is_file() or path.suffix.lower() != ".json":
            continue
        try:
            value = json.loads(path.read_text(encoding="utf-8", errors="replace"))
        except (json.JSONDecodeError, OSError):
            continue
        collect_json_private_values(value, values)
    return sorted(values, key=len, reverse=True)


def read_meta(path: Path) -> dict[str, str]:
    result: dict[str, str] = {}
    if not path.is_file():
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


def response_shape(path: Path) -> dict[str, object]:
    if not path.is_file():
        return {"kind": "missing"}
    text = path.read_text(encoding="utf-8", errors="replace")
    try:
        value = json.loads(text)
    except json.JSONDecodeError:
        return {"kind": "text", "characters": len(text)}
    if isinstance(value, dict):
        return {"kind": "object", "keys": sorted(str(key) for key in value)[:100]}
    if isinstance(value, list):
        return {"kind": "array", "items": len(value)}
    return {"kind": type(value).__name__}


def endpoint_candidates(paths: list[Path], secrets: list[str]) -> list[dict[str, str]]:
    candidates: dict[tuple[str, str], dict[str, str]] = {}
    for path in paths:
        if not path.is_file() or path.suffix.lower() not in TEXT_SUFFIXES:
            continue
        source = path.read_text(encoding="utf-8", errors="replace")
        for match in ABSOLUTE_URL.finditer(source):
            value = redact_text(match.group(0), secrets).rstrip(".,;:")
            candidates[(str(path.name), value)] = {"source": path.name, "value": value}
        for match in RELATIVE_ENDPOINT.finditer(source):
            value = redact_text(match.group("path"), secrets).rstrip(".,;:")
            candidates[(str(path.name), value)] = {"source": path.name, "value": value}
    return [candidates[key] for key in sorted(candidates)][:2000]


def copy_sanitized_tree(raw_dir: Path, share_dir: Path, secrets: list[str]) -> None:
    output_root = share_dir / "sanitized"
    for source in raw_dir.rglob("*"):
        if not source.is_file() or source.suffix.lower() not in TEXT_SUFFIXES:
            continue
        if source.name == "PRIVATE-DO-NOT-SHARE.txt":
            continue
        target = output_root / source.relative_to(raw_dir)
        target.parent.mkdir(parents=True, exist_ok=True)
        text = source.read_text(encoding="utf-8", errors="replace")
        target.write_text(
            redact_text(text, secrets, headers=source.suffix.lower() == ".headers"),
            encoding="utf-8",
        )


def sanitize(args: argparse.Namespace) -> int:
    raw_dir = Path(args.raw_dir).resolve()
    share_dir = Path(args.share_dir).resolve()
    if not raw_dir.is_dir():
        raise ValueError(f"raw directory does not exist: {raw_dir}")
    share_dir.mkdir(parents=True, exist_ok=True)
    password = sys.stdin.read() if args.password_stdin else ""
    secrets = [args.account, password, *discover_private_values(raw_dir)]
    copy_sanitized_tree(raw_dir, share_dir, secrets)

    page_entries = []
    api_entries = []
    for body in sorted(raw_dir.glob("*.html")):
        page_entries.append(
            {
                "file": body.name,
                "meta": safe_meta(read_meta(body.with_suffix(".meta")), secrets),
            }
        )
    for body in sorted(raw_dir.glob("*.json")):
        api_entries.append(
            {
                "file": body.name,
                "meta": safe_meta(read_meta(body.with_suffix(".meta")), secrets),
                "shape": response_shape(body),
            }
        )

    all_text_files = [
        path
        for path in raw_dir.rglob("*")
        if path.is_file() and path.suffix.lower() in TEXT_SUFFIXES
    ]
    openid = extract_openid(all_text_files)
    report = {
        "formatVersion": 1,
        "targets": [
            "https://cas.bjut.edu.cn",
            "https://uc.bjut.edu.cn",
            "https://ydapp.bjut.edu.cn",
        ],
        "privacy": {
            "account": "redacted",
            "password": "read from stdin and never persisted",
            "cookies": "excluded from the share package",
            "casTickets": "redacted",
            "openid": "redacted" if openid else "not observed",
        },
        "safety": {
            "loginPosts": 1,
            "otherRequests": "credential-free read-only GET requests",
            "passwordChanges": 0,
            "recharges": 0,
            "otherStateChanges": 0,
        },
        "loginOutcome": args.login_outcome,
        "openidDetected": bool(openid),
        "openidLength": len(openid) if openid else None,
        "rechargeRouteTemplate": (
            "#/pages/recharge/networkFeeCharge/networkFeeChargeNew?"
            "openid=[REDACTED]&displayflag=1&id=20220719101102"
        ),
        "pages": page_entries,
        "readOnlyApiResponses": api_entries,
        "endpointCandidates": endpoint_candidates(all_text_files, secrets),
    }
    (share_dir / "report.json").write_text(
        json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    (share_dir / "README.txt").write_text(
        "BJUT-AL CAS/UC/mobile-portal redacted capture\n\n"
        "This package contains only text files sanitized for offline protocol analysis.\n"
        "The supplied account/password, cookies, CAS tickets, openid, common personal\n"
        "fields, phone numbers, identity numbers and email addresses were removed.\n"
        "Inspect report.json and sanitized/ before sharing. Never share the sibling\n"
        "directory whose name ends in .private.\n",
        encoding="utf-8",
    )
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)

    action = commands.add_parser("login-action")
    action.add_argument("--base-url", required=True)
    action.add_argument("html", type=Path)

    captcha = commands.add_parser("captcha-state")
    captcha.add_argument("--base-url", required=True)
    captcha.add_argument("html", type=Path)

    form = commands.add_parser("form-body")
    form.add_argument("--base-url", required=True)
    form.add_argument("--account", required=True)
    form.add_argument("html", type=Path)

    redirect = commands.add_parser("safe-redirect")
    redirect.add_argument("--base-url", required=True)
    redirect.add_argument("--allowed-hosts", required=True)
    redirect.add_argument("headers", type=Path)

    assets = commands.add_parser("asset-urls")
    assets.add_argument("--base-url", required=True)
    assets.add_argument("files", nargs="+", type=Path)

    name = commands.add_parser("asset-name")
    name.add_argument("index", type=int)
    name.add_argument("url")

    openid = commands.add_parser("openid-state")
    openid.add_argument("files", nargs="+", type=Path)

    scrub = commands.add_parser("sanitize")
    scrub.add_argument("--raw-dir", required=True)
    scrub.add_argument("--share-dir", required=True)
    scrub.add_argument("--account", required=True)
    scrub.add_argument("--login-outcome", required=True)
    scrub.add_argument("--password-stdin", action="store_true")
    return parser


def main() -> int:
    args = build_parser().parse_args()
    if args.command == "login-action":
        print(login_action(args.html, args.base_url))
        return 0
    if args.command == "captcha-state":
        print(captcha_state(args.html, args.base_url))
        return 0
    if args.command == "form-body":
        password = sys.stdin.read()
        sys.stdout.write(
            build_form_body(args.html, args.base_url, args.account, password)
        )
        return 0
    if args.command == "safe-redirect":
        hosts = {host.strip().lower() for host in args.allowed_hosts.split(",") if host.strip()}
        if not hosts or not hosts.issubset(ALLOWED_HOSTS):
            raise ValueError("redirect host allowlist is invalid")
        print(extract_safe_redirect(args.headers, args.base_url, hosts))
        return 0
    if args.command == "asset-urls":
        for url in asset_urls(args.files, args.base_url):
            print(url)
        return 0
    if args.command == "asset-name":
        print(asset_name(args.index, args.url))
        return 0
    if args.command == "openid-state":
        value = extract_openid(args.files)
        print(f"present:{len(value)}" if value else "missing")
        return 0
    if args.command == "sanitize":
        return sanitize(args)
    raise AssertionError(f"unhandled command: {args.command}")


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError) as error:
        print(f"Error: {error}", file=sys.stderr)
        raise SystemExit(1)
