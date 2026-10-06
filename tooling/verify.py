#!/usr/bin/env python3
"""Canonical CI and pre-commit verification; never formats or fixes source files."""
import argparse
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def run(*command, env=None):
    print('+ ' + ' '.join(command), flush=True)
    subprocess.run(command, cwd=ROOT, env=env, check=True)


def staged_tree():
    run('git', 'diff', '--exit-code', '--quiet')
    untracked = subprocess.check_output(
        ['git', 'ls-files', '--others', '--exclude-standard'], cwd=ROOT, text=True
    )
    if untracked:
        raise SystemExit('Stage intended new files before committing; untracked files could affect verification:\n' + untracked)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('suite', choices=['native', 'reference-and-wasm', 'all'])
    parser.add_argument('--staged', action='store_true')
    args = parser.parse_args()
    if args.staged:
        staged_tree()
    if args.suite in ('native', 'all'):
        run('cargo', 'fmt', '--all', '--check')
        run('cargo', 'clippy', '--workspace', '--all-targets', '--locked', '--', '-D', 'warnings')
        run('cargo', 'test', '--workspace', '--locked')
        run('cargo', 'doc', '--workspace', '--no-deps', '--locked', env={**os.environ, 'RUSTDOCFLAGS': '-D warnings'})
    if args.suite in ('reference-and-wasm', 'all'):
        run('cargo', 'build', '--release', '--locked', '-p', 'aaftool', '-p', 'libaaf-acceptance')
        run('cmake', '-S', 'tooling/reference', '-B', 'tooling/reference-build', '-DCMAKE_BUILD_TYPE=Release', '-DBUILD_DOC=OFF', '-DBUILD_UNIT_TEST=OFF', '-DBUILD_SHARED_LIB=OFF', '-DBUILD_STATIC_LIB=ON')
        run('cmake', '--build', 'tooling/reference-build', '--parallel', '2')
        for script in ('differential', 'cli_differential', 'upstream_tests', 'build_reference_tools', 'inspect_differential'):
            run('python3', f'tooling/{script}.py')
        run('cargo', 'check', '--manifest-path', 'fuzz/Cargo.toml', '--locked')
        run('npm', 'ci', '--prefix', 'web')
        run('npm', 'run', 'build', '--prefix', 'web')
        run('npm', 'test', '--prefix', 'web')
    if args.staged:
        staged_tree()


if __name__ == '__main__':
    main()
