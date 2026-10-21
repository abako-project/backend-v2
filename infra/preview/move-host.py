#!/usr/bin/env python3
"""Move the existing preview to dev.abako.xyz with guarded configuration rollback."""

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import time
import urllib.error
import urllib.request


ROOT = Path('/opt/digesta/preview')
BACKUPS = Path('/srv/digesta/preview/backups')
DEV = Path('/etc/nginx/sites-available/abako-backend')
WEB = Path('/etc/nginx/sites-available/web.dev.abako.xyz')
ENV = ROOT / 'deployment.env'
LOG_FORMAT = "log_format digesta_api '$remote_addr $request_method $uri $status';\n"
FILES = {'dev.conf': DEV, 'web.conf': WEB, 'deployment.env': ENV}
COMPOSE = ['docker', 'compose', '--env-file', str(ENV), '-f', str(ROOT / 'compose.yaml')]


def digest(value):
    return hashlib.sha256(value.encode()).hexdigest()


def command(*args):
    subprocess.run(args, check=True, timeout=180)


def verify_cors():
    base = 'https://dev.abako.xyz'
    for origin in [base, 'https://web.dev.abako.xyz']:
        request = urllib.request.Request(base + '/api/workers', method='OPTIONS', headers={
            'Origin': origin, 'Access-Control-Request-Method': 'POST',
            'Access-Control-Request-Headers': 'content-type,x-csrf-token,idempotency-key'})
        # A graceful reload returns before new workers accept every request.
        for attempt in range(20):
            with urllib.request.urlopen(request, timeout=5) as response:
                fields = response.headers
                ready = (response.status == 204 and
                         fields.get_all('Access-Control-Allow-Origin') == [origin] and
                         fields.get_all('Access-Control-Allow-Credentials') == ['true'])
                allowed = ','.join(fields.get_all('Access-Control-Allow-Headers') or []).lower()
            if ready:
                break
            time.sleep(0.25)
        else:
            raise ValueError('exact credentialed CORS preflight failed')
        if not all(name in allowed for name in ['content-type', 'x-csrf-token', 'idempotency-key']):
            raise ValueError('required CORS request headers missing')
    request = urllib.request.Request(base + '/api/catalog', headers={'Origin': 'https://untrusted.invalid'})
    try:
        with urllib.request.urlopen(request, timeout=15):
            raise ValueError('untrusted origin unexpectedly allowed')
    except urllib.error.HTTPError as error:
        if error.code != 403:
            raise ValueError('unexpected origin denial status') from None
    print('PASS: exact credentialed CORS for API/frontend origins and untrusted-origin denial')


def replace_file(path, text):
    metadata = path.stat()
    temporary = path.with_name(path.name + '.digesta-move-new')
    with os.fdopen(os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), 'w') as output:
        output.write(text)
    os.chown(temporary, metadata.st_uid, metadata.st_gid)
    os.chmod(temporary, metadata.st_mode & 0o777)
    os.replace(temporary, path)


def rollback(folder, partial=False):
    folder = folder.resolve(strict=True)
    if folder.parent != BACKUPS or not folder.name.startswith('dev-host-'):
        raise ValueError('rollback requires a recorded hostname-move backup')
    record = json.loads((folder / 'record.json').read_text())
    for name, target in FILES.items():
        previous = (folder / name).read_text()
        if digest(previous) != record['before'][name]:
            raise ValueError('rollback backup hash changed')
        allowed = {record['after'][name]}
        if partial:
            allowed.add(record['before'][name])
        if digest(target.read_text()) not in allowed:
            raise ValueError('live configuration changed; refusing to overwrite later edits')
    for name, target in FILES.items():
        replace_file(target, (folder / name).read_text())
    command(*COMPOSE, 'up', '--detach', '--no-deps', '--wait', '--wait-timeout', '120', 'adapter-api')
    command('nginx', '-t')
    command('systemctl', 'reload', 'nginx')
    print('Hostname configuration restored from:', folder)


