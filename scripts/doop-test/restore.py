"""Same-schema database restore inside one positively owned disposable PostgreSQL container."""
import re
import subprocess

MAX_DUMP_BYTES = 16 * 1024 * 1024
SOCKET = "/var/run/postgresql"
ROLE = "doop_fixture"
DATABASE = "doop"


def postgres_id(service):
    service.resources.check_engine()
    item = service.resources.state["resources"].get("postgres")
    if (not item or item["phase"] != "created" or item["kind"] != "container"
            or item["label"] != "io.openbot.private-deployment"
            or item["name"] != service.pg_name or item["owner"] != service.owner
            or not re.fullmatch(r"[0-9a-f]{64}", item["id"] or "")):
        raise RuntimeError("Database restore requires a confirmed owned PostgreSQL container")
    identifier = service.resources.locate(item)
    if identifier != item["id"]:
        raise RuntimeError("Owned PostgreSQL is absent; database restore refused")
    return identifier


def quiet(service, identifier, stage, arguments):
    try:
        service.execute(["docker", "exec", identifier, *arguments],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=15)
    except BaseException as error:
        # Data-tool stderr can contain SQL/COPY contents. Keep only fixed stage/class outcomes.
        failure = "timeout" if isinstance(error, subprocess.TimeoutExpired) else "failed"
        service.last_control_error = "owned-database-restore." + stage + "." + failure
        raise RuntimeError("Owned database restore " + stage + " " + failure) from None


def restore(service):
    identifier = postgres_id(service)
    if service.restore_started:
        raise RuntimeError("Owned database restore is a single-use fixture operation")
    service.restore_started = True
    service.stop()
    if postgres_id(service) != identifier:
        raise RuntimeError("PostgreSQL identity changed after stopping the application")
    path = "/tmp/openbot-" + service.owner + ".dump"
    # The path is a positional shell argument; no credentials or caller text enter shell code.
    quiet(service, identifier, "dump", ["sh", "-c",
        'umask 077; exec pg_dump -h /var/run/postgresql -U doop_fixture -d doop '
        '--format=custom --no-owner --no-privileges --file "$1"', "openbot-backup", path])
    metadata = service.raw_command("exec", identifier, "stat", "-c", "%s %a", path).split()
    if len(metadata) != 2 or not metadata[0].isdigit() or metadata[1] != "600":
        raise RuntimeError("Owned database backup metadata is invalid")
    length = int(metadata[0])
    if not 0 < length <= MAX_DUMP_BYTES:
        raise ValueError("Owned database backup exceeds its fixed16MiB fixture bound")
    checksum = service.raw_command("exec", identifier, "sha256sum", path).split()[0]
    if not re.fullmatch(r"[0-9a-f]{64}", checksum):
        raise RuntimeError("Owned database backup checksum is invalid")
    quiet(service, identifier, "drop", ["dropdb", "-h", SOCKET, "-U", ROLE,
        "--maintenance-db=postgres", DATABASE])
    quiet(service, identifier, "create", ["createdb", "-h", SOCKET, "-U", ROLE,
        "--owner=" + ROLE, DATABASE])
    quiet(service, identifier, "restore", ["pg_restore", "-h", SOCKET, "-U", ROLE,
        "--dbname=" + DATABASE, "--exit-on-error", "--no-owner", "--no-privileges", path])
    quiet(service, identifier, "remove-backup", ["rm", "--", path])
    return {"version": 1, "database": DATABASE, "postgres_container": identifier,
        "backup_bytes": length, "backup_sha256": checksum, "backup_permissions": "0600",
        "same_schema_database_restored": True, "private_asset_store": "retained",
        "storage_mode": service.mode, "image_id": service.image["image_id"]}
