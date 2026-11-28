import unittest

import changes

GRAPH = {
    "vale-app": {"vale-globe", "vale-store", "eframe"},
    "vale-globe": {"vale-store", "vale-sphere"},
    "vale-store": set(),
    "vale-sphere": set(),
    "vale-labeler": set(),
    "isotherm-app": {"vale-globe", "isotherm-world"},
    "isotherm-world": set(),
}


class Touched(unittest.TestCase):
    def test_a_vale_crate_touches_vale_only(self):
        self.assertEqual(changes.touched(["crates/vale-app/src/app.rs"], GRAPH), {"vale"})
        self.assertEqual(changes.touched(["crates/vale-labeler/src/lib.rs"], GRAPH), {"vale"})

    def test_an_isotherm_crate_touches_isotherm_only(self):
        self.assertEqual(changes.touched(["crates/isotherm-world/src/lib.rs"], GRAPH), {"isotherm"})
        self.assertEqual(changes.touched(["crates/isotherm-app/ios/project.yml"], GRAPH), {"isotherm"})
        self.assertEqual(changes.touched(["docs/isotherm/DESIGN.md"], GRAPH), {"isotherm"})

    def test_a_shared_crate_touches_both(self):
        self.assertEqual(changes.touched(["crates/vale-globe/src/lib.rs"], GRAPH), {"vale", "isotherm"})
        # Isotherm uses vale-sphere through vale-globe.
        self.assertEqual(changes.touched(["crates/vale-sphere/src/lib.rs"], GRAPH), {"vale", "isotherm"})

    def test_the_workspace_touches_both(self):
        for path in ("Cargo.lock", ".github/workflows/ios.yml", "vendor/winit/src/lib.rs"):
            self.assertEqual(changes.touched([path], GRAPH), {"vale", "isotherm"})

    def test_other_files_touch_vale(self):
        self.assertEqual(changes.touched(["docs/DESIGN.md"], GRAPH), {"vale"})

    def test_reads_the_targets_of_a_manifest(self):
        manifest = {"dependencies": {"a": "1"}, "target": {"cfg(ios)": {"dependencies": {"b": "1"}}}}
        self.assertEqual(changes.dependencies(manifest), {"a", "b"})

    def test_writes_the_list_of_the_apps(self):
        self.assertEqual(changes.output({"isotherm"}), ["vale=false", "isotherm=true", 'apps=["isotherm"]'])
        self.assertEqual(changes.output(set()), ["vale=false", "isotherm=false", "apps=[]"])

    def test_reads_the_workspace(self):
        graph = changes.crates()
        self.assertIn("vale-globe", graph["vale-app"])


if __name__ == "__main__":
    unittest.main()
