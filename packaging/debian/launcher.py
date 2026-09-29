"""Lanceur de l'application installée (paquet .deb).

Démarre le serveur Streamlit en local sur un port libre, puis l'affiche dans
une fenêtre native (pywebview + WebKitGTK). Fermer la fenêtre arrête le
serveur. Si la fenêtre native est indisponible, ouvre le navigateur par défaut.
"""
import argparse
import ctypes
import os
import signal
import socket
import subprocess
import sys
import time
import urllib.request
import webbrowser

APP_NAME = "dashboard-crypto"
APP_TITLE = "Dashboard Crypto"
INSTALL_DIR = os.path.dirname(os.path.abspath(__file__))
APP_DIR = os.path.join(INSTALL_DIR, "app")
STARTUP_TIMEOUT = 90


def _xdg_dir(env_var, default):
    base = os.environ.get(env_var) or os.path.expanduser(default)
    path = os.path.join(base, APP_NAME)
    os.makedirs(path, exist_ok=True)
    return path


def _free_port():
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def _wait_until_ready(url, server):
    deadline = time.monotonic() + STARTUP_TIMEOUT
    while time.monotonic() < deadline:
        if server.poll() is not None:
            return False
        try:
            with urllib.request.urlopen(f"{url}/_stcore/health", timeout=2) as resp:
                if resp.status == 200:
                    return True
        except OSError:
            pass
        time.sleep(0.3)
    return False


def _die_with_parent():
    """Linux : le serveur reçoit SIGTERM si le lanceur meurt (même tué brutalement)."""
    try:
        ctypes.CDLL("libc.so.6", use_errno=True).prctl(1, signal.SIGTERM)  # PR_SET_PDEATHSIG
    except OSError:
        pass


def _start_server(port, log_file):
    env = dict(os.environ)
    env["DASHBOARD_CRYPTO_CONFIG"] = os.path.join(_xdg_dir("XDG_CONFIG_HOME", "~/.config"), "config.ini")
    env["PYTHONDONTWRITEBYTECODE"] = "1"
    cmd = [
        sys.executable, "-m", "streamlit", "run", "app.py",
        "--server.headless=true",
        "--server.address=127.0.0.1",
        f"--server.port={port}",
        "--server.fileWatcherType=none",
        "--browser.gatherUsageStats=false",
        "--global.developmentMode=false",
        "--client.toolbarMode=minimal",
    ]
    return subprocess.Popen(cmd, cwd=APP_DIR, env=env, stdout=log_file, stderr=subprocess.STDOUT,
                            preexec_fn=_die_with_parent)


def _stop_server(server):
    if server.poll() is None:
        server.terminate()
        try:
            server.wait(timeout=10)
        except subprocess.TimeoutExpired:
            server.kill()


def _show_window(url):
    """Affiche l'URL dans une fenêtre native ; retourne False si impossible."""
    try:
        import webview
    except ImportError:
        return False
    try:
        webview.create_window(APP_TITLE, url, width=1400, height=900, min_size=(900, 600))
        webview.start(gui="gtk")
    except Exception as e:
        print(f"Fenêtre native indisponible ({e}), ouverture dans le navigateur.", file=sys.stderr)
        return False
    return True


def main():
    parser = argparse.ArgumentParser(prog=APP_NAME, description=APP_TITLE)
    parser.add_argument("--browser", action="store_true",
                        help="ouvre le navigateur par défaut au lieu d'une fenêtre native")
    parser.add_argument("--check", action="store_true",
                        help="démarre le serveur, vérifie qu'il répond, puis quitte (test d'installation)")
    args = parser.parse_args()

    port = _free_port()
    url = f"http://127.0.0.1:{port}"
    log_path = os.path.join(_xdg_dir("XDG_CACHE_HOME", "~/.cache"), "streamlit.log")

    with open(log_path, "w") as log_file:
        server = _start_server(port, log_file)

        def _on_sigterm(signum, frame):
            # Fermeture de session / kill : une exception levée ici serait
            # avalée par la boucle GTK, on arrête donc le serveur puis on sort.
            _stop_server(server)
            os._exit(0)

        signal.signal(signal.SIGTERM, _on_sigterm)
        try:
            if not _wait_until_ready(url, server):
                print(f"Le serveur n'a pas démarré. Journal : {log_path}", file=sys.stderr)
                return 1
            if args.check:
                print(f"OK : serveur prêt sur {url}")
                return 0
            if args.browser or not _show_window(url):
                webbrowser.open(url)
                print(f"{APP_TITLE} tourne sur {url} (Ctrl+C pour arrêter).")
                server.wait()
            return 0
        except KeyboardInterrupt:
            return 0
        finally:
            _stop_server(server)


if __name__ == "__main__":
    sys.exit(main())
