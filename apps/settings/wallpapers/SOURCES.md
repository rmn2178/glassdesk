# Wallpaper assets

Only the small JPEG previews are bundled in Settings. Selecting a wallpaper
fetches its original from this public repository and caches the verified file.
Opening the gallery does not download originals; cached selections work offline.

## Publishing and adding images

- Commit and push `apps/settings/wallpapers/` to `main` before distributing the
  app that references new images. The downloader uses this directory on
  `raw.githubusercontent.com`; no token or user setup is required.
- Keep published original filenames and bytes unchanged. Append new IDs instead,
  so older app versions retain their verified download URLs.
- Run `python scripts/prepare-wallpapers.py --import-directory <folder>` to append
  JPEGs, generate previews up to 400 × 400 without distortion and update the
  native integrity manifest and frontend catalog. Add the new display names in
  all eight Settings translations. `--check` verifies generated assets.

The 20 originals total **10,113,712 bytes** and are excluded from the executable.
The bundled previews total **270,256 bytes**, with metadata removed and JPEG
quality 82. The actual installer size also depends on code and compression.

## Provenance and integrity

Images 01–12 were downloaded on 2026-11-23 from
[the requested gallery](https://openvid.dev/images/backgrounds/desktop/), without
recompressing the originals. Images 13–20 were supplied by the project maintainer
on 2026-11-24. Their original filenames are recorded below.

| File | Original source/name | Bytes | SHA-256 |
| --- | --- | ---: | --- |
| desktop-01.jpg | openvid.dev / desktop-01.jpg | 60977 | 1f1fbbf15e3b8ba121efaad287d0911abdf2db3bbf50f721caa881675facc7be |
| desktop-02.jpg | openvid.dev / desktop-02.jpg | 29143 | 320209461acba7b71beed1a79151842530df83cf20cc6b33e5f457f1db1f09f5 |
| desktop-03.jpg | openvid.dev / desktop-03.jpg | 192588 | 5e99e172fe4b545f8fa2c743659b577e6380967d242f884c975ae95d9c7bf468 |
| desktop-04.jpg | openvid.dev / desktop-04.jpg | 44823 | c64f72dd33fafe6ab61dc5d6a92cea88f672db2b3f88b284683c8e468012b8d2 |
| desktop-05.jpg | openvid.dev / desktop-05.jpg | 50568 | b971d046074fa05c3f342f29d5af14e4548032b0ffffc76db4b7f4a1094d0d13 |
| desktop-06.jpg | openvid.dev / desktop-06.jpg | 99249 | a54a14a885b0c4ee275c927894b93d0eb9467aa71447283e5387ef30c84d3fa4 |
| desktop-07.jpg | openvid.dev / desktop-07.jpg | 63567 | 4165d8fc4c3056ffe2842ebcae8bee680a9b40f010ec4142e4534625e5b6b59a |
| desktop-08.jpg | openvid.dev / desktop-08.jpg | 54208 | 6eb2aef247a73be1c576d0d0d8ecd3c2957ad6365bf11e2068e67b63971d35fa |
| desktop-09.jpg | openvid.dev / desktop-09.jpg | 136765 | cbeba7964d5850f9dc53787069b091fe6b55e5064d607e89fc431e961fc7eccf |
| desktop-10.jpg | openvid.dev / desktop-10.jpg | 1034545 | 4c8456065b5f51ed43a30271c7c6c3c8c4ccfbcaa0d8859d15c2b9537ee4b4b9 |
| desktop-11.jpg | openvid.dev / desktop-11.jpg | 17350 | 55496164efe3f7dceba85b0f7a742007c76bcd95c7da47bf2796b58a631c5919 |
| desktop-12.jpg | openvid.dev / desktop-12.jpg | 805497 | d64ec224c1a6b3c2aa5360f6deb8fab6706523c1d09110c612846b7e1b0bc17c |
| desktop-13.jpg | puente-de-san-francisco_4k.jpg | 1116810 | 3e562bda256553446d31296ef8d3f3b64b3e35169bee99f33c2ab9ca57e66cc5 |
| desktop-14.jpg | wallhaven-4dl79j.jpg | 781334 | 9a5eeccc2af619fccc3f2c2323fdd22efeb47b9b07a1f4ddde618b3cfb442af7 |
| desktop-15.jpg | wallhaven-72pgxv.jpg | 447496 | 509e5ea32487225983934872d16c5f7759d6e15ff1800fc358f6192c0b8e474a |
| desktop-16.jpg | wallhaven-9mm2mk.jpg | 1659618 | bda1763944c7d81812c0ab8e4ba625bd172e1cce967baa7e91df444f1f86dbc6 |
| desktop-17.jpg | wallhaven-jxv78q.jpg | 773773 | 10034d79cf1ed2fc560d080e647f103de19750483fd70a616a01bd9d2cc7792a |
| desktop-18.jpg | wallhaven-lm38ep.jpg | 1128899 | 017f35b04eb39b997f74957395ec439ae3f583b94df491cfb13c8b39e51212aa |
| desktop-19.jpg | wallhaven-odgmrl.jpg | 1203432 | f1e0922fedac9fd5c060cf2eabaff40cf369ab65f10ea0ffb3db65e91c46a60b |
| desktop-20.jpg | wp7045248-tokyo-night-anime-wallpapers.jpg | 413070 | 37c2fc28a43e784690e9aa35626e42750ad58505fc60b01302df06f8e414aa38 |

Downloads have a five-second connection timeout and thirty-second total timeout,
accept only the fixed HTTPS origin without redirects, and check exact size and
SHA-256 before publishing the cached file. Failed downloads leave the current
Windows wallpaper and prior cache untouched. No shell commands or executables
are downloaded or launched.
