"""Tests for sidestore.py. Run: python3 -m unittest discover scripts/sidestore"""

import unittest

import sidestore

REPO = "Owner/vale"
PAGES = "https://example.org/vale/"


def release(tag, name, asset, created, size=10):
    return {
        "tag_name": tag,
        "name": name,
        "assets": [
            {
                "name": asset,
                "created_at": created,
                "size": size,
                "browser_download_url": f"https://github.com/{REPO}/releases/download/{tag}/{asset}",
            }
        ],
    }


class Source(unittest.TestCase):
    def test_lists_main_and_each_pr_under_one_bundle_id(self):
        source = sidestore.build_source(
            REPO,
            [
                release("main-build", "main (aaaaaaa)", "Vale-0.0.7.ipa", "2026-10-01T00:00:00Z"),
                release("pr-12", "PR #12: Globe (bbbbbbb)", "Vale-0.12.9.ipa", "2026-10-03T00:00:00Z", 42),
                release("pr-15", "PR #15: Pen (ccccccc)", "Vale-0.15.8.ipa", "2026-10-02T00:00:00Z"),
            ],
            PAGES,
        )
        self.assertEqual(len(source["apps"]), 1)
        app = source["apps"][0]
        self.assertEqual(app["bundleIdentifier"], "dev.vale.app")
        self.assertEqual([v["version"] for v in app["versions"]], ["0.12.9", "0.15.8", "0.0.7"])
        self.assertEqual(app["version"], "0.12.9")
        self.assertEqual(app["size"], 42)
        self.assertEqual(app["downloadURL"], f"https://github.com/{REPO}/releases/download/pr-12/Vale-0.12.9.ipa")
        self.assertEqual(source["sourceURL"], "https://example.org/vale/apps.json")

    def test_ignores_other_releases_and_other_files(self):
        source = sidestore.build_source(
            REPO,
            [
                release("v1.0.0", "Version 1", "Vale-1.0.0.ipa", "2026-10-01T00:00:00Z"),
                release("pr-3", "PR #3", "notes.txt", "2026-10-01T00:00:00Z"),
            ],
            PAGES,
        )
        self.assertEqual(source["apps"], [])


class Comment(unittest.TestCase):
    def pr(self, body, head="Owner/vale"):
        return {"number": 12, "body": body, "head": {"repo": {"full_name": head}}}

    def test_shows_the_install_link_the_commit_and_the_ipad_test(self):
        body = sidestore.comment_body(REPO, self.pr("## iPad test\n\nDraw a line.\n\n## Notes\n\nNo."), "abc1234def", 9, PAGES)
        self.assertTrue(body.startswith(sidestore.MARKER))
        self.assertIn("commit abc1234def", body)
        self.assertIn("https://example.org/vale/?tag=pr-12&file=Vale-0.12.9.ipa", body)
        self.assertIn(f"https://github.com/{REPO}/releases/download/pr-12/Vale-0.12.9.ipa", body)
        self.assertIn("> Draw a line.", body)
        self.assertNotIn("No.", body)
        self.assertNotIn("fork", body)

    def test_warns_about_a_fork(self):
        body = sidestore.comment_body(REPO, self.pr("", head="other/vale"), "abc1234", 9, PAGES)
        self.assertIn("Read its code before you install the build.", body)
        self.assertIn("has no iPad test section", body)


class IpadTest(unittest.TestCase):
    def test_reads_a_paragraph(self):
        body = "Closes #1\n\niPad test: Open the app.\nDraw a line.\n\nDesign reference: x"
        self.assertEqual(sidestore.ipad_test_section(body), "Open the app.\nDraw a line.")

    def test_skips_template_comments(self):
        body = "### iPad test steps\n<!-- Write the steps. -->\n1. Pan.\n#### Detail\nMore.\n## Other"
        self.assertEqual(sidestore.ipad_test_section(body), "1. Pan.\n#### Detail\nMore.")

    def test_no_section(self):
        self.assertEqual(sidestore.ipad_test_section(None), "")


class Version(unittest.TestCase):
    def test_holds_the_pr_and_the_build(self):
        self.assertEqual(sidestore.version("12", "57"), "0.12.57")
        self.assertEqual(sidestore.version("", 57), "0.0.57")
        self.assertEqual(sidestore.tag_of("12"), "pr-12")
        self.assertEqual(sidestore.tag_of(""), "main-build")
        self.assertEqual(sidestore.default_pages_url(REPO), "https://owner.github.io/vale/")


if __name__ == "__main__":
    unittest.main()
