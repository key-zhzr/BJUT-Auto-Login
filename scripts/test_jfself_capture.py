#!/usr/bin/env python3

import importlib.util
import io
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from urllib.parse import parse_qs


MODULE_PATH = Path(__file__).with_name("jfself_capture.py")
SPEC = importlib.util.spec_from_file_location("jfself_capture", MODULE_PATH)
assert SPEC and SPEC.loader
jfself = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(jfself)


class JfselfCaptureTests(unittest.TestCase):
    def test_extracts_checkcode_regardless_of_attribute_order(self):
        with tempfile.TemporaryDirectory() as directory:
            first = Path(directory) / "first.html"
            first.write_text('<input name="checkcode" value="1234">', encoding="utf-8")
            second = Path(directory) / "second.html"
            second.write_text("<input value='5678' type='hidden' name='checkcode'>", encoding="utf-8")
            self.assertEqual(jfself.extract_checkcode(first), "1234")
            self.assertEqual(jfself.extract_checkcode(second), "5678")
            variable_width = Path(directory) / "variable-width.html"
            variable_width.write_text(
                '<input name="checkcode" value="123">', encoding="utf-8"
            )
            self.assertEqual(jfself.extract_checkcode(variable_width), "123")

    def test_form_body_has_the_exact_four_expected_fields(self):
        body = jfself.build_form_body("1234", "student", "!example password!")
        self.assertEqual(
            parse_qs(body, keep_blank_values=True),
            {
                "checkcode": ["1234"],
                "account": ["student"],
                "password": ["!example password!"],
                "code": [""],
            },
        )

    def test_extracts_and_restricts_session_bound_login_action(self):
        with tempfile.TemporaryDirectory() as directory:
            page = Path(directory) / "login.html"
            page.write_text(
                '<form method="post" action="/Self/login/verify;jsessionid=abc123">'
                '<input name="checkcode"><input name="account">'
                '<input name="password"><input name="code"></form>',
                encoding="utf-8",
            )
            self.assertEqual(
                jfself.extract_login_action(
                    page, "https://jfself.bjut.edu.cn/Self/login/?302=LI"
                ),
                "https://jfself.bjut.edu.cn/Self/login/verify;jsessionid=abc123",
            )
            page.write_text(
                '<form method="post" action="https://example.com/steal">'
                '<input name="checkcode"><input name="account">'
                '<input name="password"><input name="code"></form>',
                encoding="utf-8",
            )
            with self.assertRaises(ValueError):
                jfself.extract_login_action(
                    page, "https://jfself.bjut.edu.cn/Self/login/?302=LI"
                )

    def test_only_accepts_https_same_origin_login_redirects(self):
        with tempfile.TemporaryDirectory() as directory:
            headers = Path(directory) / "headers.txt"
            headers.write_text(
                "HTTP/2 302\r\nLocation: /Self/dashboard\r\n", encoding="utf-8"
            )
            self.assertEqual(
                jfself.extract_safe_redirect(
                    headers,
                    "https://jfself.bjut.edu.cn/Self/login/verify;jsessionid=test",
                ),
                "https://jfself.bjut.edu.cn/Self/dashboard",
            )
            for unsafe in (
                "https://example.com/collect",
                "http://jfself.bjut.edu.cn/Self/dashboard",
                "https://user:pass@jfself.bjut.edu.cn/Self/dashboard",
            ):
                headers.write_text(
                    f"HTTP/2 302\r\nLocation: {unsafe}\r\n", encoding="utf-8"
                )
                with self.assertRaises(ValueError):
                    jfself.extract_safe_redirect(
                        headers, "https://jfself.bjut.edu.cn/Self/login/verify"
                    )

    def test_detects_when_visual_captcha_is_required(self):
        with tempfile.TemporaryDirectory() as directory:
            page = Path(directory) / "login.html"
            page.write_text(
                '<div class="form-group hide" id="randomDiv"></div>', encoding="utf-8"
            )
            self.assertEqual(jfself.captcha_state(page), "hidden")
            page.write_text(
                '<div id="randomDiv" class="form-group"></div>', encoding="utf-8"
            )
            self.assertEqual(jfself.captcha_state(page), "required")

    def test_distinguishes_rejected_login_from_authenticated_page(self):
        with tempfile.TemporaryDirectory() as directory:
            page = Path(directory) / "response.html"
            page.write_text(
                '<form><input name="checkcode"><input name="account">'
                '<input name="password"><input name="code"></form>',
                encoding="utf-8",
            )
            self.assertEqual(jfself.login_outcome(page), "rejected")
            page.write_text('<main>账户余额</main>', encoding="utf-8")
            self.assertEqual(jfself.login_outcome(page), "authenticated")

    def test_parser_reports_form_shape_without_values(self):
        parser = jfself.CaptureParser("https://jfself.bjut.edu.cn/Self/home")
        parser.feed(
            '<form method="post" action="/Self/login/verify">'
            '<input name="account" value="12345678">'
            '<input type="password" name="password" value="secret">'
            '</form>'
        )
        self.assertEqual(parser.forms[0]["method"], "POST")
        self.assertEqual(parser.forms[0]["inputs"][0]["valueShape"], "digits:8")
        self.assertEqual(parser.forms[0]["inputs"][1]["valueShape"], "redacted")
        self.assertNotIn("12345678", str(parser.forms))
        self.assertNotIn("secret", str(parser.forms))

    def test_extracts_only_a_bounded_ajax_csrf_token(self):
        with tempfile.TemporaryDirectory() as directory:
            script = Path(directory) / "shareJS.js"
            script.write_text(
                "$.ajaxSetup({data:{ajaxCsrfToken:'abc-123_DEF.456'}});",
                encoding="utf-8",
            )
            self.assertEqual(
                jfself.extract_ajax_csrf_token(script), "abc-123_DEF.456"
            )
            script.write_text(
                "var AJAXCSRFTOKEN = 'page-local-token';",
                encoding="utf-8",
            )
            self.assertEqual(
                jfself.extract_ajax_csrf_token(script), "page-local-token"
            )
            script.write_text(
                "$.ajaxSetup({data:{ajaxCsrfToken:window.token}});",
                encoding="utf-8",
            )
            with self.assertRaises(ValueError):
                jfself.extract_ajax_csrf_token(script)

    def test_capture_date_window_is_bounded_and_ordered(self):
        start, end, year = jfself.capture_date_window(60)
        self.assertLessEqual(start, end)
        self.assertEqual(year, end[:4])
        with self.assertRaises(ValueError):
            jfself.capture_date_window(367)

    def test_redaction_removes_credentials_and_headers(self):
        source = (
            "Set-Cookie: SESSION=abc123\n"
            "account=12345678&password=p%40ss&email=a@example.com\n"
        )
        redacted = jfself.redact_text(
            source, ["12345678", "p@ss", "4321"], headers=True
        )
        self.assertNotIn("abc123", redacted)
        self.assertNotIn("12345678", redacted)
        self.assertNotIn("p%40ss", redacted)
        self.assertNotIn("a@example.com", redacted)

    def test_redaction_does_not_corrupt_javascript_password_variables(self):
        source = "var $password = $(\"[name='password']\"); password: { required: true };"
        self.assertEqual(jfself.redact_text(source, []), source)

    def test_redaction_removes_all_csrf_token_shapes(self):
        source = (
            '<input type="hidden" name="csrftoken" '
            'value="11111111-2222-3333-4444-555555555555">'
            "<script>var AJAXCSRFTOKEN = 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee';"
            "$.post('x', {ajaxCsrfToken: 'literal-token'});"
            "location.href='x?ajaxCsrfToken=query-token';"
            "location.href='x?ajaxCsrfToken=' + '12345678-1234-1234-1234-123456789abc';"
            "</script>"
        )
        redacted = jfself.redact_text(source, [])
        for token in (
            "11111111-2222-3333-4444-555555555555",
            "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
            "literal-token",
            "query-token",
            "12345678-1234-1234-1234-123456789abc",
        ):
            self.assertNotIn(token, redacted)
        self.assertGreaterEqual(redacted.count("[REDACTED_TOKEN]"), 5)

    def test_structured_redaction_removes_token_fields(self):
        source = '{"ajaxCsrfToken":"private","rows":[],"total":0}'
        sanitized = jfself.sanitize_structured_text(Path("table.json"), source)
        self.assertNotIn("private", sanitized)
        self.assertEqual(
            jfself.json.loads(sanitized)["ajaxCsrfToken"], "[REDACTED_PRIVATE]"
        )

    def test_redaction_removes_private_fields_from_embedded_user_json(self):
        source = (
            '<script>(function(user){window.user=user;})({'
            '"userName":"student","userRealName":"Example Name",'
            '"userPassword":"encrypted-value","userIdNumber":"masked-id",'
            '"userGender":"x","userIp":"10.0.0.8",'
            '"macAddress":"001122334455","sessionId":"private-session",'
            '"userId":123456,'
            '"userExtar":{"userId":123456},"leftMoney":12.5});</script>'
        )
        redacted = jfself.redact_text(source, [])
        for private_value in (
            "student",
            "Example Name",
            "encrypted-value",
            "masked-id",
            "10.0.0.8",
            "001122334455",
            "private-session",
            "123456",
        ):
            self.assertNotIn(private_value, redacted)
        self.assertIn('"leftMoney":12.5', redacted)

    def test_redaction_removes_private_fields_from_table_json(self):
        source = (
            '[{"account":25000000,"ip":"10.1.2.3","ipv6":"2001:db8::1",'
            '"mac":"00-11-22-33-44-55","sessionid":"private",'
            '"useFlow":42.5}]'
        )
        redacted = jfself.redact_text(source, [])
        for private_value in ("25000000", "10.1.2.3", "2001:db8::1", "00-11-22-33-44-55", "private"):
            self.assertNotIn(private_value, redacted)
        self.assertIn('"useFlow":42.5', redacted)

    def test_structured_redaction_handles_positional_history_rows(self):
        source = (
            '[[1700000000000,1700000100000,"10.1.2.3","2001:db8::1",'
            '"001122334455",10,2.5,2,0.01]]'
        )
        sanitized = jfself.sanitize_structured_text(Path("history.json"), source)
        self.assertNotIn("10.1.2.3", sanitized)
        self.assertNotIn("2001:db8::1", sanitized)
        self.assertNotIn("001122334455", sanitized)
        result = jfself.json.loads(sanitized)
        self.assertEqual(len(result[0]), 9)
        self.assertEqual(result[0][5:], [10, 2.5, 2, 0.01])

    def test_structured_redaction_handles_keyed_online_sessions(self):
        source = (
            '[{"sessionId":"secret-session","ip":"10.1.2.3",'
            '"mac":"00-11-22-33-44-55","useTime":30}]'
        )
        sanitized = jfself.sanitize_structured_text(Path("online.json"), source)
        self.assertNotIn("secret-session", sanitized)
        self.assertNotIn("10.1.2.3", sanitized)
        self.assertNotIn("00-11-22-33-44-55", sanitized)
        self.assertEqual(jfself.json.loads(sanitized)[0]["useTime"], 30)

    def test_discovered_private_values_are_available_for_plain_html_redaction(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "dashboard.html").write_text(
                '<script>window.user={"userRealName":"Example Person",'
                '"mac":"00-11-22-33-44-55"}</script>'
                '<p>Example Person</p><p>00-11-22-33-44-55</p>',
                encoding="utf-8",
            )
            discovered = jfself.discover_private_values(root)
            self.assertIn("Example Person", discovered)
            self.assertIn("00-11-22-33-44-55", discovered)
            redacted = jfself.redact_text(
                (root / "dashboard.html").read_text(encoding="utf-8"), discovered
            )
            self.assertNotIn("Example Person", redacted)
            self.assertNotIn("00-11-22-33-44-55", redacted)

    def test_share_tree_contains_no_known_secret_or_cookie(self):
        account = "12345678"
        password = "!example secret!"
        checkcode = "4321"
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            raw = root / "raw"
            share = root / "share"
            raw.mkdir()
            (raw / "00-login.html").write_text(
                '<form action="/Self/login/verify">'
                f'<input name="checkcode" value="{checkcode}">'
                '</form>',
                encoding="utf-8",
            )
            (raw / "01-verify.html").write_text(
                f'<p>{account}</p><p>password={password}</p>'
                '<script src="/Self/js/app.js"></script>',
                encoding="utf-8",
            )
            (raw / "00-login.headers").write_text(
                "HTTP/2 200\nSet-Cookie: SESSION=private-cookie\n", encoding="utf-8"
            )
            (raw / "00-login.meta").write_text(
                "http_code=200\nurl_effective=https://jfself.bjut.edu.cn/Self/login/\n",
                encoding="utf-8",
            )
            (raw / "01-verify.meta").write_text(
                "http_code=200\nurl_effective=https://jfself.bjut.edu.cn/Self/home\n",
                encoding="utf-8",
            )
            previous_stdin = jfself.sys.stdin
            try:
                jfself.sys.stdin = io.StringIO(password)
                jfself.sanitize(
                    SimpleNamespace(
                        raw_dir=str(raw),
                        share_dir=str(share),
                        account=account,
                        checkcode=checkcode,
                        route="fixed",
                        password_stdin=True,
                    )
                )
            finally:
                jfself.sys.stdin = previous_stdin
            combined = "\n".join(
                path.read_text(encoding="utf-8")
                for path in share.rglob("*")
                if path.is_file()
            )
            for secret in (account, password, checkcode, "private-cookie"):
                self.assertNotIn(secret, combined)

    def test_sanitizer_reports_additional_authenticated_pages(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            raw = root / "raw"
            share = root / "share"
            raw.mkdir()
            for name, body, url in (
                ("00-login", '<div id="randomDiv" class="hide"></div>', "https://jfself.bjut.edu.cn/Self/login/"),
                ("01-verify", "<main>账户余额</main>", "https://jfself.bjut.edu.cn/Self/dashboard"),
                ("02-bill", "<main>账单查询</main>", "https://jfself.bjut.edu.cn/Self/bill"),
            ):
                (raw / f"{name}.html").write_text(body, encoding="utf-8")
                (raw / f"{name}.meta").write_text(
                    f"http_code=200\nurl_effective={url}\n", encoding="utf-8"
                )
            previous_stdin = jfself.sys.stdin
            try:
                jfself.sys.stdin = io.StringIO("")
                jfself.sanitize(
                    SimpleNamespace(
                        raw_dir=str(raw), share_dir=str(share), account="student",
                        checkcode="123", route="fixed", password_stdin=True,
                    )
                )
            finally:
                jfself.sys.stdin = previous_stdin
            report = jfself.json.loads((share / "report.json").read_text(encoding="utf-8"))
            self.assertEqual(
                [page["file"] for page in report["pages"]],
                ["00-login.html", "01-verify.html", "02-bill.html"],
            )

    def test_resanitizes_an_existing_share_without_raw_credentials(self):
        with tempfile.TemporaryDirectory() as directory:
            share = Path(directory)
            page = share / "page.html"
            page.write_text(
                '<script>window.user={"userRealName":"Private",'
                '"userPassword":"cipher","leftMoney":8.5}</script>',
                encoding="utf-8",
            )
            (share / "report.json").write_text(
                jfself.json.dumps(
                    {
                        "endpointCandidates": [
                            "https://jfself.bjut.edu.cn/Self/bill/getMonthPay",
                            "https://jfself.bjut.edu.cn/not-a-self-endpoint",
                            "http://jfself.bjut.edu.cn/Self/insecure",
                            "https://example.com/Self/foreign",
                        ]
                    }
                ),
                encoding="utf-8",
            )
            jfself.resanitize_shared(SimpleNamespace(share_dir=str(share)))
            result = page.read_text(encoding="utf-8")
            self.assertNotIn("Private", result)
            self.assertNotIn("cipher", result)
            self.assertIn('"leftMoney":8.5', result)
            report = jfself.json.loads(
                (share / "report.json").read_text(encoding="utf-8")
            )
            self.assertEqual(
                report["endpointCandidates"],
                ["https://jfself.bjut.edu.cn/Self/bill/getMonthPay"],
            )

    def test_only_same_origin_script_and_stylesheet_assets_are_collected(self):
        parser = jfself.CaptureParser("https://jfself.bjut.edu.cn/Self/home")
        parser.feed(
            '<script src="/Self/js/app.js?v=1"></script>'
            '<link rel="stylesheet" href="css/main.css">'
            '<script src="https://example.com/foreign.js"></script>'
            '<script src="https://jfself.bjut.edu.cn:444/Self/wrong-port.js"></script>'
            '<script src="https://user:pass@jfself.bjut.edu.cn/Self/private.js"></script>'
        )
        self.assertEqual(
            parser.assets,
            {
                "https://jfself.bjut.edu.cn/Self/js/app.js?v=1",
                "https://jfself.bjut.edu.cn/Self/css/main.css",
            },
        )

    def test_asset_urls_drop_session_path_parameters_after_cookie_setup(self):
        parser = jfself.CaptureParser("https://jfself.bjut.edu.cn/Self/login/")
        parser.feed('<script src="/Self/resources/app.js;jsessionid=secret"></script>')
        self.assertEqual(
            parser.assets,
            {"https://jfself.bjut.edu.cn/Self/resources/app.js"},
        )


if __name__ == "__main__":
    unittest.main()
