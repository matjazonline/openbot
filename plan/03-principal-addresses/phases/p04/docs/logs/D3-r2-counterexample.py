"""One-off retained counterfactual; normal regression tests never depend on this file."""
from pathlib import Path
import runpy
import tempfile

root = Path.cwd()
fixtures = runpy.run_path(str(root / "scripts/tests/network-isolation-scope.py"), run_name="counterexample")
before = fixtures["owned_resources"]()
with tempfile.TemporaryDirectory(prefix="postmutation-counterexample-") as temporary:
    directory = Path(temporary)
    wrapper = fixtures["extracted_wrapper"](directory / "old-root")
    helper = directory / "old-root/services/memory/scripts/network-isolation-linux.sh"
    helper.write_text(Path(__file__).with_name("D3-r2-old-helper.sh").read_text())
    case = directory / "case"
    case.mkdir()
    try:
        fixtures["after_effect_case"](wrapper, case, "post-error", "groupadd")
    except AssertionError as error:
        assert str(error) == "post-mutation acquisition leaked resources", str(error)
        print("EXPECTED DEFECT REPRODUCED: original post-success ownership leaked an actually created group", flush=True)
    else:
        raise AssertionError("old ownership helper unexpectedly passed the post-mutation case")
    finally:
        assert fixtures["owned_resources"]() == before, "counterexample teardown leaked resources"
