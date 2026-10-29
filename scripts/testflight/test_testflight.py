"""Tests for testflight.py. Run: python3 -m unittest discover scripts/testflight"""

import base64
import json
import shutil
import subprocess
import tempfile
import io
import unittest
import urllib.error
from pathlib import Path
from unittest import mock

import testflight


class WhatToTest(unittest.TestCase):
    def test_shows_the_pr_and_the_ipad_test(self):
        text = testflight.what_to_test("12", "Globe", "## iPad test\n\nDraw a line.\n\n## Notes\n\nNo.", "abc1234def")
        self.assertEqual(text, "PR #12: Globe\n\nDraw a line.")

    def test_reads_a_paragraph(self):
        body = "Closes #1\n\niPad test: Open the app.\nDraw a line.\n\nDesign reference: x"
        self.assertEqual(testflight.test_section(body), "Open the app.\nDraw a line.")

    def test_reads_an_iphone_paragraph(self):
        body = "Closes #1\n\niPhone test: Turn the globe.\n"
        self.assertEqual(testflight.test_section(body), "Turn the globe.")

    def test_reads_both_devices(self):
        body = "iPad test: Draw a line.\n\niPhone test: Turn the globe."
        self.assertEqual(
            testflight.test_section(body),
            "iPad test:\nDraw a line.\n\niPhone test:\nTurn the globe.",
        )

    def test_skips_template_comments(self):
        body = "### iPad test steps\n<!-- Write the steps. -->\n1. Pan.\n#### Detail\nMore.\n## Other"
        self.assertEqual(testflight.test_section(body), "1. Pan.\n#### Detail\nMore.")

    def test_no_section(self):
        self.assertIn("has no iPad or iPhone test section", testflight.what_to_test("12", "Globe", None, "abc1234"))

    def test_main_shows_the_commit(self):
        self.assertEqual(testflight.what_to_test("", "", "", "abc1234def"), "main at commit abc1234")

    def test_stays_inside_the_limit(self):
        text = testflight.what_to_test("12", "Globe", "iPad test: " + "x" * 5000, "abc1234")
        self.assertEqual(len(text), testflight.NOTES_LIMIT)


class Comment(unittest.TestCase):
    def test_shows_the_build_number_the_commit_and_the_state(self):
        for state, sentence in testflight.STATES.items():
            body = testflight.comment_body(state, "0.1.12", "57.1", "abc1234def")
            self.assertTrue(body.startswith(testflight.MARKER))
            self.assertIn("TestFlight build 57.1 of commit abc1234def, version 0.1.12", body)
            self.assertIn(sentence, body)

    def test_a_failure_links_to_the_run(self):
        body = testflight.comment_body("failed", "0.1.12", "57.1", "abc1234", "https://example.org/run")
        self.assertIn("(https://example.org/run)", body)


class Token(unittest.TestCase):
    def test_der_to_raw_pads_and_strips(self):
        r = bytes([0x00, 0x80]) + bytes(31)  # 33 bytes with a sign byte
        s = bytes([0x01]) * 31  # 31 bytes
        der = bytes([0x30, 4 + len(r) + len(s), 0x02, len(r)]) + r + bytes([0x02, len(s)]) + s
        raw = testflight.der_to_raw(der)
        self.assertEqual(len(raw), 64)
        self.assertEqual(raw[:32], r[1:])
        self.assertEqual(raw[32:], bytes(1) + s)

    @unittest.skipUnless(shutil.which("openssl"), "needs openssl")
    def test_token_has_the_claims_and_a_64_byte_signature(self):
        with tempfile.TemporaryDirectory() as tmp:
            key = Path(tmp) / "key.p8"
            subprocess.run(
                ["openssl", "genpkey", "-algorithm", "EC", "-pkeyopt", "ec_paramgen_curve:P-256", "-out", str(key)],
                check=True,
                capture_output=True,
            )
            header, claims, signature = testflight.token(str(key), "KEY", "ISSUER", now=1000).split(".")

        def decode(part):
            return base64.urlsafe_b64decode(part + "=" * (-len(part) % 4))

        self.assertEqual(json.loads(decode(header)), {"alg": "ES256", "kid": "KEY", "typ": "JWT"})
        self.assertEqual(
            json.loads(decode(claims)), {"iss": "ISSUER", "iat": 1000, "exp": 1600, "aud": "appstoreconnect-v1"}
        )
        self.assertEqual(len(decode(signature)), 64)


class Request(unittest.TestCase):
    def connect(self):
        with mock.patch.dict("os.environ", {"ASC_KEY_PATH": "k", "ASC_KEY_ID": "i", "ASC_ISSUER_ID": "s"}):
            return testflight.Connect()

    def error(self, code):
        return urllib.error.HTTPError("url", code, "error", {}, io.BytesIO(b"{}"))

    def ok(self):
        response = mock.MagicMock()
        response.__enter__.return_value.read.return_value = b'{"data": []}'
        return response

    @mock.patch("testflight.time.sleep")
    @mock.patch("testflight.token", return_value="t")
    def test_a_server_error_gets_another_try(self, _token, sleep):
        with mock.patch("urllib.request.urlopen", side_effect=[self.error(500), self.ok()]) as urlopen:
            self.assertEqual(self.connect().request("GET", "/v1/builds"), {"data": []})
        self.assertEqual(urlopen.call_count, 2)
        sleep.assert_called_once_with(testflight.RETRY_WAITS[0])

    @mock.patch("testflight.time.sleep")
    @mock.patch("testflight.token", return_value="t")
    def test_a_client_error_stops_at_once(self, _token, sleep):
        with mock.patch("urllib.request.urlopen", side_effect=[self.error(404)]):
            with self.assertRaises(SystemExit):
                self.connect().request("GET", "/v1/builds")
        sleep.assert_not_called()

    @mock.patch("testflight.time.sleep")
    @mock.patch("testflight.token", return_value="t")
    def test_server_errors_stop_after_the_last_try(self, _token, sleep):
        tries = len(testflight.RETRY_WAITS) + 1
        with mock.patch("urllib.request.urlopen", side_effect=[self.error(500)] * tries) as urlopen:
            with self.assertRaises(SystemExit):
                self.connect().request("GET", "/v1/builds")
        self.assertEqual(urlopen.call_count, tries)
        self.assertEqual(sleep.call_count, tries - 1)


if __name__ == "__main__":
    unittest.main()
