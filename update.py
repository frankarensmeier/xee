from pathlib import Path
import subprocess
root = Path('vendor/xslt-tests/tests')
files = sorted(root.glob('**/_*-test-set.xml'))
files = [p for p in files if 'misc/unicode-90/' not in p.as_posix()]
for i, path in enumerate(files, 1):
    print(f'[{i}/{len(files)}] updating {path}', flush=True)
    result = subprocess.run([
        'cargo', 'run', '--release', '--bin', 'xee-testrunner', '--', 'update', str(path)
    ])
    if result.returncode != 0:
        print(f'command failed for {path} with exit code {result.returncode}', flush=True)
        raise SystemExit(result.returncode)
print('done', flush=True)