"""Read public fork release metadata. Never download or execute an update."""
import argparse
import json
import urllib.request

API = 'https://api.github.com/repos/kuaan08/Switchrecompz/releases/tags/dev'
PAGE = 'https://github.com/kuaan08/Switchrecompz/releases/tag/dev'
DOWNLOAD = 'https://github.com/kuaan08/Switchrecompz/releases/download/dev/SwitchRecomp.exe'
MAX_RESPONSE = 1024 * 1024


def release_info(release, current):
    if release.get('draft') or release.get('html_url') != PAGE:
        raise ValueError('Unexpected release metadata.')
    asset = next((asset for asset in release.get('assets', [])
                  if asset.get('name') == 'SwitchRecomp.exe' and asset.get('browser_download_url') == DOWNLOAD), None)
    if asset is None:
        raise ValueError('The release has no Windows executable yet. Try again later.')
    commit = release.get('target_commitish', '')
    if not isinstance(commit, str) or len(commit) != 40 or any(c not in '0123456789abcdefABCDEF' for c in commit):
        raise ValueError('The release does not identify its source commit.')
    same = current.removeprefix('dev-').lower() == commit[:7].lower()
    return {'name': str(release.get('name', 'Development build'))[:160], 'current': same,
            'url': PAGE, 'commit': commit}


def main():
    parser = argparse.ArgumentParser(description='Check the public Switchrecompz development release.')
    parser.add_argument('--current', required=True)
    args = parser.parse_args()
    request = urllib.request.Request(API, headers={'User-Agent': 'Switchrecompz-update-check', 'Accept': 'application/vnd.github+json'})
    try:
        with urllib.request.urlopen(request, timeout=15) as response:
            payload = response.read(MAX_RESPONSE + 1)
        if len(payload) > MAX_RESPONSE:
            raise ValueError('Release response is too large.')
        print(json.dumps(release_info(json.loads(payload), args.current)))
    except Exception:
        # No raw network data, proxy credentials or local paths in the UI.
        print(json.dumps({'error': 'Could not check GitHub releases. Check your connection and try again later.'}))
        raise SystemExit(1)


if __name__ == '__main__':
    main()
