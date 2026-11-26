"""OpenSSH fixture listening on loopback with disposable keys and config."""
from contextlib import contextmanager
import pwd
import sys
import os
from pathlib import Path
import shutil
import socket
import subprocess as sp
import time

@contextmanager
def ssh_server(root):
    root = Path(root)
    sshd = shutil.which("sshd")
    keygen = shutil.which("ssh-keygen")
    assert sshd and keygen
    for filename in ("host", "identity", "wrong"):
        sp.run([keygen, "-q", "-t", "ed25519", "-N", "", "-f", root / filename], check=True)
    (root / "authorized_keys").write_bytes((root / "identity.pub").read_bytes())
    with socket.socket() as reserved:
        reserved.bind(("127.0.0.1", 0))
        port = reserved.getsockname()[1]
    user = pwd.getpwuid(os.getuid()).pw_name
    settings = {
        "ListenAddress": "127.0.0.1", "Port": port, "HostKey": root / "host",
        "AuthorizedKeysFile": root / "authorized_keys", "StrictModes": "no",
        "PasswordAuthentication": "no", "KbdInteractiveAuthentication": "no",
        "PubkeyAuthentication": "yes", "UsePAM": "no", "PidFile": root / "sshd.pid",
        "AllowUsers": user, "LogLevel": "ERROR",
    }
    config = root / "sshd_config"
    config.write_text("".join(f"{key} {value}\n" for key, value in settings.items()))
    for name in ("host", "wrong"):
        key = (root / f"{name}.pub").read_text().split()[:2]
        (root / f"known_{name}").write_text(f"[127.0.0.1]:{port} {' '.join(key)}\n")
    (root / "unknown").touch()
    with (root / "sshd.log").open("wb") as log, sp.Popen(
        [sshd, "-f", config, "-D", "-e"], stderr=log
    ) as server:
        try:
            for _ in range(100):
                if server.poll() is not None:
                    raise RuntimeError((root / "sshd.log").read_text())
                try:
                    with socket.create_connection(("127.0.0.1", port), timeout=0.2):
                        break
                except OSError:
                    time.sleep(0.03)
            else:
                raise RuntimeError("sshd fixture did not listen")
            yield user, port
        finally:
            server.terminate()
            server.wait(timeout=4)

if __name__ == "__main__":
    try:
        with ssh_server(Path(sys.argv[1])) as (user, port):
            print(f"{user} {port}", flush=True)
            sys.stdin.read()
    except (AssertionError, OSError, RuntimeError) as error:
        print(f"OpenSSH fixture unavailable: {error}", file=sys.stderr)
        sys.exit(77)
