from pathlib import Path
import os
import signal
import subprocess

EXCLUDED = [
    'misc/unicode-90/',   # very slow Unicode normalization tests
    'misc/catalog/',      # W3C meta-tests; catalog-007 hangs, catalog-008 takes 57s
    'decl/function/',     # function-1031 (fib(92) without cache) hangs indefinitely
]

root = Path('vendor/xslt-tests/tests')
files = sorted(root.glob('**/_*-test-set.xml'))
files = [p for p in files if not any(ex in p.as_posix() for ex in EXCLUDED)]
for i, path in enumerate(files, 1):
    print(f'[{i}/{len(files)}] updating {path}', flush=True)
    proc = subprocess.Popen(
        ['cargo', 'run', '--release', '--bin', 'xee-testrunner', '--', 'update', str(path)],
        start_new_session=True,
    )
    try:
        result_code = proc.wait(timeout=30)
    except subprocess.TimeoutExpired:
        os.killpg(proc.pid, signal.SIGKILL)
        proc.wait()
        print(f'TIMEOUT after 30s for {path}, skipping', flush=True)
        continue
    if result_code != 0:
        print(f'command failed for {path} with exit code {result_code}', flush=True)
        raise SystemExit(result_code)
print('done', flush=True)