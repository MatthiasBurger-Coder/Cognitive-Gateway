# PostgreSQL with Docker Compose

The repository root [`compose.yaml`](../compose.yaml) runs PostgreSQL 18 from
the official image by default. It mounts the named `postgres_data` volume at
`/var/lib/postgresql`, the data mount used by the PostgreSQL 18 image. The
database and user are both `cognitive_gateway`. The host port defaults to
`127.0.0.1:55432`; other Compose services can use `postgres:5432`.

## Start

On Linux, `./scripts/install-linux.sh --with-postgres` installs the CLIs and
starts the database. To start the database independently:

```bash
./scripts/start-postgres.sh
```

On first start, the script creates
`~/.config/cognitive-gateway/postgres.env` containing a random password and
requires owner-only file permissions. An existing credential file is
preserved. To configure the service manually, copy `.env.example` to that
location, replace its example password, then run the start script. Set
`CG_POSTGRES_ENV_FILE` to use another secure path. The credential file stays
outside the checkout because repositories on Windows-mounted drives may not
support Unix file permissions.

The image can be fetched through `tiny-swarm-nexus-cache` by setting the full
mirror image reference in the credential file:

```text
CG_POSTGRES_IMAGE=<your-nexus-registry>/<path>/postgres:18
```

The mirror must contain the official PostgreSQL 18 image for the configured
platform. `CG_POSTGRES_PORT` changes the host port if `55432` is occupied.
`CG_COMPOSE_PROJECT_NAME` changes the Compose project and therefore the volume
namespace. Keep the project name stable to reuse the same data volume.

## Inspect and stop

Run these commands from the repository root:

```bash
./scripts/postgres-compose.sh ps
./scripts/postgres-compose.sh exec postgres pg_isready -U cognitive_gateway -d cognitive_gateway
./scripts/postgres-compose.sh down
docker volume ls
```

`./scripts/postgres-compose.sh down` leaves the named volume intact. Adding `-v`
removes the volume and its data; use it only when intentionally deleting the
database. Changing the image to another PostgreSQL major version requires a
database upgrade, not just a tag change.

## Direct Compose and IDE launches

Compose interpolates `POSTGRES_PASSWORD` even for `start`. A direct invocation
must pass the same credential file used by the scripts:

```bash
docker compose --env-file ~/.config/cognitive-gateway/postgres.env -f compose.yaml -p cognitive-gateway up -d --wait postgres
```

Configure the IDE's Compose environment file to this path. For a WSL Docker
launcher, use the WSL path. Alternatively, in a WSL checkout, link the existing
credential file as the Git-ignored project `.env` so the default Compose lookup
finds it without copying the password:

```bash
ln -s ~/.config/cognitive-gateway/postgres.env .env
```

If Docker reports that an existing Compose network does not exist, recreate
the project network with `./scripts/postgres-compose.sh down`, then run
`./scripts/start-postgres.sh`. The named data volume is retained by `down`.

## Backup

Before upgrades or volume changes, make a logical backup to a protected
location outside the Compose volume:

```bash
./scripts/postgres-compose.sh exec -T postgres pg_dump -U cognitive_gateway -d cognitive_gateway -Fc > cognitive-gateway.dump
```

The dump may contain sensitive data. `PostgresMemoryStore` persists governed
memory entries and curation decisions; `PostgresExperienceStore` persists
verified-execution references. Both stores are needed for restart-safe pattern
inspection. The memory adapter accepts reference-only payloads, so inline
payload bytes are never stored in this database. A forget decision removes the
reference from the current entry; erasing the referenced content remains the
responsibility of its owning store. Run the integration test with
`./scripts/test-postgres.sh`. `cg patterns --scope <project-scope> --json`
inspects stored experience directly. The CLI does not write verified executions;
callers use the Rust adapter after verifying a runtime outcome.
