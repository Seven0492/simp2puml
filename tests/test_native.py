import importlib.util
import io
from pathlib import Path
import tarfile

import pytest


SCRIPT = Path(__file__).resolve().parents[1] / "tools" / "build-native.py"
spec = importlib.util.spec_from_file_location("build_native", SCRIPT)
native = importlib.util.module_from_spec(spec)
spec.loader.exec_module(native)


def test_dynamic_version_is_read_without_executing_package(tmp_path):
    package = tmp_path / "src" / "simp2puml"
    package.mkdir(parents=True)
    (package / "__init__.py").write_text('__version__ = "0.1.0"\nraise RuntimeError("executed")\n')
    (tmp_path / "pyproject.toml").write_text(
        '[project]\ndynamic = ["version"]\n'
        '[tool.setuptools.dynamic]\nversion = {attr = "simp2puml.__version__"}\n'
    )
    assert native.project_version(tmp_path) == "0.1.0"


def test_debian_requires_email_before_build(tmp_path, capsys):
    assert native.main(["deb", "--prepare-only", "--output", str(tmp_path / "out")]) == 1
    assert "--maintainer-email" in capsys.readouterr().err
    assert not (tmp_path / "out").exists()


@pytest.mark.parametrize("email", ["nela", "nela@example", "a@example.org\nInjected: yes"])
def test_invalid_maintainer_email(email):
    with pytest.raises(ValueError):
        native.maintainer("Nela", email)


def test_copy_never_overwrites(tmp_path):
    source = tmp_path / "source"
    target = tmp_path / "target"
    source.write_text("new")
    target.write_text("existing")
    with pytest.raises(FileExistsError):
        native.copy_new(source, target)
    assert target.read_text() == "existing"


def test_source_only_rpm_command_and_existing_artifact(tmp_path, monkeypatch):
    version = native.project_version(native.ROOT)
    calls = []

    def fake_run(command, cwd):
        calls.append(command)
        if command[0] == "rpmbuild":
            assert command[1] == "-bs"
            spec_text = Path(command[-1]).read_text()
            assert f"Version:        {version}" in spec_text
            assert "@VERSION@" not in spec_text
            (cwd / "SRPMS" / "result.src.rpm").write_bytes(b"new")
        else:
            (Path(command[-1]) / f"simp2puml-{version}.tar.gz").write_bytes(b"sdist")

    monkeypatch.setattr(native, "run", fake_run)
    existing = tmp_path / "result.src.rpm"
    existing.write_bytes(b"existing")
    assert native.main(["rpm", "--source-only", "--output", str(tmp_path)]) == 1
    assert existing.read_bytes() == b"existing"
    assert len(calls) == 2
    existing.unlink()
    assert native.main(["rpm", "--source-only", "--output", str(tmp_path)]) == 0
    assert existing.read_bytes() == b"new"


def test_source_only_rejected_for_debian(tmp_path, capsys):
    with pytest.raises(SystemExit) as exc:
        native.main(["deb", "--source-only", "--output", str(tmp_path / "out")])
    assert exc.value.code == 2
    assert "only supported for rpm" in capsys.readouterr().err
    assert not (tmp_path / "out").exists()


def test_missing_native_tool_reported(tmp_path, monkeypatch):
    def missing(*args, **kwargs):
        raise FileNotFoundError("missing")

    monkeypatch.setattr(native.subprocess, "run", missing)
    with pytest.raises(ValueError, match="required tool not found: rpmbuild"):
        native.run(["rpmbuild", "-bs"], tmp_path)


@pytest.mark.parametrize("name,kind", [("simp2puml-0.1.0/../../escape", "file"), ("simp2puml-0.1.0/link", "symlink")])
def test_rejects_unsafe_archive(tmp_path, name, kind):
    archive = tmp_path / "source.tar.gz"
    with tarfile.open(archive, "w:gz") as tar:
        info = tarfile.TarInfo(name)
        if kind == "symlink":
            info.type = tarfile.SYMTYPE
            info.linkname = "../../escape"
            tar.addfile(info)
        else:
            info.size = 1
            tar.addfile(info, io.BytesIO(b"x"))
    with pytest.raises(ValueError, match="unsafe"):
        native.extract_sdist(archive, tmp_path, "simp2puml-0.1.0")
