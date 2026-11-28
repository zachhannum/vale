import unittest

import paths

GRAPH = {
    "vale-app": {"vale-globe", "vale-store", "eframe"},
    "vale-globe": {"vale-store", "vale-sphere"},
    "vale-store": set(),
    "vale-sphere": set(),
    "vale-labeler": set(),
    "isotherm-app": {"vale-globe", "isotherm-world"},
    "isotherm-world": set(),
}


class Filters(unittest.TestCase):
    def test_isotherm_gets_the_vale_crates_that_it_uses(self):
        isotherm = paths.filters(GRAPH)["isotherm"]
        # Isotherm uses vale-sphere through vale-globe.
        for name in ("vale-globe", "vale-store", "vale-sphere"):
            self.assertIn(f"crates/{name}/**", isotherm)
        for name in ("vale-app", "vale-labeler", "isotherm-world"):
            self.assertNotIn(f"crates/{name}/**", isotherm)
        self.assertIn("crates/isotherm-*/**", isotherm)
        self.assertIn("Cargo.lock", isotherm)

    def test_vale_gets_all_but_isotherm(self):
        vale = paths.filters(GRAPH)["vale"]
        self.assertEqual(vale[0], "**")
        self.assertIn("!crates/isotherm-*/**", vale)
        self.assertIn("!docs/isotherm/**", vale)

    def test_reads_the_targets_of_a_manifest(self):
        manifest = {"dependencies": {"a": "1"}, "target": {"cfg(ios)": {"dependencies": {"b": "1"}}}}
        self.assertEqual(paths.dependencies(manifest), {"a", "b"})

    def test_reads_the_filter_of_a_workflow(self):
        text = 'on:\n  pull_request:\n    paths:\n      - "**"\n      - "!docs/x/**"\n  push:\n'
        self.assertEqual(paths.workflow_filter(text), ["**", "!docs/x/**"])


class Workflows(unittest.TestCase):
    def test_each_workflow_has_the_filter_of_its_app(self):
        expected = paths.filters(paths.crates())
        for app, names in paths.WORKFLOWS.items():
            for name in names:
                text = (paths.ROOT / ".github" / "workflows" / name).read_text()
                with self.subTest(workflow=name):
                    self.assertEqual(
                        paths.workflow_filter(text),
                        expected[app],
                        f"Run scripts/ci/paths.py and copy the list of {app} into {name}.",
                    )


if __name__ == "__main__":
    unittest.main()
