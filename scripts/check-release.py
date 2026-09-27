#!/usr/bin/env python3
"""配布ZIPの構成と秘密設定の混入を、内容を表示せずに検査する。"""
import argparse
import hashlib
import json
from pathlib import PurePosixPath
import zipfile


def check_release(path):
    with zipfile.ZipFile(path) as archive:
        names = archive.namelist()
        if len(names) != len(set(names)):
            raise ValueError('ZIPに重複したパスがあります')
        for name in names:
            parts = PurePosixPath(name).parts
            if name.startswith('/') or '..' in parts or '\\' in name:
                raise ValueError('ZIPに不正なパスがあります')
            for part in parts:
                lower = part.lower()
                if lower in {'env', '.env', 'preferences.json', 'paired-host.txt', '.git', 'id_rsa', 'id_ed25519'} or lower.startswith('.env.') or lower.endswith(('.pem', '.key', '.p12', '.pfx', '.log', '.dpapi')):
                    raise ValueError('配布対象外の設定・鍵・ログが含まれています')
        manifests = [n for n in names if n.endswith('/release-manifest.json')]
        if len(manifests) != 1:
            raise ValueError('リリース情報が1件必要です')
        manifest = json.loads(archive.read(manifests[0]))
        if manifest.get('schema_version') != 1 or not manifest.get('version'):
            raise ValueError('リリース情報が不正です')
        root = manifests[0].split('/')[0]
        platform = manifest.get('platform')
        binary = {'macos-arm64': 'Tsunagu.app/Contents/MacOS/Tsunagu', 'windows-x64': 'tsunagu-win.exe'}.get(platform)
        if not binary:
            raise ValueError('未対応の配布先です')
        required = [binary, 'Tsunagu.app/Contents/Info.plist', 'Tsunagu.app/Contents/Resources/AppIcon.icns', 'README-Mac.txt'] if platform == 'macos-arm64' else [binary, 'app.ico', 'install.bat', 'uninstall.bat', 'run_tsunagu.bat', 'run_tsunagu.vbs', 'README-win.txt']
        for file in required:
            if f'{root}/{file}' not in names:
                raise ValueError(f'必須ファイルがありません: {file}')
        actual = hashlib.sha256(archive.read(f'{root}/{binary}')).hexdigest()
        if actual != manifest.get('binary_sha256'):
            raise ValueError('実行ファイルとリリース情報のハッシュが一致しません')
        return {'platform': platform, 'version': manifest['version'], 'files': len(names)}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('archive')
    args = parser.parse_args()
    try:
        result = check_release(args.archive)
    except (ValueError, KeyError, OSError, zipfile.BadZipFile) as error:
        parser.exit(1, f'[release-check] NG: {error}\n')
    print('[release-check] OK: ' + json.dumps(result, ensure_ascii=False))
