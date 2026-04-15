# Makefile for brig-discord
# Uses BSD make conventions. Run with `make` on FreeBSD.

PREFIX?=	/usr/local
BINDIR=		${PREFIX}/bin
SHAREDIR=	${PREFIX}/share/brig
RCDIR=		${PREFIX}/etc/rc.d

CARGO?=		cargo
CARGO_FLAGS=	--release

.PHONY: all build install install-service clean test uninstall

all: build

build:
	${CARGO} build ${CARGO_FLAGS}

test:
	${CARGO} test

install: build
	install -m 0755 target/release/brig-discord ${BINDIR}/brig-discord
	install -d ${SHAREDIR}/skills/discord-gateway
	install -m 0644 manifest.toml ${SHAREDIR}/skills/discord-gateway/manifest.toml
	@echo ""
	@echo "Installed. To enable in a brig jail:"
	@echo "  brig secret set discord-gateway.discord_token"
	@echo "  brig skill enable discord-gateway"

install-service:
	install -m 0755 scripts/rc.d/brig_discord ${RCDIR}/brig_discord
	@echo ""
	@echo "rc.d script installed. Configure /etc/rc.conf:"
	@echo '  sysrc brig_discord_enable=YES'
	@echo '  sysrc brig_discord_token="your-bot-token"'
	@echo '  sysrc brig_discord_user="jim"'

clean:
	${CARGO} clean

uninstall:
	rm -f ${BINDIR}/brig-discord
	rm -rf ${SHAREDIR}/skills/discord-gateway
	rm -f ${RCDIR}/brig_discord
