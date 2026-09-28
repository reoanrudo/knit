import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
import zipfile

spec = importlib.util.spec_from_file_location('release_check', Path(__file__).resolve().parents[1] / 'check-release.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class ReleaseArchiveTests(unittest.TestCase):
    def write_zip(self, path, extra=None, corrupt=False, platform='windows-x64'):
        root = 'Knit-test' if platform == 'macos-arm64' else 'Knit-win-test'
        binary = 'Knit.app/Contents/MacOS/Knit' if platform == 'macos-arm64' else 'knit-win.exe'
        manifest = dict(schema_version=1, version='0.1.0', platform=platform, binary_sha256=hashlib.sha256(b'binary').hexdigest())
        with zipfile.ZipFile(path, 'w') as z:
            z.writestr(f'{root}/{binary}', b'changed' if corrupt else b'binary')
            files = ['Knit.app/Contents/Info.plist', 'Knit.app/Contents/Resources/AppIcon.icns', 'README-Mac.txt'] if platform == 'macos-arm64' else ['app.ico', 'install.bat', 'uninstall.bat', 'run_knit.bat', 'run_knit.vbs', 'README-win.txt']
            for f in files:
                z.writestr(f'{root}/{f}', b'fixture')
            prefix = ''
            z.writestr(f'{root}/{prefix}release-manifest.json', json.dumps(manifest))
            if extra:
                z.writestr(extra, b'private fixture')

    def test_valid_platform_archives(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/'release.zip'
            for platform in ['macos-arm64', 'windows-x64']:
                self.write_zip(p, platform=platform)
                self.assertEqual(module.check_release(p)['platform'], platform)

    def test_rejects_private_settings_and_keys_at_any_depth(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/'release.zip'
            for extra in ['Knit-win-test/.env', 'Knit-win-test/nested/.env.backup', 'Knit-win-test/nested/preferences.json', 'Knit-win-test/private.p12', 'Knit-win-test/app.log', 'Knit-win-test/connection.dpapi', 'Knit-win-test/paired-host.txt']:
                self.write_zip(p, extra=extra)
                with self.assertRaises(ValueError):
                    module.check_release(p)

    def test_rejects_changed_binary(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/'release.zip';self.write_zip(p,corrupt=True)
            with self.assertRaises(ValueError): module.check_release(p)

    def test_rejects_traversal_and_duplicate_entries(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/'release.zip'
            for extra in ['../outside', '/absolute', 'Knit-win-test/knit-win.exe']:
                self.write_zip(p,extra=extra)
                with self.assertRaises(ValueError): module.check_release(p)


if __name__ == '__main__':
    unittest.main()