def prepare():
    spec = importlib.util.spec_from_file_location('preview_nginx', ROOT / 'install-nginx.py')
    nginx = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(nginx)
    before = {name: path.read_text() for name, path in FILES.items()}
    snippet = (ROOT / 'nginx-api.conf').read_text()
    added = '\n    ' + nginx.MARKER + '\n' + '\n'.join('    ' + line for line in snippet.splitlines()) + '\n'
    web = before['web.conf']
    if web.count(added) != 1 or not web.startswith(LOG_FORMAT):
        raise ValueError('existing web site no longer matches the bounded Digesta addition')
    web = web[len(LOG_FORMAT):].replace(added, '', 1)
    settings = before['deployment.env']
    for name in ['PREVIEW_HOST', 'ALLOWED_ORIGINS']:
        old = name + '=' + ('https://' if name == 'ALLOWED_ORIGINS' else '') + 'web.dev.abako.xyz\n'
        new = old.replace('web.dev.abako.xyz', 'dev.abako.xyz')
        if name == 'ALLOWED_ORIGINS':
            new = new.rstrip('\n') + ',https://web.dev.abako.xyz\n'
        if settings.count(old) != 1:
            raise ValueError('existing hostname settings differ; refusing implicit reconfiguration')
        settings = settings.replace(old, new, 1)
    after = {'dev.conf': nginx.build_site(before['dev.conf'], 'dev.abako.xyz', snippet),
             'web.conf': web, 'deployment.env': settings}
    folder = BACKUPS / ('dev-host-' + time.strftime('%Y%m%d%H%M%S', time.gmtime()))
    folder.mkdir(mode=0o700)
    for name in FILES:
        (folder / name).write_text(before[name])
        (folder / ('candidate-' + name)).write_text(after[name])
    record = {key: {name: digest(text) for name, text in contents.items()}
              for key, contents in [('before', before), ('after', after)]}
    (folder / 'record.json').write_text(json.dumps(record, indent=2))

    sites = folder / 'sites-enabled'
    sites.mkdir(mode=0o700)
    for source in Path('/etc/nginx/sites-enabled').iterdir():
        resolved = source.resolve(strict=True)
        candidate = next((folder / ('candidate-' + name) for name, path in FILES.items()
                          if path == resolved), None)
        (sites / source.name).symlink_to(candidate or source)
    main = Path('/etc/nginx/nginx.conf').read_text()
    include = 'include /etc/nginx/sites-enabled/*;'
    if main.count(include) != 1:
        raise ValueError('unexpected Nginx site include layout')
    validation = folder / 'nginx.conf'
    validation.write_text(main.replace(include, 'include ' + str(sites) + '/*;'))
    command('nginx', '-t', '-c', str(validation))
    print('Validated candidate and private configuration backup:', folder)
    return folder, before, after


def move(apply):
    folder, before, after = prepare()
    if not apply:
        print('Check only: active routes and adapter settings are unchanged.')
        return
    if any(path.read_text() != before[name] for name, path in FILES.items()):
        raise ValueError('configuration changed during validation')
    try:
        for name, path in FILES.items():
            replace_file(path, after[name])
        command('nginx', '-t')
        command(*COMPOSE, 'up', '--detach', '--no-deps', '--wait', '--wait-timeout', '120', 'adapter-api')
        command('systemctl', 'reload', 'nginx')
        verify_cors()
        command('python3', str(ROOT / 'verify-api.py'), '--base-url', 'https://dev.abako.xyz',
                '--origin', 'https://dev.abako.xyz', '--identity-file',
                '/srv/digesta/preview/secrets/smoke-identity.json')
    except Exception:
        rollback(folder, partial=True)
        raise
    (folder / 'verified').write_text('Public HTTPS authentication, signed operation and SSE checks passed.\n')
    print('Verified backend URL: https://dev.abako.xyz/api/')
    print('Guarded rollback: python3 /opt/digesta/preview/move-host.py --rollback', folder)


if __name__ == '__main__':
    if os.geteuid() != 0:
        raise SystemExit('Run on the approved server as root.')
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    modes = parser.add_mutually_exclusive_group()
    modes.add_argument('--apply', action='store_true')
    modes.add_argument('--rollback', type=Path)
    args = parser.parse_args()
    if args.rollback:
        rollback(args.rollback)
    else:
        move(args.apply)
