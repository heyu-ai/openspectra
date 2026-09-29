# p05: multi-fault ordering (driven by matrix.py, validate only). Which single error wins?
def art(i, req='[]', gen=None, tpl='a.md'):
    return (f"  - id: {i}\n    generates: {gen or i + '.md'}\n    description: {i.upper()}\n"
            f"    template: {tpl}\n    instruction: do {i}\n    requires: {req}\n")


def schema(arts, apply_req='[]', head='name: m\nversion: 1\ndescription: d\n'):
    return head + 'artifacts:\n' + ''.join(arts) + f'apply:\n  requires: {apply_req}\n  tracks: t.md\n  instruction: go\n'


T = {'a.md': '# A\n'}
CASES = [
    dict(name='dup+requnk', desc='dup a (2nd,3rd) + b requires zzz (b before dup)', validate_only=True,
         yaml=schema([art('a'), art('b', '[zzz]'), art('a')]), templates=T),
    dict(name='requnk+dup-order', desc='b requires zzz listed AFTER the duplicate', validate_only=True,
         yaml=schema([art('a'), art('a'), art('b', '[zzz]')]), templates=T),
    dict(name='dup+cycle', desc='dup + cycle b<->c', validate_only=True,
         yaml=schema([art('b', '[c]'), art('c', '[b]'), art('a'), art('a')]), templates=T),
    dict(name='dup+applyunk', desc='dup + apply unknown', validate_only=True,
         yaml=schema([art('a'), art('a')], '[zzz]'), templates=T),
    dict(name='requnk+cycle', desc='cycle a<->b listed first, c requires zzz last', validate_only=True,
         yaml=schema([art('a', '[b]'), art('b', '[a]'), art('c', '[zzz]')]), templates=T),
    dict(name='requnk+applyunk', desc='req unknown + apply unknown', validate_only=True,
         yaml=schema([art('a'), art('b', '[zzz]')], '[yyy]'), templates=T),
    dict(name='cycle+applyunk', desc='cycle + apply unknown', validate_only=True,
         yaml=schema([art('a', '[b]'), art('b', '[a]')], '[yyy]'), templates=T),
    dict(name='two-requnk', desc='a requires x, b requires y', validate_only=True,
         yaml=schema([art('a', '[x]'), art('b', '[y]')]), templates=T),
    dict(name='two-requnk-rev', desc='b(first) requires y, a requires x', validate_only=True,
         yaml=schema([art('b', '[y]'), art('a', '[x]')]), templates=T),
    dict(name='one-art-two-unk', desc='a requires [x, y]', validate_only=True,
         yaml=schema([art('a', '[x, y]')]), templates=T),
    dict(name='two-applyunk', desc='apply requires [x, y]', validate_only=True,
         yaml=schema([art('a')], '[x, y]'), templates=T),
    dict(name='two-cycles', desc='c<->d listed first, a<->b', validate_only=True,
         yaml=schema([art('c', '[d]'), art('d', '[c]'), art('a', '[b]'), art('b', '[a]')]), templates=T),
    dict(name='missing-name-version', desc='no name, no version', validate_only=True,
         yaml=schema([art('a')], head='description: d\n'), templates=T),
    dict(name='missing-version-artifacts', desc='no version, no artifacts', validate_only=True,
         yaml='name: m\ndescription: d\napply:\n  requires: []\n'),
    dict(name='missing-version-only-apply', desc='no version, no apply', validate_only=True,
         yaml='name: m\ndescription: d\nartifacts: []\n'),
    dict(name='art-missing-gen-tpl', desc='artifact missing generates and template', validate_only=True,
         yaml='name: m\nversion: 1\nartifacts:\n  - id: a\n    description: A\n    instruction: i\napply:\n  requires: []\n'),
    dict(name='art-missing-desc-tpl', desc='artifact missing description and template', validate_only=True,
         yaml='name: m\nversion: 1\nartifacts:\n  - id: a\n    generates: a.md\n    instruction: i\napply:\n  requires: []\n'),
    dict(name='art2-missing', desc='second artifact missing id (flow style)', validate_only=True,
         yaml='name: m\nversion: 1\nartifacts:\n  - {id: a, generates: a.md, description: A, template: a.md}\n  - {generates: b.md, description: B, template: b.md}\napply:\n  requires: []\n'),
    dict(name='parse+semantic', desc='missing version AND req unknown', validate_only=True,
         yaml=schema([art('a', '[zzz]')], head='name: m\ndescription: d\n'), templates=T),
    dict(name='dup-field', desc='duplicate top-level key name', validate_only=True,
         yaml='name: m\nname: n\n' + schema([art('a')], head='version: 1\n')[0:], templates=T),
    dict(name='cycle-selfdep-3', desc='a->b->c->a', validate_only=True,
         yaml=schema([art('a', '[c]'), art('b', '[a]'), art('c', '[b]')]), templates=T),
    dict(name='dup-in-requires', desc='b requires [a, a] (control)', validate_only=True,
         yaml=schema([art('a'), art('b', '[a, a]')]), templates=T),
    dict(name='schema-dir-is-file', desc='schema.yaml is a directory', validate_only=True, yaml=None,
         extra={'openspec/schemas/m/schema.yaml/x': 'x'}),
]
