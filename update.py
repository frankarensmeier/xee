from pathlib import Path
import subprocess

EXCLUDED = [
    'misc/unicode-90/',   # very slow Unicode normalization tests
    'misc/catalog/',      # W3C meta-tests; catalog-007 hangs, catalog-008 takes 57s
]

root = Path('vendor/xslt-tests/tests')
files = sorted(root.glob('**/_*-test-set.xml'))
files = [p for p in files if not any(ex in p.as_posix() for ex in EXCLUDED)]
for i, path in enumerate(files, 1):
    print(f'[{i}/{len(files)}] updating {path}', flush=True)
    result = subprocess.run([
        'cargo', 'run', '--release', '--bin', 'xee-testrunner', '--', 'update', str(path)
    ])
    if result.returncode != 0:
        print(f'command failed for {path} with exit code {result.returncode}', flush=True)
        raise SystemExit(result.returncode)
print('done', flush=True)