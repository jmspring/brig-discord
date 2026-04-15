---
id: bd-p0rt
status: done
deps: []
links: []
created: 2026-04-14T12:00:00Z
type: feature
priority: 2
assignee: Jim Spring
tags: [packaging, freebsd, ports]
---
# FreeBSD port infrastructure for brig-discord

## Goal

Add BSD Makefile, port skeleton, and optional rc.d script so that brig-discord
can be installed following FreeBSD conventions.  The default install path assumes
jailed mode (managed by brig).  Host-mode operation is opt-in via a separate
make target.

## Context

brig-discord is a ~500-line synchronous Discord gateway that bridges bot messages
to brig's unix domain socket.  It currently has no Makefile or install target —
the README documents a manual `cargo build && cp` workflow followed by
`brig skill add ./` and `brig skill enable`.

brig itself already has a BSD Makefile (`brig/Makefile`) and an rc.d script
(`brig/scripts/rc.d/brig`) that serve as the template for this work.

### Two deployment modes

1. **Jailed (default)** — brig manages the gateway inside a FreeBSD jail.
   The port installs the binary and manifest; the operator runs
   `brig skill enable discord-gateway` which creates the ZFS dataset,
   jail.conf, pf rules, and its own rc.d script.  The binary at
   `/usr/local/bin/brig-discord` is nullfs-mounted read-only into the jail.

2. **Host-mode (opt-in)** — the gateway runs directly on the host as a
   standard FreeBSD service.  An rc.d script is installed via
   `make install-service`.  The operator configures rc.conf variables and
   uses `service brig_discord start`.  No jail, no pf, no ZFS — just the
   binary talking to brig's unix socket.

### Existing manifest.toml

The manifest already exists at `brig-discord/manifest.toml`:

```toml
[skill]
name = "discord-gateway"
description = "Bridge Discord messages to Brig"
kind = "persistent"

[requires]
network = ["discord.com", "gateway.discord.gg"]
max_runtime = "forever"

[persistent]
rc_name = "brig_discord"
entrypoint = "/usr/local/bin/brig-discord"
restart_on_failure = true
depends_on = ["brig"]

[persistent.socket]
capabilities = ["submit_task", "read_status"]

[secrets]
discord_token = { env = "BRIG_DISCORD_TOKEN" }
```

Note: `entrypoint` already points to `/usr/local/bin/brig-discord`, which is
where the port will install the binary.

### brig's Makefile (reference)

```makefile
PREFIX?=    /usr/local
BINDIR=     ${PREFIX}/bin
MANDIR=     ${PREFIX}/share/man
SHAREDIR=   ${PREFIX}/share/brig
RCDIR=      ${PREFIX}/etc/rc.d

CARGO?=     cargo
CARGO_FLAGS=    --release

.PHONY: all build install clean test man

all: build

build:
    ${CARGO} build ${CARGO_FLAGS}

test:
    ${CARGO} test

install: build
    install -m 0755 target/release/brig ${BINDIR}/brig
    install -d ${SHAREDIR}/skills
    # ... man pages, rc.d script ...

clean:
    ${CARGO} clean

uninstall:
    rm -f ${BINDIR}/brig
    # ... man pages, rc.d script ...
```

### brig's rc.d script (reference)

```sh
#!/bin/sh
#
# PROVIDE: brig
# REQUIRE: LOGIN NETWORKING
# KEYWORD: shutdown
#
# Add the following lines to /etc/rc.conf to enable brig:
#
#   brig_enable="YES"
#   brig_user="jim"
#   brig_config="/home/jim/.brig/brig.conf"

. /etc/rc.subr

name=brig
rcvar=brig_enable

load_rc_config $name

: ${brig_enable:="NO"}
: ${brig_user:="root"}
: ${brig_config:="/usr/local/etc/brig.conf"}
: ${brig_flags:=""}

pidfile="/var/run/${name}.pid"
command="/usr/local/bin/brig"
command_args="-d -c ${brig_config} ${brig_flags}"

start_precmd="brig_prestart"

brig_prestart()
{
    local data_dir
    data_dir=$(grep '^data_dir=' "${brig_config}" 2>/dev/null | cut -d= -f2)
    data_dir="${data_dir:-/home/${brig_user}/.brig}"

    install -d -o "${brig_user}" -g "$(id -gn ${brig_user})" "${data_dir}"
    install -d -o "${brig_user}" -g "$(id -gn ${brig_user})" "${data_dir}/sock"
    install -d -o "${brig_user}" -g "$(id -gn ${brig_user})" "${data_dir}/skills"
    install -d -o "${brig_user}" -g "$(id -gn ${brig_user})" "${data_dir}/proposals"
}

run_rc_command "$1"
```

## Deliverables

