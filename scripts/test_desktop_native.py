"""Drive the real Tauri webview and Rust IPC on disposable synthetic data."""
import argparse
import json
import os
import pathlib
import shutil
import subprocess
import tempfile
import time
import urllib.request

from selenium import webdriver
from selenium.common.exceptions import ElementClickInterceptedException, ElementNotInteractableException, StaleElementReferenceException
from selenium.webdriver.common.by import By
from selenium.webdriver.common.options import ArgOptions
from selenium.webdriver.remote.file_detector import UselessFileDetector
from selenium.webdriver.support.ui import WebDriverWait

from test_git import encrypted_files, fixture, inventory
from test_remote import HTTPSFixture, TOKEN, USERNAME
from test_migration import ProviderFixture
from test_new_migration import CreationFixture


class Options(ArgOptions):
    @property
    def default_capabilities(self):
        return {}


def native_driver():
    if os.name != "nt":
        return shutil.which("WebKitWebDriver")
    for path in [os.environ.get("RS_EDGE_DRIVER"), shutil.which("msedgedriver"),
                 str(pathlib.Path(os.environ.get("EDGEWEBDRIVER", "C:/SeleniumWebDrivers/EdgeDriver")) / "msedgedriver.exe"),
                 "C:/WebDrivers/Edge/msedgedriver.exe"]:
        if path and pathlib.Path(path).is_file():
            return path
    raise RuntimeError("Matching WebView2/Edge driver is required for the native Windows test")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("application", type=pathlib.Path)
    parser.add_argument("--screenshot", type=pathlib.Path)
    parser.add_argument("--smoke", action="store_true", help="Only launch and check the installed native UI")
    args = parser.parse_args()
    application = args.application.resolve()
    server = subprocess.Popen(["tauri-driver", "--native-driver", native_driver()])
    driver = None
    try:
        # The intermediary listens before its native child is necessarily
        # ready. Wait for both HTTP endpoints, not just the first TCP socket.
        for _ in range(300):
            try:
                for port in (4445, 4444):
                    with urllib.request.urlopen(f"http://127.0.0.1:{port}/status", timeout=1) as response:
                        assert response.status == 200
                break
            except (OSError, AssertionError):
                assert server.poll() is None, "WebDriver exited; inspect its inherited diagnostics above"
                time.sleep(.1)
        else:
            raise TimeoutError("Native WebDriver did not become ready")
        options = Options()
        options.set_capability("tauri:options", {"application": str(application)})
        # This is a local native driver, not Selenium Grid. Existing backup
        # paths are text input, not files to upload to a remote test machine.
        driver = webdriver.Remote(command_executor="http://127.0.0.1:4444", options=options, file_detector=UselessFileDetector())
        driver.set_window_size(1360, 1040)
        wait = WebDriverWait(driver, 120, ignored_exceptions=(StaleElementReferenceException, ElementClickInterceptedException, ElementNotInteractableException))

        def click(text):
            print("Desktop control:", text, flush=True)
            def attempt(d):
                for element in d.find_elements(By.TAG_NAME, "button"):
                    if (element.get_attribute("aria-label") or element.text).strip() == text and element.is_enabled() and element.is_displayed():
                        element.click()
                        return True
                return False
            wait.until(attempt)

        def fill(label, value):
            # Set through the native prototype (not React's own value tracker)
            # and dispatch input/change, so controlled state receives the edit.
            # This changes DOM input only; real form handlers and Rust IPC run.
            def replace(d):
                element=d.find_element(By.CSS_SELECTOR,f'input[aria-label="{label}"]')
                d.execute_script("""
                    const input=arguments[0],value=arguments[1];
                    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(input,value);
                    input.dispatchEvent(new Event('input',{bubbles:true}));
                    input.dispatchEvent(new Event('change',{bubbles:true}));
                """,element,str(value))
                return element.get_attribute("value")==str(value)
            wait.until(replace, message="Native input did not retain requested value: "+label)

        def status():
            return driver.execute_async_script("const done=arguments[0]; window.__TAURI_INTERNALS__.invoke('get_status').then(done).catch(e=>done({error:String(e)}));")

        last_job = 0

        def completed(expected="completed"):
            nonlocal last_job
            result = wait.until(lambda _: (state if not state["busy"] and state["jobs"][0]["id"] > last_job else False) if (state := status()).get("jobs") else False)
            assert result["jobs"][0]["state"] == expected, result["jobs"][0]
            last_job = result["jobs"][0]["id"]
            print("Completed desktop job:", result["jobs"][0]["label"], flush=True)
            return result

        wait.until(lambda _: driver.find_elements(By.CSS_SELECTOR, ".page-heading h1"))
        assert "Browser preview" not in driver.find_element(By.TAG_NAME, "body").text
        expected_version=json.loads((pathlib.Path(__file__).resolve().parents[1] / "apps/desktop/package.json").read_text())["version"]
        assert status()["version"]==expected_version, "Native/installed app version differs from the requested build"
        if args.smoke:
            assert status()["vault"] is None
            print("Installed native desktop launched with real IPC and a locked vault.")
            return
        parent = pathlib.Path(os.environ.get("RS_ENCRYPTED_PARENT", tempfile.gettempdir()))
        with tempfile.TemporaryDirectory(prefix="rs-native-ui-") as temporary, tempfile.TemporaryDirectory(prefix="rs-native-encrypted-", dir=parent) as encrypted:
            root = pathlib.Path(temporary)
            vault = root / "vault"
            work = pathlib.Path(encrypted) / "workspace"
            source = fixture(root / "source")
            password = "synthetic desktop passphrase"
            click("Create new")
            fill("Vault location", vault); fill("Vault passphrase", password); fill("Confirm passphrase", password)
            click("Create vault"); completed()
            click("Lock vault"); wait.until(lambda _: status()["vault"] is None)
            click("Unlock vault"); fill("Vault location", vault); fill("Vault passphrase", password)
            # The overview also has an Unlock vault action outside the dialog.
            driver.find_element(By.CSS_SELECTOR, '.modal-footer button:not([type])').click(); completed()
            click("Set up workspace"); click("Create workspace")
            fill("Encrypted workspace", work); click("Verify workspace"); completed()
            click("Capture repository"); fill("Snapshot name", "synthetic-desktop-history"); fill("Local repository", source)
            # The background page action is obscured; select the dialog submit.
            driver.find_element(By.CSS_SELECTOR, '.modal-footer button[type="submit"], .modal-footer button:not([type])').click()
            completed()
            assert status()["vault"]["itemCount"] == 1
            click("Repositories")
            click("Restore Git")
            destination = work / "restored.git"
            fill("New bare repository", destination); click("Restore repository"); completed()
            assert inventory(source) == inventory(destination)
            encrypted_files(work)
            with HTTPSFixture(root / "remote-server") as remote:
                click("Capture repository"); click("HTTPS remote")
                fill("Snapshot name", "synthetic-remote-history"); fill("HTTPS clone URL", remote.url)
                click("Username & token"); fill("Repository username", USERNAME); fill("Repository access token", TOKEN)
                fill("Custom CA file (optional)", remote.certificate)
                driver.find_element(By.CSS_SELECTOR, '.modal-footer button:not([type])').click()
                result = completed()
                report = result["jobs"][0]["result"]
                assert report["inventory"]["headSubmodules"] == report["inventory"]["headLfsPointers"] == 1
                assert result["vault"]["itemCount"] == 2
                assert TOKEN not in json.dumps(result)
                click("Activity")
                wait.until(lambda d: any(remote.url in element.text for element in d.find_elements(By.CSS_SELECTOR, ".capture-result")))
                click("Repositories"); click("Capture repository"); click("HTTPS remote")
                fill("Snapshot name", "cancelled-remote"); fill("HTTPS clone URL", remote.url.replace("/project.git", "/slow.git"))
                fill("Custom CA file (optional)", remote.certificate)
                driver.find_element(By.CSS_SELECTOR, '.modal-footer button:not([type])').click()
                wait.until(lambda _: any(path.startswith("/slow.git/") for _, path in remote.requests))
                click("Cancel"); completed("cancelled")
                assert status()["vault"]["itemCount"] == 2
                encrypted_files(work)
            with ProviderFixture(root / "provider-server") as provider_server:
                click("Migration plans")
                for provider_name in ("github", "gitlab"):
                    provider_server.provider=provider_name
                    provider_server.prefix="/platform" if provider_name=="gitlab" else ""
                    provider_server.repo_reads=0
                    from selenium.webdriver.support.ui import Select
                    Select(driver.find_element(By.CSS_SELECTOR, 'select[aria-label="Destination provider"]')).select_by_value(provider_name)
                    box=driver.find_element(By.CSS_SELECTOR, '.plan-form input[type="checkbox"]')
                    if not box.is_selected(): box.click()
                    fill("Plan source snapshot", "synthetic-desktop-history")
                    fill("Destination repository", provider_server.project)
                    fill("Destination instance", provider_server.instance)
                    fill("Destination access token", TOKEN)
                    fill("Destination Git username", USERNAME)
                    fill("Destination CA bundle", provider_server.certificate)
                    click("Prepare read-only plan")
                    result=completed()
                    assert not result["plan"]["blockers"], result["plan"]
                    assert result["plan"]["rows"]>0 and "rows" not in result["jobs"][0]
                    assert result["vault"]["itemCount"]==2
                    assert TOKEN not in json.dumps(result)
                    wait.until(lambda d:d.find_elements(By.CSS_SELECTOR,'section[aria-label="Migration plan result"]'))
                    assert driver.find_element(By.CSS_SELECTOR,'input[aria-label="Destination access token"]').get_attribute("value")==""
                    assert "Write authorization unproven" in driver.find_element(By.TAG_NAME,"body").text
                provider_server.mode="slow-api"
                provider_server.external_requests.clear()
                fill("Destination access token",TOKEN)
                click("Prepare read-only plan")
                wait.until(lambda _:provider_server.external_requests)
                click("Cancel"); completed("cancelled")
                assert status()["plan"] is None, "Cancelled preparation retained an old plan"
                provider_server.mode="normal"
                fill("Destination access token",TOKEN)
                click("Prepare read-only plan"); completed()
                assert status()["plan"] is not None, "Cannot prepare a new plan after cancellation"
                encrypted_files(work)
            if args.screenshot:
                args.screenshot.parent.mkdir(parents=True, exist_ok=True)
                driver.save_screenshot(str(args.screenshot))
            click("Backups"); click("Create backup")
            backup = root / "portable.rsbackup"
            fill("Backup destination", backup)
            driver.find_element(By.CSS_SELECTOR, '.modal-footer button[type="submit"], .modal-footer button:not([type])').click()
            completed(); assert backup.is_file()
            click("Verify backup"); fill("Encrypted backup", backup); fill("Original vault passphrase", password)
            driver.find_element(By.CSS_SELECTOR, '.modal-footer button:not([type])').click(); completed()
            assert status()["plan"] is not None, "Lock test must begin with a populated plan"
            click("Lock vault"); wait.until(lambda _: status()["vault"] is None)
            assert status()["plan"] is None, "Lock retained decrypted migration plan"
            assert all(job["result"] is None for job in status()["jobs"]), "Lock retained decrypted remote provenance"
            click("Restore backup"); fill("Encrypted backup", backup); fill("New vault location", root / "restored-vault"); fill("Original vault passphrase", password)
            click("Restore vault"); completed()
            assert status()["vault"]["itemCount"] == 2
            click("Lock vault"); wait.until(lambda _: status()["vault"] is None)
            click("Overview");click("Unlock vault");fill("Vault location",root / "restored-vault");fill("Vault passphrase",password)
            driver.find_element(By.CSS_SELECTOR,'.modal-footer button:not([type])').click();completed()
            click("New migrations")
            from selenium.webdriver.support.ui import Select
            for provider_name in ("github","gitlab"):
                with CreationFixture(root/("creation-"+provider_name),provider_name) as target:
                    Select(driver.find_element(By.CSS_SELECTOR,'select[aria-label="New migration provider"]')).select_by_value(provider_name)
                    fill("New migration source","synthetic-desktop-history");fill("New migration instance",target.instance)
                    fill("New migration repository",target.project);fill("New migration token",TOKEN)
                    fill("New migration username",USERNAME);fill("New migration CA",target.certificate)
                    click("Prepare private migration");review=completed()
                    assert review["newMigration"]["repository"]==target.project
                    assert TOKEN not in json.dumps(review)
                    fill("Execute migration token",TOKEN);fill("Confirm new repository",target.project)
                    click("Create private repository and migrate");done=completed()
                    assert done["migrationResult"]["state"]=="verified",done["migrationResult"]
                    wait.until(lambda d:d.find_elements(By.CSS_SELECTOR,'section[aria-label="Private migration result"]'))
                    assert len(target.pushes)==2
            click("Lock vault");wait.until(lambda _:status()["vault"] is None)
            assert status()["newMigration"] is None and status()["migrationResult"] is None
            print("Native desktop encrypted archive, guarded personal GitHub/GitLab migrations, receipts, lock privacy and recovery passed.")
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
