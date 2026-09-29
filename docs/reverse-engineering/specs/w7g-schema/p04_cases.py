# p04: single-fault invalid-schema matrix (driven by matrix.py). Each case mutates BASE once.
DUP_A = """  - id: a
    generates: c.md
    description: C
    template: a.md
    instruction: do c
    requires: []
"""
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


def drop(line, text=BASE, count=1):
    assert line in text, line
    return text.replace(line, '', count)


def sub(old, new, text=BASE, count=1):
    assert old in text, old
    return text.replace(old, new, count)


CASES = [
    dict(name='valid', desc='control: BASE is valid', verbose=True),
    dict(name='yaml-syntax', desc='invalid YAML syntax', yaml='name: m\nartifacts: [\n  - id: a\n', verbose=True),
    dict(name='yaml-tab', desc='tab indentation', yaml=sub('    generates: a.md\n', '\tgenerates: a.md\n')),
    dict(name='empty-file', desc='schema.yaml empty', yaml=''),
    dict(name='no-name', desc='missing top-level name', yaml=drop('name: m\n')),
    dict(name='no-version', desc='missing version', yaml=drop('version: 1\n')),
    dict(name='no-description', desc='missing description', yaml=drop('description: test schema\n')),
    dict(name='no-artifacts', desc='missing artifacts', yaml=BASE[:BASE.index('artifacts:')] + BASE[BASE.index('apply:'):]),
    dict(name='no-apply', desc='missing apply', yaml=BASE[:BASE.index('apply:')]),
    dict(name='art-no-id', desc='artifact a missing id', yaml=sub('  - id: a\n    generates: a.md', '  - generates: a.md')),
    dict(name='art-no-generates', desc='artifact a missing generates', yaml=drop('    generates: a.md\n')),
    dict(name='art-no-description', desc='artifact a missing description', yaml=drop('    description: A\n')),
    dict(name='art-no-template', desc='artifact a missing template', yaml=drop('    template: a.md\n')),
    dict(name='art-no-instruction', desc='artifact a missing instruction', yaml=drop('    instruction: do a\n')),
    dict(name='art-no-requires', desc='artifact a missing requires', yaml=drop('    requires: []\n')),
    dict(name='unknown-top-key', desc='unknown top-level key', yaml=BASE + 'foo: 1\n'),
    dict(name='unknown-art-key', desc='unknown artifact key', yaml=sub('    requires: []\n', '    requires: []\n    bar: 1\n')),
    dict(name='unknown-apply-key', desc='unknown apply key', yaml=BASE + '  baz: 1\n'),
    dict(name='dup-id', desc='third artifact with duplicate id a', yaml=sub('apply:\n', DUP_A + 'apply:\n')),
    dict(name='req-unknown', desc='b requires unknown artifact', yaml=sub('      - a\napply', '      - zzz\napply')),
    dict(name='cycle', desc='a requires b, b requires a', yaml=sub('    requires: []\n', '    requires: [b]\n')),
    dict(name='self-dep', desc='a requires a', yaml=sub('    requires: []\n', '    requires: [a]\n')),
    dict(name='apply-unknown', desc='apply.requires unknown', yaml=sub('    - b\n  tracks', '    - zzz\n  tracks')),
    dict(name='apply-empty', desc='apply.requires []', yaml=sub('  requires:\n    - b\n  tracks', '  requires: []\n  tracks')),
    dict(name='apply-no-requires', desc='apply without requires', yaml=sub('  requires:\n    - b\n  tracks', '  tracks')),
    dict(name='apply-no-tracks', desc='apply without tracks', yaml=drop('  tracks: b.md\n')),
    dict(name='apply-no-instruction', desc='apply without instruction', yaml=drop('  instruction: go\n')),
    dict(name='tpl-missing', desc='templates/b.md absent', templates={'a.md': '# A\n'}),
    dict(name='tpl-empty', desc='templates/b.md empty', templates={'a.md': '# A\n', 'b.md': ''}),
    dict(name='tpl-dir-missing', desc='no templates/ dir at all', templates=None),
    dict(name='tpl-traversal', desc='template ../x.md (file exists at schemas/m/x.md)', yaml=sub('    template: b.md\n', '    template: ../x.md\n'),
         extra={'openspec/schemas/m/x.md': '# X\n'}),
    dict(name='tpl-traversal-missing', desc='template ../nope.md (absent)', yaml=sub('    template: b.md\n', '    template: ../nope.md\n')),
    dict(name='tpl-absolute', desc='template absolute path to existing file',
         yaml=sub('    template: b.md\n', '    template: /Users/howie/.claude/jobs/9eb90dff/tmp/w7gre/jails/p04-tpl-absolute/abs.md\n'),
         extra={'abs.md': '# ABS\n'}),
    dict(name='tpl-subdir', desc='template sub/b.md existing', yaml=sub('    template: b.md\n', '    template: sub/b.md\n'), templates={'a.md': '# A\n', 'sub/b.md': '# B\n'}),
    dict(name='gen-absolute', desc='generates absolute', yaml=sub('    generates: b.md\n', '    generates: /abs/b.md\n')),
    dict(name='gen-traversal', desc='generates ../b.md', yaml=sub('    generates: b.md\n', '    generates: ../b.md\n')),
    dict(name='gen-glob', desc='generates specs/**/*.md', yaml=sub('    generates: b.md\n', '    generates: specs/**/*.md\n')),
    dict(name='empty-artifacts', desc='artifacts: [] (apply.requires [])', yaml='name: m\nversion: 1\ndescription: d\nartifacts: []\napply:\n  requires: []\n  tracks: t.md\n  instruction: go\n'),
    dict(name='name-mismatch', desc='name: other in dir m', yaml=sub('name: m\n', 'name: other\n'), verbose=True),
    dict(name='no-schema-yaml', desc='schemas/m/ has templates but no schema.yaml', yaml=None),
    dict(name='schema-yml', desc='schema.yml instead of schema.yaml', yaml=None, extra={'openspec/schemas/m/schema.yml': BASE}),
    dict(name='version-str', desc='version: "x"', yaml=sub('version: 1\n', 'version: x\n')),
    dict(name='version-2', desc='version: 2', yaml=sub('version: 1\n', 'version: 2\n')),
    dict(name='version-0', desc='version: 0', yaml=sub('version: 1\n', 'version: 0\n')),
    dict(name='name-empty', desc="name: ''", yaml=sub('name: m\n', "name: ''\n")),
    dict(name='name-list', desc='name: [x]', yaml=sub('name: m\n', 'name: [x]\n')),
    dict(name='requires-string', desc='requires: a (scalar)', yaml=sub('    requires:\n      - a\napply', '    requires: a\napply')),
    dict(name='artifacts-map', desc='artifacts is a mapping', yaml='name: m\nversion: 1\ndescription: d\nartifacts:\n  a: 1\napply:\n  requires: []\n  tracks: t.md\n  instruction: go\n'),
    dict(name='id-empty', desc="artifact id ''", yaml=sub('  - id: b\n', "  - id: ''\n").replace('    - b\n  tracks', "    - ''\n  tracks")),
    dict(name='id-weird', desc="artifact id 'B C/..'", yaml=sub('  - id: b\n', "  - id: 'B C/..'\n").replace('    - b\n  tracks', "    - 'B C/..'\n  tracks")),
    dict(name='description-null', desc='description: ~', yaml=sub('description: test schema\n', 'description: ~\n')),
    dict(name='instruction-multiline', desc='instruction block scalar (control)', yaml=sub('    instruction: do a\n', '    instruction: |\n      line1\n      line2\n')),
]
