# NOT FOR SUBMISSION — scaffolding only.
# This port skeleton is not functional without valid distinfo.
# Do not submit to the FreeBSD ports tree.

PORTNAME=	brig-discord
DISTVERSION=	0.1.0
CATEGORIES=	net-im

MAINTAINER=	jim@example.com
COMMENT=	Discord gateway for Brig
WWW=		https://github.com/jmspring/brig-discord

LICENSE=	BSD2CLAUSE
LICENSE_FILE=	${WRKSRC}/LICENSE

RUN_DEPENDS=	brig:sysutils/brig

USES=		cargo

PLIST_FILES=	bin/brig-discord \
		share/brig/skills/discord-gateway/manifest.toml

post-install:
	@${MKDIR} ${STAGEDIR}${PREFIX}/share/brig/skills/discord-gateway
	${INSTALL_DATA} ${WRKSRC}/manifest.toml \
	    ${STAGEDIR}${PREFIX}/share/brig/skills/discord-gateway/manifest.toml
