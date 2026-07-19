#!/usr/bin/env python3

import importlib.util
import hashlib
import io
import json
import os
from pathlib import Path
import pty
import select
import tempfile
import time
from types import SimpleNamespace
import unittest
from urllib.parse import parse_qs, urlparse


MODULE_PATH = Path(__file__).with_name("cas_services_capture.py")
SPEC = importlib.util.spec_from_file_location("cas_services_capture", MODULE_PATH)
assert SPEC and SPEC.loader
capture = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(capture)


LOGIN_URL = (
    "https://cas.bjut.edu.cn/login?"
    "service=https%3A%2F%2Fuc.bjut.edu.cn%2F"
)


def login_page(*, captcha: bool = False, action: str = "") -> str:
    captcha_field = '<input name="captcha" type="text">' if captcha else ""
    return (
        f'<form method="post" action="{action}">'
        '<input type="hidden" name="execution" value="opaque-execution-token">'
        '<input type="hidden" name="type" value="username_password">'
        '<input name="username" value="">'
        '<input type="password" name="password" value="">'
        '<input type="hidden" name="_eventId" value="submit">'
        f"{captcha_field}"
        '<button type="submit">登录</button>'
        "</form>"
    )


class CasServicesCaptureTests(unittest.TestCase):
    def test_builds_session_bound_cas_form_without_losing_hidden_fields(self):
        with tempfile.TemporaryDirectory() as directory:
            page = Path(directory) / "login.html"
            page.write_text(login_page(), encoding="utf-8")
            self.assertEqual(capture.login_action(page, LOGIN_URL), LOGIN_URL)
            body = capture.build_form_body(
                page, LOGIN_URL, "25000000", "!example password!"
            )
            self.assertEqual(
                parse_qs(body, keep_blank_values=True),
                {
                    "execution": ["opaque-execution-token"],
                    "type": ["username_password"],
                    "username": ["25000000"],
                    "password": ["!example password!"],
                    "_eventId": ["submit"],
                },
            )

    def test_rejects_captcha_before_form_submission(self):
        with tempfile.TemporaryDirectory() as directory:
            page = Path(directory) / "login.html"
            page.write_text(login_page(captcha=True), encoding="utf-8")
            self.assertEqual(capture.captcha_state(page, LOGIN_URL), "required")
            with self.assertRaises(ValueError):
                capture.build_form_body(page, LOGIN_URL, "student", "password")

    def test_rejects_login_forms_for_an_untrusted_service_or_host(self):
        with tempfile.TemporaryDirectory() as directory:
            page = Path(directory) / "login.html"
            page.write_text(
                login_page(
                    action=(
                        "https://cas.bjut.edu.cn/login?"
                        "service=https%3A%2F%2Fexample.com%2F"
                    )
                ),
                encoding="utf-8",
            )
            with self.assertRaises(ValueError):
                capture.login_action(page, LOGIN_URL)
            page.write_text(
                login_page(action="https://example.com/collect"), encoding="utf-8"
            )
            with self.assertRaises(ValueError):
                capture.login_action(page, LOGIN_URL)

    def test_redirects_are_limited_to_the_explicit_https_allowlist(self):
        with tempfile.TemporaryDirectory() as directory:
            headers = Path(directory) / "headers.txt"
            headers.write_text(
                "HTTP/2 302\r\n"
                "Location: https://uc.bjut.edu.cn/?ticket=ST-1-secret\r\n",
                encoding="utf-8",
            )
            self.assertEqual(
                capture.extract_safe_redirect(
                    headers,
                    "https://cas.bjut.edu.cn/login",
                    {capture.CAS_HOST, capture.UC_HOST},
                ),
                "https://uc.bjut.edu.cn/?ticket=ST-1-secret",
            )
            for unsafe in (
                "http://uc.bjut.edu.cn/",
                "https://example.com/",
                "https://user:pass@uc.bjut.edu.cn/",
                "https://uc.bjut.edu.cn:444/",
            ):
                headers.write_text(
                    f"HTTP/2 302\r\nLocation: {unsafe}\r\n", encoding="utf-8"
                )
                with self.assertRaises(ValueError):
                    capture.extract_safe_redirect(
                        headers,
                        "https://cas.bjut.edu.cn/login",
                        {capture.CAS_HOST, capture.UC_HOST},
                    )

    def test_mobile_portal_allows_the_known_itsapp_oauth_hop_only_when_selected(self):
        with tempfile.TemporaryDirectory() as directory:
            headers = Path(directory) / "headers.txt"
            oauth_url = (
                "https://itsapp.bjut.edu.cn/uc/api/oauth/index?"
                "redirect=https%3A%2F%2Fydapp.bjut.edu.cn%2FopenV8HomePage&"
                "appid=200220816093810809&state=V8YKT&qrcode=1"
            )
            headers.write_text(
                f"HTTP/2 302\r\nLocation: {oauth_url}\r\n", encoding="utf-8"
            )
            self.assertEqual(
                capture.extract_safe_redirect(
                    headers,
                    "https://ydapp.bjut.edu.cn/openV8HomePage",
                    {capture.CAS_HOST, capture.ITS_HOST, capture.YD_HOST},
                ),
                oauth_url,
            )
            with self.assertRaises(ValueError):
                capture.extract_safe_redirect(
                    headers,
                    "https://ydapp.bjut.edu.cn/openV8HomePage",
                    {capture.CAS_HOST, capture.YD_HOST},
                )

    def test_solves_only_the_bounded_itsapp_md5_navigation_challenge(self):
        with tempfile.TemporaryDirectory() as directory:
            page = Path(directory) / "challenge.html"
            page.write_text(
                '<script src="/a155a53cde0f5585235d18ab56219d67.js"></script>'
                '<script>window.location.href="?redirect=https%3A%2F%2F'
                'ydapp.bjut.edu.cn%2FopenV8HomePage&appid=200220816093810809&'
                'state=V8YKT&qrcode=1&a17f5f4fdictkey="+'
                'md5("192.0.2.42");</script>',
                encoding="utf-8",
            )
            base = (
                "https://itsapp.bjut.edu.cn/uc/api/oauth/index?"
                "redirect=https%3A%2F%2Fydapp.bjut.edu.cn%2FopenV8HomePage&"
                "appid=200220816093810809&state=V8YKT&qrcode=1"
            )
            solved = capture.solve_itsapp_js_challenge(page, base)
            values = parse_qs(urlparse(solved).query, keep_blank_values=True)
            self.assertEqual(values["redirect"], [capture.YD_ENTRY_URL])
            self.assertEqual(values["appid"], [capture.YD_APP_ID])
            self.assertEqual(
                values["a17f5f4fdictkey"],
                [hashlib.md5(b"192.0.2.42").hexdigest()],
            )

            page.write_text(
                '<script>window.location.href="?redirect=https%3A%2F%2F'
                'example.com%2F&appid=200220816093810809&state=V8YKT&qrcode=1&'
                'a17f5f4fdictkey="+md5("192.0.2.42");</script>',
                encoding="utf-8",
            )
            with self.assertRaises(ValueError):
                capture.solve_itsapp_js_challenge(page, base)

    def test_collects_prefetched_and_literal_same_origin_assets_only(self):
        with tempfile.TemporaryDirectory() as directory:
            page = Path(directory) / "index.html"
            page.write_text(
                '<link rel="prefetch" href="/static/js/password.abc.js">'
                '<link rel="stylesheet" href="/static/css/app.css?v=1">'
                '<script src="https://example.com/foreign.js"></script>'
                '<script>const lazy="/static/js/recharge.0123abcd.js";</script>',
                encoding="utf-8",
            )
            self.assertEqual(
                capture.asset_urls([page], "https://uc.bjut.edu.cn/"),
                [
                    "https://uc.bjut.edu.cn/static/css/app.css?v=1",
                    "https://uc.bjut.edu.cn/static/js/password.abc.js",
                    "https://uc.bjut.edu.cn/static/js/recharge.0123abcd.js",
                ],
            )

    def test_reconstructs_only_deployed_network_fee_chunks_from_web_pack_runtime(self):
        source = (
            'const source="/static/js/device/accelerometer.js";'
            'const other="/static/js/media/choose-image.js";'
            '({"pages-recharge-networkFeeCharge-networkFeeCharge~'
            'pages-recharge-networkFeeCharge-networkFeeChargeNew":"pages-recharge-'
            'networkFeeCharge-networkFeeCharge~pages-recharge-networkFeeCharge-'
            'networkFeeChargeNew","pages-recharge-networkFeeCharge-networkFeeCharge":'
            '"pages-recharge-networkFeeCharge-networkFeeCharge","pages-recharge-'
            'networkFeeCharge-networkFeeChargeNew":"pages-recharge-networkFeeCharge-'
            'networkFeeChargeNew"}[e]||e)+"."+{"pages-recharge-networkFeeCharge-'
            'networkFeeCharge~pages-recharge-networkFeeCharge-networkFeeChargeNew":'
            '"71c23382","pages-recharge-networkFeeCharge-networkFeeCharge":'
            '"78229315","pages-recharge-networkFeeCharge-networkFeeChargeNew":'
            '"7f97a2a1","pages-recharge-cardrecharge-cardrecharge":"3ab57a45"}[e]+".js"'
        )
        with tempfile.TemporaryDirectory() as directory:
            runtime = Path(directory) / "index.js"
            runtime.write_text(source, encoding="utf-8")
            self.assertEqual(
                capture.asset_urls(
                    [runtime],
                    "https://ydapp.bjut.edu.cn/static/js/index.733fe17b.js",
                ),
                [
                    "https://ydapp.bjut.edu.cn/static/js/pages-recharge-networkFeeCharge-networkFeeCharge.78229315.js",
                    "https://ydapp.bjut.edu.cn/static/js/pages-recharge-networkFeeCharge-networkFeeChargeNew.7f97a2a1.js",
                    "https://ydapp.bjut.edu.cn/static/js/pages-recharge-networkFeeCharge-networkFeeCharge~pages-recharge-networkFeeCharge-networkFeeChargeNew.71c23382.js",
                ],
            )

    def test_extracts_openid_without_exposing_it_in_state_output(self):
        with tempfile.TemporaryDirectory() as directory:
            headers = Path(directory) / "headers.txt"
            headers.write_text(
                "Location: https://ydapp.bjut.edu.cn/#/pages/home?"
                "openid=private-openid-value&displayflag=1\n",
                encoding="utf-8",
            )
            self.assertEqual(
                capture.extract_openid([headers]), "private-openid-value"
            )

    def test_redaction_removes_all_authentication_and_personal_values(self):
        source = (
            "Set-Cookie: CASTGC=TGT-secret-cookie\n"
            "Location: https://uc.bjut.edu.cn/?ticket=ST-123-private\n"
            '<input name="execution" value="private-execution">'
            '{"openid":"private-openid","realName":"Example Student",'
            '"mobile":"13800138000","endpoint":"/api/reset/rules",'
            '"clientIp":"192.0.2.42",'
            '"appid":"200220816093810809"}'
        )
        result = capture.redact_text(
            source,
            ["25000000", "!password!", "private-openid", "Example Student"],
            headers=True,
        )
        for secret in (
            "TGT-secret-cookie",
            "ST-123-private",
            "private-execution",
            "private-openid",
            "Example Student",
            "13800138000",
            "192.0.2.42",
        ):
            self.assertNotIn(secret, result)
        self.assertIn("/api/reset/rules", result)
        self.assertIn("200220816093810809", result)

    def test_share_package_contains_no_known_secret(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            raw = root / "raw"
            share = root / "share"
            (raw / "assets" / "uc").mkdir(parents=True)
            account = "25000000"
            password = "!example secret!"
            openid = "private-openid-value"
            (raw / "00-cas-login.html").write_text(
                login_page(), encoding="utf-8"
            )
            (raw / "00-cas-login.meta").write_text(
                f"http_code=200\nurl_effective={LOGIN_URL}\n", encoding="utf-8"
            )
            (raw / "01-cas-submit.headers").write_text(
                "HTTP/2 302\nSet-Cookie: CASTGC=private-cookie\n"
                "Location: https://uc.bjut.edu.cn/?ticket=ST-123-private\n",
                encoding="utf-8",
            )
            (raw / "02-uc-home.html").write_text(
                f"<p>{account}</p><p>{password}</p>", encoding="utf-8"
            )
            (raw / "02-uc-home.meta").write_text(
                "http_code=200\nurl_effective=https://uc.bjut.edu.cn/\n",
                encoding="utf-8",
            )
            (raw / "13-uc-userinfo.json").write_text(
                json.dumps(
                    {
                        "username": account,
                        "realName": "Example Student",
                        "openid": openid,
                        "rules": ["password length"],
                    }
                ),
                encoding="utf-8",
            )
            (raw / "20-yd-entry.headers").write_text(
                f"Location: https://ydapp.bjut.edu.cn/#/home?openid={openid}\n",
                encoding="utf-8",
            )
            (raw / "assets" / "uc" / "app.js").write_text(
                'fetch("/api/reset/rules")', encoding="utf-8"
            )

            previous_stdin = capture.sys.stdin
            try:
                capture.sys.stdin = io.StringIO(password)
                capture.sanitize(
                    SimpleNamespace(
                        raw_dir=str(raw),
                        share_dir=str(share),
                        account=account,
                        login_outcome="authenticated",
                        password_stdin=True,
                    )
                )
            finally:
                capture.sys.stdin = previous_stdin

            combined = "\n".join(
                path.read_text(encoding="utf-8")
                for path in share.rglob("*")
                if path.is_file()
            )
            for secret in (
                account,
                password,
                openid,
                "private-cookie",
                "ST-123-private",
                "Example Student",
                "opaque-execution-token",
            ):
                self.assertNotIn(secret, combined)
            report = json.loads((share / "report.json").read_text(encoding="utf-8"))
            self.assertTrue(report["openidDetected"])
            self.assertEqual(report["safety"]["loginPosts"], 1)
            self.assertTrue(
                any(
                    candidate["value"] == "/api/reset/rules"
                    for candidate in report["endpointCandidates"]
                )
            )

    def test_response_report_records_shape_without_private_values(self):
        with tempfile.TemporaryDirectory() as directory:
            response = Path(directory) / "response.json"
            response.write_text(
                '{"success":true,"data":{"realName":"Private"}}',
                encoding="utf-8",
            )
            self.assertEqual(
                capture.response_shape(response),
                {"kind": "object", "keys": ["data", "success"]},
            )

    @unittest.skipIf(os.name == "nt", "the capture entry point is a Bash script")
    def test_full_shell_flow_runs_with_bash_32_and_keeps_share_redacted(self):
        """Exercise prompts, arrays, redirects, packaging and cleanup together."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fake_bin = root / "bin"
            output = root / "captures"
            fake_bin.mkdir()
            fake_curl = fake_bin / "curl"
            fake_curl.write_text(
                r'''#!/usr/bin/env python3
import pathlib
import sys

args = sys.argv[1:]

def option(name, default=""):
    try:
        return args[args.index(name) + 1]
    except (ValueError, IndexError):
        return default

url = args[-1]
output = option("--output")
headers = option("--dump-header")
write_out = option("--write-out")
status = "200"
content_type = "text/html"
body = "<html><main>ok</main></html>"
location = ""
effective = url

is_post = "--data-binary" in args
if is_post:
    posted = sys.stdin.read()
    if "username=25000000" not in posted or "password=%21example+secret%21" not in posted:
        raise SystemExit(71)
    status = "302"
    body = ""
    location = "https://uc.bjut.edu.cn/?ticket=ST-1-private-ticket"
elif "cas.bjut.edu.cn/login" in url:
    body = (
        '<form method="post" action="">'
        '<input type="hidden" name="execution" value="private-execution">'
        '<input type="hidden" name="type" value="username_password">'
        '<input name="username"><input type="password" name="password">'
        '<input type="hidden" name="_eventId" value="submit">'
        '</form>'
    )
elif "uc.bjut.edu.cn/api/" in url:
    content_type = "application/json"
    body = '{"success":true,"realName":"Example Student"}'
elif "uc.bjut.edu.cn" in url:
    body = '<html><script>fetch("/api/reset/rules")</script></html>'
elif url == "https://ydapp.bjut.edu.cn/openV8HomePage":
    status = "302"
    body = ""
    location = (
        "https://itsapp.bjut.edu.cn/uc/api/oauth/index?"
        "redirect=https%3A%2F%2Fydapp.bjut.edu.cn%2FopenV8HomePage&"
        "appid=200220816093810809&state=V8YKT&qrcode=1"
    )
elif "itsapp.bjut.edu.cn/uc/api/oauth/index" in url:
    if "a17f5f4fdictkey=" in url:
        status = "302"
        body = ""
        location = (
            "https://ydapp.bjut.edu.cn/#/pages/homepage/index/index?"
            "openid=private-openid-value"
        )
    else:
        body = (
            '<script src="/a155a53cde0f5585235d18ab56219d67.js"></script>'
            '<script>window.location.href="?redirect=https%3A%2F%2F'
            'ydapp.bjut.edu.cn%2FopenV8HomePage&appid=200220816093810809&'
            'state=V8YKT&qrcode=1&a17f5f4fdictkey="+'
            'md5("192.0.2.42");</script>'
        )
elif "ydapp.bjut.edu.cn" in url:
    body = '<html><script>const api="/api/recharge/query";</script></html>'

if output:
    pathlib.Path(output).write_text(body, encoding="utf-8")
if headers:
    header = f"HTTP/2 {status}\n"
    if location:
        header += f"Location: {location}\n"
    if is_post:
        header += "Set-Cookie: CASTGC=TGT-private-cookie\n"
    pathlib.Path(headers).write_text(header, encoding="utf-8")

if write_out == "%{http_code}":
    sys.stdout.write(status)
elif write_out:
    sys.stdout.write(
        f"http_code={status}\n"
        f"url_effective={effective}\n"
        "remote_ip=127.0.0.1\n"
        f"content_type={content_type}\n"
        "num_redirects=0\n"
        "ssl_verify_result=0\n"
        "time_total=0.001\n"
    )
''',
                encoding="utf-8",
            )
            fake_curl.chmod(0o755)

            environment = os.environ.copy()
            environment["PATH"] = f"{fake_bin}:{environment.get('PATH', '')}"
            environment["TMPDIR"] = str(root)
            command = [
                "/bin/bash",
                str(Path(__file__).with_name("capture-cas-services.sh")),
                "--output",
                str(output),
            ]
            pid, master = pty.fork()
            if pid == 0:  # pragma: no cover - replaced by the capture process
                try:
                    os.chdir(Path(__file__).resolve().parent.parent)
                    os.execve(command[0], command, environment)
                except BaseException:
                    os._exit(127)

            console_bytes = bytearray()

            def wait_for_prompt(marker: bytes) -> None:
                deadline = time.monotonic() + 10
                while marker not in console_bytes:
                    if time.monotonic() >= deadline:
                        os.kill(pid, 9)
                        raise AssertionError(
                            f"capture prompt timed out: {console_bytes.decode(errors='replace')}"
                        )
                    ready, _, _ = select.select([master], [], [], 0.25)
                    if ready:
                        chunk = os.read(master, 4096)
                        if not chunk:
                            break
                        console_bytes.extend(chunk)

            status = None
            try:
                wait_for_prompt(b"account: ")
                os.write(master, b"25000000\r")
                wait_for_prompt(b"password (input hidden): ")
                os.write(master, b"!example secret!\r")
                deadline = time.monotonic() + 30
                while status is None:
                    if time.monotonic() >= deadline:
                        os.kill(pid, 9)
                        _, status = os.waitpid(pid, 0)
                        self.fail(
                            "capture script timed out:\n"
                            + console_bytes.decode(errors="replace")
                        )
                    ready, _, _ = select.select([master], [], [], 0.1)
                    if ready:
                        try:
                            chunk = os.read(master, 4096)
                        except OSError:
                            chunk = b""
                        console_bytes.extend(chunk)
                    waited_pid, waited_status = os.waitpid(pid, os.WNOHANG)
                    if waited_pid:
                        status = waited_status
                while True:
                    ready, _, _ = select.select([master], [], [], 0)
                    if not ready:
                        break
                    try:
                        chunk = os.read(master, 4096)
                    except OSError:
                        break
                    if not chunk:
                        break
                    console_bytes.extend(chunk)
            finally:
                os.close(master)
            console = console_bytes.decode(errors="replace")
            assert status is not None
            self.assertEqual(os.waitstatus_to_exitcode(status), 0, console)
            self.assertIn("Capture complete.", console)

            archives = list(output.glob("cas-services-*.share.zip"))
            self.assertEqual(len(archives), 1)
            shares = list(output.glob("cas-services-*.share"))
            self.assertEqual(len(shares), 1)
            combined = "\n".join(
                path.read_text(encoding="utf-8")
                for path in shares[0].rglob("*")
                if path.is_file()
            )
            for secret in (
                "25000000",
                "!example secret!",
                "private-execution",
                "private-ticket",
                "private-openid-value",
                "TGT-private-cookie",
                "Example Student",
                "192.0.2.42",
            ):
                self.assertNotIn(secret, combined)
            report = json.loads((shares[0] / "report.json").read_text(encoding="utf-8"))
            self.assertTrue(report["itsappJsChallengeDetected"])


if __name__ == "__main__":
    unittest.main()
