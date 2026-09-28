#!/usr/bin/env python3
"""Invalid-schema matrix driver (used by p04/p05/...).

Each case = one fresh jail under jails/<prefix>-<case>/ with a project schema `m`
(schema.yaml text + templates given per case) and a change `c1` whose .openspec.yaml
records `schema: m`. For each case, runs:
  oracle: schema validate m | --json | --verbose ; status --change c1 --json ;
          instructions a --change c1 --json ; instructions apply --change c1 --json ;
          schema which m --json ; schemas --json (only the names)
  ours:   schema validate m
Outputs stdout/stderr/rc separately. Usage: matrix.py <prefix> <case-module>
"""
import json, os, shutil, subprocess, sys, importlib.util

W = '/Users/howie/.claude/jobs/9eb90dff/tmp/w7gre'
O = '/Applications/Spectra.app/Contents/MacOS/spectra'
R = '/Users/howie/Workspace/github/heyu-ai/openspectra/.claude/worktrees/w7f-custom-schema-apply/target/release/spectra'
ENV = dict(os.environ, GIT_CONFIG_GLOBAL='/dev/null', GIT_CONFIG_NOSYSTEM='1',
           GIT_AUTHOR_NAME='T', GIT_AUTHOR_EMAIL='t@x', GIT_COMMITTER_NAME='T',
           GIT_COMMITTER_EMAIL='t@x', GIT_AUTHOR_DATE='2026-09-01T00:00:00Z',
           GIT_COMMITTER_DATE='2026-09-01T00:00:00Z')

BASE = """name: m
version: 1
description: test schema
artifacts:
  - id: a
    generates: a.md
    description: A
    template: a.md
    instruction: do a
    requires: []
  - id: b
    generates: b.md
    description: B
    template: b.md
    instruction: do b
    requires:
      - a
apply:
  requires:
    - b
  tracks: b.md
  instruction: go
"""
BASE_TEMPLATES = {'a.md': '# A\n', 'b.md': '# B\n'}


def mkjail(name, schema_yaml, templates, extra=None, schema_dir='m'):
    j = os.path.join(W, 'jails', name)
    shutil.rmtree(j, ignore_errors=True)
    os.makedirs(os.path.join(j, 'openspec/changes/archive'))
    os.makedirs(os.path.join(j, 'openspec/specs'))
    open(os.path.join(j, '.spectra.yaml'), 'w').write('spec_dir: openspec\n')
    open(os.path.join(j, 'openspec/config.yaml'), 'w').write('schema: spec-driven\n')
    os.makedirs(os.path.join(j, 'openspec/changes/c1'))
    open(os.path.join(j, 'openspec/changes/c1/.openspec.yaml'), 'w').write(
        'schema: m\ncreated: 2026-09-01\n')
    sd = os.path.join(j, 'openspec/schemas', schema_dir)
    os.makedirs(sd)
    if schema_yaml is not None:
        open(os.path.join(sd, 'schema.yaml'), 'w').write(schema_yaml)
    if templates is not None:
        os.makedirs(os.path.join(sd, 'templates'))
        for k, v in templates.items():
            p = os.path.join(sd, 'templates', k)
            os.makedirs(os.path.dirname(p), exist_ok=True)
            open(p, 'w').write(v)
    for k, v in (extra or {}).items():
        p = os.path.join(j, k)
        os.makedirs(os.path.dirname(p), exist_ok=True)
        open(p, 'w').write(v)
    subprocess.run(['git', 'init', '-q', '-b', 'main'], cwd=j, env=ENV, check=True)
    subprocess.run(['git', 'add', '-A'], cwd=j, env=ENV, check=True)
    subprocess.run(['git', '-c', 'commit.gpgsign=false', 'commit', '-q', '-m', 'init'],
                   cwd=j, env=ENV, check=True)
    return j


def run(binary, args, cwd):
    p = subprocess.run([binary] + args, cwd=cwd, env=ENV, capture_output=True)
    return p.stdout.decode('utf-8', 'replace'), p.stderr.decode('utf-8', 'replace'), p.returncode


def show(label, binary, args, cwd, brief=False):
    out, err, rc = run(binary, args, cwd)
    print(f'### {label}: {" ".join(args)}  [rc={rc}]')
    if brief:
        lines = out.splitlines()
        if lines:
            print('  stdout(head): ' + ' | '.join(l.strip() for l in lines[:4]) + (' | ...' if len(lines) > 4 else ''))
    else:
        for l in out.splitlines():
            print('  out| ' + l)
    for l in err.splitlines():
        print('  err| ' + l)


def main():
    prefix, mod = sys.argv[1], sys.argv[2]
    spec = importlib.util.spec_from_file_location('cases', mod)
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    only = sys.argv[3:] or None
    for case in m.CASES:
        name = case['name']
        if only and name not in only:
            continue
        print(f'\n########## CASE {name}: {case.get("desc", "")}')
        j = mkjail(f'{prefix}-{name}', case.get('yaml', BASE), case.get('templates', BASE_TEMPLATES),
                   case.get('extra'), case.get('dir', 'm'))
        tgt = case.get('target', 'm')
        show('ORACLE', O, ['schema', 'validate', tgt], j)
        show('ORACLE', O, ['schema', 'validate', tgt, '--json'], j)
        if case.get('verbose'):
            show('ORACLE', O, ['schema', 'validate', tgt, '--verbose'], j)
        if not case.get('validate_only'):
            show('ORACLE', O, ['status', '--change', 'c1', '--json'], j, brief=True)
            show('ORACLE', O, ['instructions', 'a', '--change', 'c1', '--json'], j, brief=True)
            show('ORACLE', O, ['instructions', 'apply', '--change', 'c1', '--json'], j, brief=True)
            out, err, rc = run(O, ['schemas', '--json'], j)
            try:
                names = [s.get('name') for s in json.loads(out)]
            except Exception as e:  # noqa
                names = f'unparseable: {out[:80]!r} {err[:200]!r}'
            print(f'### ORACLE: schemas --json  [rc={rc}] names={names}')
        show('OURS', R, ['schema', 'validate', tgt], j)


main()
