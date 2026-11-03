#!/usr/bin/env python3
"""Build local native packages from the project's source distribution."""
import argparse
import ast
from datetime import datetime, timezone
from email.utils import format_datetime

from pathlib import Path, PurePosixPath
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parent.parent


def project_version(root):
    with (root / "pyproject.toml").open("rb") as stream:
        metadata = tomllib.load(stream)
    project = metadata["project"]
    version = project.get("version")
    if version is None:
        attr = metadata["tool"]["setuptools"]["dynamic"]["version"]["attr"]
        parts = attr.split(".")
        module = root / "src" / Path(*parts[:-1])
        source = module / "__init__.py" if module.is_dir() else module.with_suffix(".py")
        tree = ast.parse(source.read_text(encoding="utf-8"), filename=str(source))
        for node in tree.body:
            if isinstance(node, (ast.Assign, ast.AnnAssign)):
                targets = node.targets if isinstance(node, ast.Assign) else [node.target]
                if any(isinstance(t, ast.Name) and t.id == parts[-1] for t in targets):
                    version = ast.literal_eval(node.value)
                    break
    if not isinstance(version, str) or not re.fullmatch(r"[0-9]+(?:\.[0-9]+)+", version):
        raise ValueError("version must be a numeric dotted release suitable for RPM and Debian")
    return version


def maintainer(name, email):
    if not email or not re.fullmatch(r"[A-Za-z0-9.!#$%&'*+/=?^_`{|}~-]+@[A-Za-z0-9](?:[A-Za-z0-9.-]*[A-Za-z0-9])?\.[A-Za-z]{2,}", email):
        raise ValueError("Debian requires a valid --maintainer-email, including with --prepare-only")
    if not name.strip() or any(c in name for c in "\r\n<>"):
        raise ValueError("invalid maintainer name")
    return f"{name.strip()} <{email}>"


def extract_sdist(archive, destination, expected):
    with tarfile.open(archive, "r:gz") as tar:
        for member in tar.getmembers():
            path = PurePosixPath(member.name)
            if (path.is_absolute() or not path.parts or path.parts[0] != expected
                    or ".." in path.parts or not (member.isdir() or member.isfile())):
                raise ValueError(f"unsafe source archive member: {member.name}")
        tar.extractall(destination)


def run(command, cwd):
    try:
        subprocess.run(command, cwd=cwd, check=True)
    except FileNotFoundError as exc:
        raise ValueError(f"required tool not found: {command[0]}") from exc


def copy_new(source, destination):
    with source.open("rb") as src:
        with destination.open("xb") as dst:
            try:
                shutil.copyfileobj(src, dst)
            except BaseException:
                destination.unlink()
                raise


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("format", choices=("rpm", "deb"))
    parser.add_argument("--maintainer-name", default="Nela")
    parser.add_argument("--maintainer-email")
    parser.add_argument("--output", type=Path, default=Path("dist"))
    parser.add_argument("--prepare-only", action="store_true", help="keep a prepared native build tree without invoking native tools")
    parser.add_argument("--source-only", action="store_true", help="build only the source RPM (rpm only)")
    args = parser.parse_args(argv)
    if args.source_only and args.format != "rpm":
        parser.error("--source-only is only supported for rpm")
    try:
        identity = maintainer(args.maintainer_name, args.maintainer_email) if args.format == "deb" else None
        version = project_version(ROOT)
        output = args.output.resolve()
        output.mkdir(parents=True, exist_ok=True)
        workspace = Path(tempfile.mkdtemp(prefix=f"{args.format}-build-", dir=output))
        prepared = False
        try:
            sdist_dir = workspace / "sdist"
            sdist_dir.mkdir()
            run([sys.executable, "-m", "build", "--sdist", "--no-isolation", "--outdir", str(sdist_dir)], ROOT)
            archive = sdist_dir / f"simp2puml-{version}.tar.gz"
            if not archive.is_file():
                raise ValueError(f"expected source distribution not found: {archive.name}")
            if args.format == "rpm":
                for directory in ("BUILD", "BUILDROOT", "RPMS", "SOURCES", "SPECS", "SRPMS"):
                    (workspace / directory).mkdir()
                shutil.copy2(archive, workspace / "SOURCES" / archive.name)
                spec = workspace / "SPECS" / "simp2puml.spec"
                template = (ROOT / "packaging/rpm/simp2puml.spec.in").read_text(encoding="utf-8")
                if template.count("@VERSION@") != 1:
                    raise ValueError("RPM spec template must contain exactly one @VERSION@")
                spec.write_text(template.replace("@VERSION@", version), encoding="utf-8")
                if not args.prepare_only:
                    run(["rpmbuild", "-bs" if args.source_only else "-ba", "--define", f"_topdir {workspace}", str(spec)], workspace)
                    artifacts = list((workspace / "SRPMS").glob("*.src.rpm"))
                    if not args.source_only:
                        artifacts += list((workspace / "RPMS").rglob("*.rpm"))
            else:
                extract_sdist(archive, workspace, f"simp2puml-{version}")
                source = workspace / f"simp2puml-{version}"
                shutil.copy2(archive, workspace / f"simp2puml_{version}.orig.tar.gz")
                debian = source / "debian"
                shutil.copytree(ROOT / "packaging/debian", debian)
                for name in ("control", "changelog"):
                    template = debian / f"{name}.in"
                    content = template.read_text(encoding="utf-8").replace("@MAINTAINER@", identity).replace("@VERSION@", version).replace("@DATE@", format_datetime(datetime.now(timezone.utc)))
                    (debian / name).write_text(content, encoding="utf-8")
                    template.unlink()
                (debian / "rules").chmod(0o755)
                if not args.prepare_only:
                    run(["dpkg-buildpackage", "-us", "-uc", "-b"], source)
                    artifacts = list(workspace.glob("*.deb"))
            if args.prepare_only:
                prepared = True
                print(f"Prepared build tree: {workspace}")
                return 0
            if not artifacts:
                raise ValueError("native build produced no packages")
            for artifact in artifacts:
                copy_new(artifact, output / artifact.name)
                print(f"Created: {output / artifact.name}")
        finally:
            if not prepared:
                shutil.rmtree(workspace)
        return 0
    except (OSError, ValueError, KeyError, SyntaxError, tarfile.TarError, subprocess.CalledProcessError) as exc:
        print(f"build-native: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
