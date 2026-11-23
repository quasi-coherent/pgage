{
  dockerTools,
  lib,
  postgresql,
  writeShellApplication,
  coreutils,
}:
let
  uid = "999";
  gid = "999";
  pgdata = "/var/lib/postgresql/data";

  pg = postgresql.withPackages (ps: [ ps.age ]);

  entrypoint = writeShellApplication {
    name = "docker-entrypoint";
    runtimeInputs = [
      pg
      coreutils
    ];
    text = ''
      : "''${PGDATA:=${pgdata}}"
      : "''${POSTGRES_USER:=postgres}"
      : "''${POSTGRES_HOST_AUTH_METHOD:=scram-sha-256}"
      export PGDATA

      if [ ! -s "$PGDATA/PG_VERSION" ]; then
        if [ -z "''${POSTGRES_PASSWORD:-}" ] && [ "$POSTGRES_HOST_AUTH_METHOD" != trust ]; then
          echo "error: POSTGRES_PASSWORD must be set (or POSTGRES_HOST_AUTH_METHOD=trust)" >&2
          exit 1
        fi

        mkdir -p "$PGDATA"
        chmod 700 "$PGDATA"

        pwfile="$(mktemp)"
        printf '%s' "''${POSTGRES_PASSWORD:-}" > "$pwfile"
        initdb \
          --username="$POSTGRES_USER" \
          --pwfile="$pwfile" \
          --encoding=UTF8 \
          --locale-provider=builtin \
          --builtin-locale=C.UTF-8 \
          --auth-local=trust \
          --auth-host="$POSTGRES_HOST_AUTH_METHOD"
        rm -f "$pwfile"

        cat >> "$PGDATA/postgresql.conf" <<EOF
      listen_addresses = '*'
      shared_preload_libraries = 'age'
      search_path = 'ag_catalog, "\$user", public'
      EOF
        echo "host all all all $POSTGRES_HOST_AUTH_METHOD" >> "$PGDATA/pg_hba.conf"

        # Install age into template1 (inherited by new databases) and the default db.
        for db in template1 postgres; do
          echo "CREATE EXTENSION IF NOT EXISTS age;" | postgres --single -D "$PGDATA" "$db" >/dev/null
        done
      fi

      exec postgres -D "$PGDATA" "$@"
    '';
  };
in
dockerTools.buildLayeredImage {
  name = "pgage";
  contents = [
    dockerTools.binSh
    pg
    entrypoint
  ];
  extraCommands = ''
    mkdir -p etc tmp run/postgresql ${lib.removePrefix "/" pgdata}
    chmod 1777 tmp
    cat > etc/passwd <<EOF
    root:x:0:0:root:/root:/bin/sh
    postgres:x:${uid}:${gid}:PostgreSQL:/var/lib/postgresql:/bin/sh
    nobody:x:65534:65534:nobody:/var/empty:/bin/false
    EOF
    cat > etc/group <<EOF
    root:x:0:
    postgres:x:${gid}:
    nobody:x:65534:
    EOF
    echo 'hosts: files dns' > etc/nsswitch.conf
  '';
  fakeRootCommands = ''
    chown -R ${uid}:${gid} run/postgresql var/lib/postgresql
  '';
  config = {
    User = "${uid}:${gid}";
    Entrypoint = [ (lib.getExe entrypoint) ];
    Env = [ "PGDATA=${pgdata}" ];
    ExposedPorts."5432/tcp" = { };
    Volumes.${pgdata} = { };
    StopSignal = "SIGINT";
  };
}
