"""Drive the real Tauri webview and Rust IPC on disposable synthetic data."""
import argparse
import os
import pathlib
import shutil
import socket
import subprocess
import tempfile
import time

from selenium import webdriver
from selenium.webdriver.common.by import By
from selenium.webdriver.common.options import ArgOptions
from selenium.webdriver.support.ui import WebDriverWait

from test_git import fixture, inventory


class Options(ArgOptions):
    @property
    def default_capabilities(self):
        return {}


def native_driver():
    if os.name != "nt":
        return shutil.which("WebKitWebDriver")
    for path in [shutil.which("msedgedriver"),
                 str(pathlib.Path(os.environ.get("EDGEWEBDRIVER", "C:/SeleniumWebDrivers/EdgeDriver")) / "msedgedriver.exe"),
                 "C:/WebDrivers/Edge/msedgedriver.exe"]:
        if path and pathlib.Path(path).is_file():
            return path
    raise RuntimeError("Matching WebView2/Edge driver is required for the native Windows test")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("application", type=pathlib.Path)
    parser.add_argument("--screenshot", type=pathlib.Path)
    args = parser.parse_args()
    application = args.application.resolve()
    server = subprocess.Popen(["tauri-driver", "--native-driver", native_driver()],
                              stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    driver = None
    try:
        for _ in range(200):
            try:
                with socket.create_connection(("127.0.0.1", 4444), timeout=.2):
                    break
            except OSError:
                assert server.poll() is None, server.stderr.read().decode(errors="replace")
                time.sleep(.1)
        options = Options()
        options.set_capability("tauri:options", {"application": str(application)})
        driver = webdriver.Remote(command_executor="http://127.0.0.1:4444", options=options)
        driver.set_window_size(1360, 1040)
        wait = WebDriverWait(driver, 120)

        def button(text):
            return wait.until(lambda d: next((element for element in d.find_elements(By.TAG_NAME, "button") if element.text.strip() == text and element.is_enabled()), False))

        def fill(label, value):
            element = wait.until(lambda d: d.find_element(By.CSS_SELECTOR, f'input[aria-label="{label}"]'))
            element.clear(); element.send_keys(str(value))

        def status():
            return driver.execute_async_script("const done=arguments[0]; window.__TAURI_INTERNALS__.invoke('get_status').then(done).catch(e=>done({error:String(e)}));")

        last_job = 0

        def completed():
            nonlocal last_job
            result = wait.until(lambda _: (state if not state["busy"] and state["jobs"][0]["id"] > last_job else False) if (state := status()).get("jobs") else False)
            assert result["jobs"][0]["state"] == "completed", result["jobs"][0]
            last_job = result["jobs"][0]["id"]
            return result

        wait.until(lambda _: driver.find_elements(By.CSS_SELECTOR, ".page-heading h1"))
        assert "Browser preview" not in driver.find_element(By.TAG_NAME, "body").text
        parent = pathlib.Path(os.environ.get("RS_ENCRYPTED_PARENT", tempfile.gettempdir()))
        with tempfile.TemporaryDirectory(prefix="rs-native-ui-") as temporary, tempfile.TemporaryDirectory(prefix="rs-native-encrypted-", dir=parent) as encrypted:
            root = pathlib.Path(temporary)
            vault = root / "vault"
            work = pathlib.Path(encrypted) / "workspace"
            source = fixture(root / "source")
            password = "synthetic desktop passphrase"
            button("Create new").click()
            fill("Vault location", vault); fill("Vault passphrase", password); fill("Confirm passphrase", password)
            button("Create vault").click(); completed()
            button("Set up workspace").click(); button("Create workspace").click()
            fill("Encrypted workspace", work); button("Verify workspace").click(); completed()
            button("Capture repository").click(); fill("Snapshot name", "synthetic-desktop-history"); fill("Local repository", source)
            # The background page action is obscured; select the dialog submit.
            driver.find_element(By.CSS_SELECTOR, '.modal-footer button[type="submit"], .modal-footer button:not([type])').click()
            completed()
            assert status()["vault"]["itemCount"] == 1
            button("Repositories").click()
            button("Restore Git").click()
            destination = work / "restored.git"
            fill("New bare repository", destination); button("Restore repository").click(); completed()
            assert inventory(source) == inventory(destination)
            if args.screenshot:
                args.screenshot.parent.mkdir(parents=True, exist_ok=True)
                driver.save_screenshot(str(args.screenshot))
            button("Backups").click(); button("Create backup").click()
            backup = root / "portable.rsbackup"
            fill("Backup destination", backup)
            driver.find_element(By.CSS_SELECTOR, '.modal-footer button[type="submit"], .modal-footer button:not([type])').click()
            completed(); assert backup.is_file()
            button("Lock vault").click(); wait.until(lambda _: status()["vault"] is None)
            button("Restore backup").click(); fill("Encrypted backup", backup); fill("New vault location", root / "restored-vault"); fill("Original vault passphrase", password)
            button("Restore vault").click(); completed()
            assert status()["vault"]["itemCount"] == 1
            button("Lock vault").click(); wait.until(lambda _: status()["vault"] is None)
            print("Native desktop create/unlock, encrypted workspace, capture, Git restore, backup and portable vault recovery passed.")
    finally:
        if driver:
            driver.quit()
        server.terminate()
        try:
            server.wait(timeout=10)
        except subprocess.TimeoutExpired:
            server.kill(); server.wait()


if __name__ == "__main__":
    main()