### 1. Makefile (BSD make)

Create `brig-discord/Makefile` with these targets:

- `all` / `build` — `cargo build --release`
- `test` — `cargo test`
- `install` — installs binary and manifest only (jailed mode):
  - `install -m 0755 target/release/brig-discord ${BINDIR}/brig-discord`
  - `install -d ${SHAREDIR}/skills/discord-gateway`
  - `install -m 0644 manifest.toml ${SHAREDIR}/skills/discord-gateway/manifest.toml`
  - Print post-install message about `brig secret set` + `brig skill enable`
- `install-service` — installs the host-mode rc.d script:
  - `install -m 0755 scripts/rc.d/brig_discord ${RCDIR}/brig_discord`
  - Print message about rc.conf configuration
- `uninstall` — removes binary, manifest directory, rc.d script (if present)
- `clean` — `cargo clean`

Variables (matching brig's conventions):
```makefile
PREFIX?=    /usr/local
BINDIR=     ${PREFIX}/bin
SHAREDIR=   ${PREFIX}/share/brig
RCDIR=      ${PREFIX}/etc/rc.d
CARGO?=     cargo
CARGO_FLAGS=    --release
```

### 2. rc.d script for host mode

Create `brig-discord/scripts/rc.d/brig_discord`:

```sh
#!/bin/sh
#
# PROVIDE: brig_discord
# REQUIRE: brig NETWORKING
# KEYWORD: shutdown
#
# Add the following lines to /etc/rc.conf to enable brig-discord:
#
#   brig_discord_enable="YES"
#   brig_discord_token="your-bot-token"     # or use brig secret
#   brig_discord_user="jim"                 # user to run as
#
# Optional:
#   brig_discord_socket="/var/brig/sock/brig.sock"
#   brig_discord_name="discord-gateway"
#   brig_discord_prefix="discord"
#   brig_discord_flags=""

. /etc/rc.subr

name=brig_discord
rcvar=brig_discord_enable

load_rc_config $name

: ${brig_discord_enable:="NO"}
: ${brig_discord_user:="root"}
: ${brig_discord_token:=""}
: ${brig_discord_socket:="/var/brig/sock/brig.sock"}
: ${brig_discord_name:="discord-gateway"}
: ${brig_discord_prefix:="discord"}
: ${brig_discord_flags:=""}

pidfile="/var/run/${name}.pid"
command="/usr/local/bin/brig-discord"
command_args="${brig_discord_flags}"

start_precmd="brig_discord_prestart"

brig_discord_prestart()
{
    if [ -z "${brig_discord_token}" ]; then
        err 1 "brig_discord_token is not set in /etc/rc.conf"
    fi
    export BRIG_DISCORD_TOKEN="${brig_discord_token}"
    export BRIG_SOCKET="${brig_discord_socket}"
    export BRIG_GATEWAY_NAME="${brig_discord_name}"
    export BRIG_SESSION_PREFIX="${brig_discord_prefix}"
}

run_rc_command "$1"
```

Key points:
- `REQUIRE: brig NETWORKING` — brig daemon must be running first
- `KEYWORD: shutdown` — clean stop on system shutdown
- Token is required; prestart fails if missing
- All env vars configurable via rc.conf
- Uses `/usr/sbin/daemon` implicitly via rc.subr (brig-discord is a
  foreground process, so rc.subr will background it)

Note: brig-discord runs in the foreground (infinite loop). The rc.d script
needs to background it.  Check whether rc.subr handles this automatically
or whether `daemon(8)` wrapping is needed (like the enclave daemon rc.d
template uses).  If daemon wrapping is needed:

```sh
start_cmd="brig_discord_start"

brig_discord_start()
{
    brig_discord_prestart || return 1
    /usr/sbin/daemon -f -p ${pidfile} -u ${brig_discord_user} \
        /usr/bin/env \
        BRIG_DISCORD_TOKEN="${brig_discord_token}" \
        BRIG_SOCKET="${brig_discord_socket}" \
        BRIG_GATEWAY_NAME="${brig_discord_name}" \
        BRIG_SESSION_PREFIX="${brig_discord_prefix}" \
        ${command} ${command_args}
}
```

Look at `brig/templates/enclave_daemon.rc.d` for the daemon(8) pattern
already used in this project.

### 3. Port skeleton

Create `brig-discord/port/` with the following files.  These are not
functional without a distfile URL and valid distinfo, but they establish
the structure for when the project is ready for the ports tree or a local
poudriere build.

**port/Makefile:**
```makefile
PORTNAME=       brig-discord
DISTVERSION=    0.1.0
CATEGORIES=     net-im

MAINTAINER=     jim@example.com
COMMENT=        Discord gateway for Brig
WWW=            https://github.com/jmspring/brig-discord

LICENSE=        BSD2CLAUSE
LICENSE_FILE=   ${WRKSRC}/LICENSE

RUN_DEPENDS=    brig:sysutils/brig

USES=           cargo

PLIST_FILES=    bin/brig-discord \
                share/brig/skills/discord-gateway/manifest.toml

post-install:
    @${MKDIR} ${STAGEDIR}${PREFIX}/share/brig/skills/discord-gateway
    ${INSTALL_DATA} ${WRKSRC}/manifest.toml \
        ${STAGEDIR}${PREFIX}/share/brig/skills/discord-gateway/manifest.toml
```

**port/pkg-descr:**
```
Discord gateway for Brig.  Bridges Discord bot messages to Brig's unix
domain socket for LLM-driven task execution.

Synchronous.  No async runtime.  No Discord bot framework.
```

**port/pkg-plist:**
```
bin/brig-discord
share/brig/skills/discord-gateway/manifest.toml
```

**port/pkg-message:**
```
[
{ type: install
  message: <<EOM
To use brig-discord in jailed mode (recommended):

    brig secret set discord-gateway.discord_token
    brig skill enable discord-gateway

To use brig-discord as a host service instead:

    1. Copy the rc.d script:
       cp /usr/local/share/brig/skills/discord-gateway/scripts/brig_discord \
          /usr/local/etc/rc.d/brig_discord

    2. Configure /etc/rc.conf:
       sysrc brig_discord_enable=YES
       sysrc brig_discord_token="your-bot-token"
       sysrc brig_discord_user="jim"

    3. Start the service:
       service brig_discord start

Do not run both modes simultaneously with the same session prefix.
EOM
}
]
```

**port/distinfo:**
```
# Placeholder — populate when release tarballs are available
# TIMESTAMP = 1713100000
# SHA256 (brig-discord-0.1.0.tar.gz) = ???
# SIZE (brig-discord-0.1.0.tar.gz) = ???
```

### 4. Update README.md

Add an "Install" section before "Manual Run" that documents the BSD make
workflow:

```markdown
## Install

```sh
make                     # build release binary
sudo make install        # install binary + skill manifest
```

This installs:
- `/usr/local/bin/brig-discord`
- `/usr/local/share/brig/skills/discord-gateway/manifest.toml`

Then enable via brig (jailed):

```sh
brig secret set discord-gateway.discord_token
brig skill enable discord-gateway
```

Or as a host service (no jail):

```sh
sudo make install-service    # install rc.d script
sudo sysrc brig_discord_enable=YES
sudo sysrc brig_discord_token="your-bot-token"
sudo sysrc brig_discord_user="jim"
sudo service brig_discord start
```
```

Replace the existing "Install as Brig Skill" section.  The "Manual Run"
section stays as-is for development/testing.

### 5. Update docs/GUIDE.md

The "Step 2: Install brig-discord" section currently documents the manual
`brig skill add` + `brig skill enable` flow.  Update to show `make install`
as the primary path, with `brig skill enable` as the next step.  The manual
`brig skill add ./` path can remain as an alternative for development.

### 6. Create scripts/ directory

```
brig-discord/scripts/
└── rc.d/
    └── brig_discord
```

## File checklist

| File | Action |
|------|--------|
| `Makefile` | Create |
| `scripts/rc.d/brig_discord` | Create |
| `port/Makefile` | Create |
| `port/pkg-descr` | Create |
| `port/pkg-plist` | Create |
| `port/pkg-message` | Create |
| `port/distinfo` | Create (placeholder) |
| `README.md` | Update install section |
| `docs/GUIDE.md` | Update install steps |

## Verification

- `make && sudo make install` puts binary and manifest in correct paths
- `brig skill list` shows `discord-gateway` discovered at `/usr/local/share/brig/skills/`
- `brig skill enable discord-gateway` succeeds (finds manifest, creates jail)
- `sudo make install-service` installs rc.d script
- `service brig_discord start` works in host mode (with token configured)
- `make clean && make` rebuilds from scratch
- `sudo make uninstall` removes all installed files

## Notes

- The port skeleton in `port/` is not functional without distinfo.  It is
  scaffolding for future submission to the ports tree or local poudriere.
- The `USES=cargo` directive in the port Makefile handles Rust build
  integration with the ports framework (fetching crate dependencies,
  setting CARGO_HOME, etc.).
- Multi-bot setup (contrib manifests) continues to work — each instance
  needs its own `brig skill add --manifest contrib/manifest-ops.toml` etc.
  The port only installs the default single-bot manifest.
- The rc.d script needs to handle backgrounding.  brig-discord is a
  foreground process.  Check `brig/templates/enclave_daemon.rc.d` for the
  `daemon(8)` pattern.
