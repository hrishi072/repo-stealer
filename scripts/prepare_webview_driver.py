"""Install a Microsoft-signed driver matching the CI WebView2 runtime, not Edge."""
import io
import os
import pathlib
import re
import shutil
import subprocess
import tempfile
import urllib.error
import urllib.request
import zipfile

assert os.name == "nt" and os.environ.get("GITHUB_ACTIONS") == "true"
versions = []
for key in ("ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"):
    base = os.environ.get(key)
    if base:
        for executable in (pathlib.Path(base) / "Microsoft/EdgeWebView/Application").glob("*/msedgewebview2.exe"):
            if re.fullmatch(r"\d+\.\d+\.\d+\.\d+", executable.parent.name):
                versions.append(executable.parent.name)
assert versions, "No installed WebView2 runtime found"
version = max(versions, key=lambda v: tuple(map(int, v.split("."))))
origin = "https://msedgedriver.microsoft.com/"


def download(value):
    with urllib.request.urlopen(origin + value, timeout=60) as response:
        return response.read(32 * 1024 * 1024)


try:
    archive = download(f"{version}/edgedriver_win64.zip")
except urllib.error.HTTPError as error:
    if error.code != 404:
        raise
    data = download(f"LATEST_RELEASE_{version.split('.')[0]}_WINDOWS")
    matched = data.decode("utf-16" if data.startswith((b"\xff\xfe", b"\xfe\xff")) else "utf-8").strip()
    assert re.fullmatch(r"\d+\.\d+\.\d+\.\d+", matched)
    assert matched.split(".")[:3] == version.split(".")[:3], "WebView2/driver builds differ"
    archive = download(f"{matched}/edgedriver_win64.zip")
directory = pathlib.Path(tempfile.mkdtemp(prefix="rs-edge-driver-", dir=os.environ["RUNNER_TEMP"]))
driver = directory / "msedgedriver.exe"
with zipfile.ZipFile(io.BytesIO(archive)) as package:
    driver.write_bytes(package.read("msedgedriver.exe"))
verify = directory / "verify.ps1"
verify.write_text('''param([string]$Driver)
$ErrorActionPreference = 'Stop'
Import-Module Microsoft.PowerShell.Security -ErrorAction Stop
$signature = Get-AuthenticodeSignature -LiteralPath $Driver
if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch 'O=Microsoft Corporation') {
  throw 'The downloaded WebDriver does not have a valid Microsoft signature'
}
''', encoding="utf-8")
# GitHub's PowerShell 7 runner exports its module paths. Launch the same
# PowerShell edition, rather than legacy Windows PowerShell with those paths.
subprocess.run([shutil.which("pwsh") or "powershell.exe", "-NoProfile", "-NonInteractive", "-File", str(verify), str(driver)], check=True)
with open(os.environ["GITHUB_ENV"], "a", encoding="utf-8") as output:
    output.write(f"RS_EDGE_DRIVER={driver}\n")
print("Prepared Microsoft-signed Edge WebDriver for WebView2", version)
